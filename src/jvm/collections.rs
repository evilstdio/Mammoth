use super::{
    ARRAY_LIST, Binding, Emitter, Error, Expr, Kind, ListKind, Local, Method, Name, Statement, at,
    instruction, push_int,
};

const ITERATOR: &str = "java/util/Iterator";
impl<'a> Emitter<'a> {
    /// `sumOf` adds whatever its block returns: an Int, a Long or a Double.
    pub(super) fn sum_of(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let saved = method.snapshot();
        let first = self.method_call_resolved(method, receiver, name, args, base, None);
        if first.is_ok() {
            return first;
        }
        method.restore(saved);
        for text in ["__sumOfLong", "__sumOfDouble"] {
            let saved = method.snapshot();
            let attempt = Name {
                text,
                offset: name.offset,
            };
            let result = self.method_call_resolved(method, receiver, attempt, args, base, None);
            if result.is_ok() {
                return result;
            }
            method.restore(saved);
        }
        first
    }

    /// Whether `kind` is a Mammoth class that declares `iterator()` without
    /// being a Java `Iterable`, which the usual loop already handles.
    pub(super) fn has_iterator_method(&self, kind: Kind) -> bool {
        let symbols = self.symbols;
        symbols
            .types
            .native_class(kind.non_null())
            .is_some_and(|class| {
                matches!(kind, Kind::Object(_) | Kind::Applied(_))
                    && symbols
                        .find_method(class, "iterator")
                        .is_some_and(|(_, signature)| signature.parameters.is_empty())
                    && !symbols.java_generic_views(kind).iter().any(|view| {
                        symbols.types.runtime_class_name(*view).is_some_and(|name| {
                            symbols.types.is_java_subclass(&name, "java/lang/Iterable")
                        })
                    })
            })
    }

    /// `for x in value` as `val it = value.iterator(); while it.hasNext() ...`.
    pub(super) fn loop_iterator_method(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        values: &'a Expr<'a>,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        let named = |text: &'static str| Name {
            text,
            offset: name.offset,
        };
        let holder = Name {
            text: Box::leak(format!("__iter{}", name.offset).into_boxed_str()),
            offset: name.offset,
        };
        let call = |target: Expr<'a>, text: &'static str| {
            Expr::MethodCall(Box::new(target), named(text), Vec::new())
        };
        let mut inner = vec![Statement::Binding(
            name,
            call(Expr::Variable(holder), "next"),
            false,
            None,
        )];
        inner.extend(body.iter().cloned());
        let statements: &'a [Statement<'a>] = Box::leak(
            vec![
                Statement::Binding(holder, call(values.clone(), "iterator"), false, None),
                Statement::While(name, call(Expr::Variable(holder), "hasNext"), inner),
            ]
            .into_boxed_slice(),
        );
        self.scoped_block(method, statements, &[])?;
        Ok(())
    }

    /// Whether values of `kind` have a Mammoth `unaryMinus` method.
    pub(super) fn has_unary_minus(&self, kind: Kind) -> bool {
        self.symbols
            .types
            .native_class(kind.non_null())
            .is_some_and(|class| self.symbols.find_method(class, "unaryMinus").is_some())
    }

    /// `List<Int>(size)`, `List<Text>(size) { ... }` and
    /// `MutableList<Int>(size) { ... }`. A block runs in the library's fill
    /// functions, as an ordinary lambda.
    pub(super) fn sized_collection(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        // Arrays are lists here, so `IntArray(3)` and `Array<Text>(2) { "" }` work too.
        let name = match name.text {
            "IntArray" | "LongArray" | "DoubleArray" | "BooleanArray" | "CharArray"
                if !self.symbols.types.names.contains_key(name.text)
                    && !self.symbols.functions.contains_key(name.text) =>
            {
                let element = match name.text {
                    "IntArray" => "Int",
                    "LongArray" => "Long",
                    "DoubleArray" => "Double",
                    "BooleanArray" => "Bool",
                    _ => "Char",
                };
                Name {
                    text: Box::leak(format!("List<{element}>").into_boxed_str()),
                    offset: name.offset,
                }
            }
            text if text.starts_with("Array<") => Name {
                text: Box::leak(format!("List{}", &text["Array".len()..]).into_boxed_str()),
                offset: name.offset,
            },
            _ => name,
        };
        let growable = name.text.starts_with("MutableList<");
        if !growable && !name.text.starts_with("List<") {
            return Ok(None);
        }
        if growable && args.len() != 2 {
            return Ok(None);
        }
        let Ok(kind) = super::resolve_type(&self.type_scope, name) else {
            return Ok(None);
        };
        let element = match kind {
            Kind::List(list) | Kind::MutableList(list) => list.element(),
            _ => return Ok(None),
        };
        if !growable && self.symbols.types.contains_parameter(element) {
            return Err(at(
                name,
                "Generic code cannot create a List<T>, because the element type is erased. Take the list as a parameter, or use MutableList<T>.",
            ));
        }
        let fill = match args {
            [_, Expr::Lambda(_)] => true,
            // The second argument of the library's own raw allocation.
            [_, Expr::Bool(..)] | [_] => false,
            _ => {
                return Err(at(
                    name,
                    "Give a size and an optional block, such as List<Int>(3) { it * 2 }.",
                ));
            }
        };
        if fill {
            let named = |text: String| Name {
                text: Box::leak(text.into_boxed_str()),
                offset: name.offset,
            };
            let suffix = name.text.split_once('<').map_or("", |(_, rest)| rest);
            let call = if growable {
                Expr::Call(named(format!("__growList<{suffix}")), args.to_vec())
            } else {
                let function = match element {
                    Kind::Int => "__fillInt".to_owned(),
                    Kind::Long => "__fillLong".to_owned(),
                    Kind::Double => "__fillDouble".to_owned(),
                    Kind::Bool => "__fillBool".to_owned(),
                    Kind::Char => "__fillChar".to_owned(),
                    _ => format!("__fillItems<{suffix}"),
                };
                let raw = Expr::Call(name, vec![args[0].clone(), Expr::Bool(name.offset, true)]);
                Expr::Call(named(function), vec![raw, args[1].clone()])
            };
            let call: &'a Expr<'a> = Box::leak(Box::new(call));
            return self.expression_as(method, call, base, Some(kind)).map(Some);
        }
        if args.len() == 1 && element.is_reference() && element == element.non_null() {
            return Err(at(
                name,
                "These items cannot be null, so give a block that makes each one, such as List<Text>(3) { \"\" }.",
            ));
        }
        let actual = self.expression_as(method, &args[0], base, Some(Kind::Int))?;
        self.coerce(method, &args[0], actual, Kind::Int, base)?;
        let Kind::List(list) = kind else {
            unreachable!("only blocks reach this for a mutable list")
        };
        method.max_stack = method.max_stack.max(base + 1);
        self.new_array(method, list);
        Ok(Some(kind))
    }

    pub(super) fn is_mutable_list_constructor(&self, name: Name<'_>) -> bool {
        name.text == "mutableList"
            && !self.symbols.functions.contains_key(name.text)
            && !self.symbols.types.names.contains_key(name.text)
    }

    pub(super) fn mutable_list(
        &mut self,
        method: &mut Method<'a>,
        values: &'a [Expr<'a>],
        base: u16,
        mut element_type: Option<ListKind>,
        offset: usize,
    ) -> Result<Kind, Error> {
        let length = i16::try_from(values.len()).map_err(|_| Error {
            offset,
            message: "This list has too many items.",
        })?;
        let class = self.runtime_class(ARRAY_LIST);
        let constructor = self.runtime_member(ARRAY_LIST, "<init>", "(I)V", 10);
        let add = self.runtime_member(ARRAY_LIST, "add", "(Ljava/lang/Object;)Z", 10);
        method.max_stack = method.max_stack.max(base + 3);
        instruction(&mut method.code, 0xbb, class); // new
        method.code.push(0x59); // dup
        push_int(&mut method.code, length);
        instruction(&mut method.code, 0xb7, constructor); // invokespecial
        for value in values {
            method.code.push(0x59); // dup
            method
                .stack
                .extend([Local::Object(class), Local::Object(class)]);
            let actual = match element_type {
                Some(list) => self.expression_as(method, value, base + 2, Some(list.element()))?,
                None => self.expression(method, value, base + 2)?,
            };
            let list = element_type
                .or_else(|| ListKind::for_element(actual))
                .ok_or_else(|| Error {
                    offset: value.offset(),
                    message: "A mutable list needs elements of a supported non-null type.",
                })?;
            self.coerce(method, value, actual, list.element(), base + 2)?;
            method.stack.truncate(base as usize);
            element_type = Some(list);
            self.box_element(method, list.element());
            instruction(&mut method.code, 0xb6, add);
            method.code.push(0x57); // ArrayList.add returns a boolean.
        }
        Ok(Kind::MutableList(element_type.unwrap_or(ListKind::Text)))
    }

    pub(super) fn mutable_list_method(
        &mut self,
        method: &mut Method<'a>,
        list: ListKind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let element = list.element();
        let one_element = [(element, true)];
        let one_index = [(Kind::Int, false)];
        let index_and_element = [(Kind::Int, false), (element, true)];
        let (parameters, member, descriptor, result, discard): (&[(Kind, bool)], _, _, _, _) =
            match name.text {
                "size" => (&[], "size", "()I", Some(Kind::Int), false),
                "add" => (&one_element, "add", "(Ljava/lang/Object;)Z", None, true),
                "get" => (
                    &one_index,
                    "get",
                    "(I)Ljava/lang/Object;",
                    Some(element),
                    false,
                ),
                "set" => (
                    &index_and_element,
                    "set",
                    "(ILjava/lang/Object;)Ljava/lang/Object;",
                    Some(element),
                    false,
                ),
                "removeAt" => (
                    &one_index,
                    "remove",
                    "(I)Ljava/lang/Object;",
                    Some(element),
                    false,
                ),
                "clear" => (&[], "clear", "()V", None, false),
                // `items -= value` removes the first equal element.
                "-=" => (&one_element, "remove", "(Ljava/lang/Object;)Z", None, true),
                _ => return Err(at(name, "This mutable list has no method with that name.")),
            };
        if parameters.len() != args.len() {
            return Err(at(
                name,
                "This list method has the wrong number of arguments.",
            ));
        }
        method
            .stack
            .push(Local::Object(self.runtime_class(ARRAY_LIST)));
        for (arg, &(expected, boxed)) in args.iter().zip(parameters) {
            let at = method.stack.len() as u16;
            let actual = self.expression_as(method, arg, at, Some(expected))?;
            self.coerce(method, arg, actual, expected, at)?;
            if boxed {
                self.box_element(method, expected);
                method
                    .stack
                    .push(Local::Object(self.runtime_class("java/lang/Object")));
            } else {
                method.push_stack(self.local_type(expected));
            }
        }
        method.stack.truncate(base as usize);
        let reference = self.runtime_member(ARRAY_LIST, member, descriptor, 10);
        instruction(&mut method.code, 0xb6, reference);
        if discard {
            method.code.push(0x57); // pop
        } else if let Some(kind) = result
            && member != "size"
        {
            self.unbox_element(method, kind, base);
        }
        Ok(result)
    }

    pub(super) fn box_value(&mut self, method: &mut Method<'a>, kind: Kind) {
        self.box_element(method, kind);
    }

    fn box_element(&mut self, method: &mut Method<'a>, kind: Kind) {
        if let Some((owner, box_type, _, _)) = primitive_wrapper(kind) {
            let reference = self.runtime_member(owner, "valueOf", box_type, 10);
            instruction(&mut method.code, 0xb8, reference);
        }
    }

    pub(super) fn unbox_element(&mut self, method: &mut Method<'a>, kind: Kind, base: u16) {
        if let Some((owner, _, accessor, descriptor)) = primitive_wrapper(kind) {
            let class = self.runtime_class(owner);
            let reference = self.runtime_member(owner, accessor, descriptor, 10);
            instruction(&mut method.code, 0xc0, class); // checkcast
            instruction(&mut method.code, 0xb6, reference);
            method.max_stack = method.max_stack.max(base + kind.slots());
        } else if matches!(kind, Kind::Nullable(_)) {
            if kind.non_null() != Kind::Any {
                let Local::Object(class) = self.local_type(kind) else {
                    unreachable!("nullable values are references")
                };
                instruction(&mut method.code, 0xc0, class); // checkcast
            }
        } else {
            self.guard_non_null(method, kind, base);
        }
    }

    pub(super) fn loop_mutable(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        list: ListKind,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        let factory = self.runtime_member(ARRAY_LIST, "iterator", "()Ljava/util/Iterator;", 10);
        let has_next = self.runtime_member(ITERATOR, "hasNext", "()Z", 11);
        let next = self.runtime_member(ITERATOR, "next", "()Ljava/lang/Object;", 11);
        let iterator = method.local(Local::Object(self.runtime_class(ITERATOR)));
        let element = list.element();
        let item = method.local(self.local_type(element));
        instruction(&mut method.code, 0xb6, factory);
        method.access(0x3a, iterator);
        let header = method.code.len();
        method.frame(iterator as usize + 1);
        method.access(0x19, iterator);
        instruction(&mut method.code, 0xb9, has_next); // invokeinterface
        method.code.extend([1, 0]);
        let exit = method.code.len();
        instruction(&mut method.code, 0x99, 0); // ifeq
        method.access(0x19, iterator);
        instruction(&mut method.code, 0xb9, next);
        method.code.extend([1, 0]);
        self.unbox_element(method, element, 0);
        method.access(element.store(), item);
        method.variables.push(Binding {
            name: name.text,
            slot: item,
            kind: element,
            mutable: false,
        });
        method.loops.push(super::LoopFrame::new(Some(header)));
        let continues = self.scoped_block(method, body, &[]);
        let frame = method.loops.pop().unwrap();
        method.pop_variable();
        if continues? {
            let back = method.jump();
            method.patch_branch(back, header, name.offset)?;
        }
        method.patch_branch(exit, method.code.len(), name.offset)?;
        method.patch_branches(&frame.breaks, name.offset)?;
        method.frame(iterator as usize);
        method.locals.truncate(iterator as usize);
        Ok(())
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn is_java_map(&self, kind: Kind) -> bool {
        matches!(kind, Kind::Java(_))
            && self
                .symbols
                .types
                .runtime_class_name(kind)
                .is_some_and(|binary| {
                    self.symbols
                        .types
                        .is_java_subclass(&binary, "java/util/Map")
                })
    }

    fn map_value(&self, kind: Kind) -> Kind {
        self.symbols
            .types
            .substitution(kind, "java/util/Map")
            .get("V")
            .copied()
            .unwrap_or(Kind::Any)
    }

    /// The type a key of this map must have. Java maps accept null keys.
    fn map_key(&self, kind: Kind) -> Kind {
        let key = self
            .symbols
            .types
            .substitution(kind, "java/util/Map")
            .get("K")
            .copied()
            .unwrap_or(Kind::Any);
        key.nullable().unwrap_or(key)
    }

    /// `map[key]` on a Java map that is on the stack.
    pub(super) fn map_get(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        key: &'a Expr<'a>,
        base: u16,
        allow_null: bool,
    ) -> Result<Kind, Error> {
        let value = self.map_value(kind);
        method.stack.push(self.local_type(kind));
        let key_type = self.map_key(kind);
        let key_kind = self.expression_as(method, key, base + 1, Some(key_type))?;
        self.coerce(method, key, key_kind, key_type, base + 1)?;
        self.box_element(method, key_type);
        method.stack.pop();
        method.max_stack = method.max_stack.max(base + 1 + key_kind.slots());
        let get = self.runtime_member(
            "java/util/Map",
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            11,
        );
        instruction(&mut method.code, 0xb9, get); // invokeinterface
        method.code.extend([2, 0]);
        if allow_null {
            let nullable = value.nullable().unwrap_or(value);
            if nullable.non_null() != Kind::Any {
                self.cast_top(method, Kind::Any, nullable, base);
            }
            return Ok(nullable);
        }
        if value.is_reference() {
            self.guard_non_null(method, Kind::Any, base);
        }
        self.cast_top(method, Kind::Any, value, base);
        Ok(value)
    }

    /// `map[key] = value` (or `map[key] += value`) on a Java map on the stack.
    pub(super) fn map_put(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        key: &'a Expr<'a>,
        operator: Option<super::Arithmetic>,
        value: &'a Expr<'a>,
    ) -> Result<(), Error> {
        let element = self.map_value(kind);
        let receiver = self.local_type(kind);
        method.stack.push(receiver);
        let key_type = self.map_key(kind);
        let key_kind = self.expression_as(method, key, 1, Some(key_type))?;
        self.coerce(method, key, key_kind, key_type, 1)?;
        self.box_element(method, key_type);
        let object = Local::Object(self.runtime_class("java/lang/Object"));
        method.stack.push(object);
        method.max_stack = method.max_stack.max(1 + key_kind.slots());
        let result = if let Some(operator) = operator {
            method.code.push(0x5c); // dup2
            method.max_stack = method.max_stack.max(4);
            let get = self.runtime_member(
                "java/util/Map",
                "get",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
                11,
            );
            instruction(&mut method.code, 0xb9, get);
            method.code.extend([2, 0]);
            self.cast_top(method, Kind::Any, element, 2);
            self.arithmetic_tail(method, value, element, &[(operator, value)], 2)?
        } else {
            self.expression_as(method, value, 2, Some(element))?
        };
        if result != element && element != Kind::Any {
            self.coerce(method, value, result, element, 2)?;
        }
        let stored = if element == Kind::Any {
            result
        } else {
            element
        };
        self.box_element(method, stored);
        method.max_stack = method.max_stack.max(2 + stored.slots());
        method.stack.clear();
        let put = self.runtime_member(
            "java/util/Map",
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            11,
        );
        instruction(&mut method.code, 0xb9, put);
        method.code.extend([3, 0]);
        method.code.push(0x57); // pop the previous value
        Ok(())
    }

    /// Loops over any Java `Iterable`, such as a collection or `map.keySet()`.
    pub(super) fn loop_iterable(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        element: Kind,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        let factory = self.runtime_member(
            "java/lang/Iterable",
            "iterator",
            "()Ljava/util/Iterator;",
            11,
        );
        let has_next = self.runtime_member(ITERATOR, "hasNext", "()Z", 11);
        let next = self.runtime_member(ITERATOR, "next", "()Ljava/lang/Object;", 11);
        let iterator = method.local(Local::Object(self.runtime_class(ITERATOR)));
        let item = method.local(self.local_type(element));
        instruction(&mut method.code, 0xb9, factory);
        method.code.extend([1, 0]);
        method.access(0x3a, iterator);
        let header = method.code.len();
        method.frame(iterator as usize + 1);
        method.access(0x19, iterator);
        instruction(&mut method.code, 0xb9, has_next);
        method.code.extend([1, 0]);
        let exit = method.code.len();
        instruction(&mut method.code, 0x99, 0); // ifeq
        method.access(0x19, iterator);
        instruction(&mut method.code, 0xb9, next);
        method.code.extend([1, 0]);
        self.cast_top(method, Kind::Any, element, 0);
        method.access(element.store(), item);
        method.max_stack = method.max_stack.max(2);
        method.variables.push(Binding {
            name: name.text,
            slot: item,
            kind: element,
            mutable: false,
        });
        method.loops.push(super::LoopFrame::new(Some(header)));
        let continues = self.scoped_block(method, body, &[]);
        let frame = method.loops.pop().unwrap();
        method.pop_variable();
        if continues? {
            let back = method.jump();
            method.patch_branch(back, header, name.offset)?;
        }
        method.patch_branch(exit, method.code.len(), name.offset)?;
        method.patch_branches(&frame.breaks, name.offset)?;
        method.frame(iterator as usize);
        method.locals.truncate(iterator as usize);
        Ok(())
    }
}

fn primitive_wrapper(
    kind: Kind,
) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match kind {
        Kind::Int => Some((
            "java/lang/Integer",
            "(I)Ljava/lang/Integer;",
            "intValue",
            "()I",
        )),
        Kind::Bool => Some((
            "java/lang/Boolean",
            "(Z)Ljava/lang/Boolean;",
            "booleanValue",
            "()Z",
        )),
        Kind::Long => Some(("java/lang/Long", "(J)Ljava/lang/Long;", "longValue", "()J")),
        Kind::Char => Some((
            "java/lang/Character",
            "(C)Ljava/lang/Character;",
            "charValue",
            "()C",
        )),
        Kind::Double => Some((
            "java/lang/Double",
            "(D)Ljava/lang/Double;",
            "doubleValue",
            "()D",
        )),
        _ => None,
    }
}
