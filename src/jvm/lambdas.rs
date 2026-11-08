//! Lambdas and function values.
//!
//! A lambda's body becomes a private static method of the enclosing class.
//! Captured variables (and `this`) become its leading parameters. The function
//! object is created with `invokedynamic` and `LambdaMetafactory`, as javac
//! does, so it implements the target Java interface directly.

use std::collections::HashSet;

use super::{
    Binding, Emitter, Error, Expr, Kind, Lambda, Local, Method, Name, at, globals, instruction,
    interop::JType, resolve_type, shorts,
};

const WILDCARD_ESCAPE: &str = "This value has a Java wildcard type, such as List<? extends Number>, so a lambda cannot return it without a declared result type. Copy it first, such as ArrayList<Number>(value).";

/// The interface a lambda implements and the types it must accept and return.
struct Target {
    kind: Option<Kind>,
    raw_parameters: bool,
    interface: String,
    method: String,
    /// The interface method's descriptor after generic erasure.
    erased: String,
    sam_parameters: Vec<JType>,
    sam_result: JType,
    parameters: Vec<Kind>,
    /// `None` when the result type is not known yet.
    result: Option<Option<Kind>>,
}

impl<'a> Emitter<'a> {
    fn lambda_target(&self, expected: Option<Kind>) -> Option<Target> {
        let types = &self.symbols.types;
        match expected.map(Kind::non_null)? {
            kind @ Kind::Function(index) => {
                let signature = types.function_type(index);
                let (interface, method, erased) = signature.interface()?;
                let object = || JType::Class("java/lang/Object".into());
                Some(Target {
                    kind: Some(kind),
                    raw_parameters: false,
                    interface: interface.into(),
                    method: method.into(),
                    erased: erased.into(),
                    sam_parameters: signature.parameters.iter().map(|_| object()).collect(),
                    sam_result: if signature.result.is_some() {
                        object()
                    } else {
                        JType::Void
                    },
                    parameters: signature.parameters.clone(),
                    result: Some(signature.result),
                })
            }
            kind @ Kind::Java(_) => {
                let interface = types.runtime_class_name(kind)?;
                if !types.is_java_interface(&interface) {
                    return None;
                }
                let mut methods = types.java_abstract_methods(&interface);
                if methods.len() != 1 {
                    return None;
                }
                let sam = methods.pop().unwrap();
                // Use the interface's type arguments, as in Consumer<Text>.
                let map = types.substitution(kind, &sam.owner);
                let generic = sam
                    .signature
                    .as_deref()
                    .and_then(super::generics::parse_method_signature);
                let parameters = sam
                    .parameters
                    .iter()
                    .enumerate()
                    .map(|(position, jtype)| {
                        generic
                            .as_ref()
                            .and_then(|signature| signature.parameters.get(position))
                            .and_then(|gtype| types.generic_kind(gtype, &map))
                            .or_else(|| types.kind_of(jtype))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let result = generic
                    .as_ref()
                    .and_then(|signature| types.generic_kind(&signature.result, &map))
                    .or_else(|| types.kind_of(&sam.result));
                Some(Target {
                    kind: Some(kind),
                    raw_parameters: types.type_arguments(kind).is_empty()
                        && !types.class_type_parameters(&interface).is_empty(),
                    interface,
                    method: sam.name.clone(),
                    erased: sam.descriptor.clone(),
                    result: Some(result),
                    sam_parameters: sam.parameters,
                    sam_result: sam.result,
                    parameters,
                })
            }
            _ => None,
        }
    }

    /// Whether a Java parameter type can receive a lambda with `arity`
    /// parameters (`None` when the lambda did not list its parameters).
    /// Whether a lambda passed for this parameter returns a value.
    pub(super) fn lambda_returns_value(&self, target: &JType) -> bool {
        let JType::Class(name) = target else {
            return false;
        };
        let kind = self.symbols.types.java_kind(name);
        self.lambda_target(Some(kind))
            .is_some_and(|target| target.sam_result != JType::Void)
    }

    pub(super) fn accepts_lambda(&self, target: &JType, arity: Option<usize>) -> bool {
        let JType::Class(name) = target else {
            return false;
        };
        let kind = self.symbols.types.java_kind(name);
        self.lambda_target(Some(kind))
            .is_some_and(|target| match arity {
                Some(arity) => target.parameters.len() == arity,
                None => target.parameters.len() <= 1,
            })
    }

    pub(super) fn check_java_lambda_parameters(
        &self,
        lambda: &Lambda<'_>,
        expected: Kind,
    ) -> Result<(), Error> {
        let Some(target) = self.lambda_target(Some(expected)) else {
            return Ok(());
        };
        for ((_, annotation), wanted) in lambda.parameters.iter().zip(&target.parameters) {
            if let Some(annotation) = annotation {
                let declared = resolve_type(&self.type_scope, *annotation)?;
                if !self.lambda_input_fits(declared, *wanted, target.raw_parameters) {
                    return Err(at(
                        *annotation,
                        "This parameter type does not match where the lambda is used.",
                    ));
                }
            }
        }
        Ok(())
    }

    fn lambda_input_fits(&self, declared: Kind, wanted: Kind, raw: bool) -> bool {
        self.symbols.types.same_kind(declared, wanted)
            || raw
            || self.symbols.fits(wanted, declared)
            // Java can pass null for a boxed input, as Map.compute does.
            || (matches!(declared, Kind::Nullable(_))
                && self.symbols.types.same_kind(declared.non_null(), wanted))
    }

    /// Infer a Java SAM lambda's result while preserving the enclosing stack.
    /// Generated helpers are retained, as in native generic inference.
    pub(super) fn probe_java_lambda_result(
        &mut self,
        method: &mut Method<'a>,
        lambda: &'a Lambda<'a>,
        target: Kind,
    ) -> Result<Option<Kind>, Error> {
        let snapshot = method.snapshot();
        let saved = (
            self.allow_null,
            self.generic_receiver,
            self.last_generic_result,
            self.infer_lambda_result,
        );
        self.infer_lambda_result = true;
        let result = self.lambda(method, lambda, Some(target), method.stack.len() as u16);
        method.restore(snapshot);
        (
            self.allow_null,
            self.generic_receiver,
            self.last_generic_result,
            self.infer_lambda_result,
        ) = saved;
        let kind = result?;
        let Kind::Function(index) = kind else {
            return Ok(None);
        };
        Ok(self.symbols.types.function_type(index).result)
    }

    pub(super) fn lambda(
        &mut self,
        method: &mut Method<'a>,
        lambda: &'a Lambda<'a>,
        expected: Option<Kind>,
        base: u16,
    ) -> Result<Kind, Error> {
        // A lambda body runs later, when every field has its value.
        let pending = (
            std::mem::take(&mut self.pending_fields),
            std::mem::take(&mut self.pending_statics),
        );
        let result = self.lambda_body(method, lambda, expected, base);
        (self.pending_fields, self.pending_statics) = pending;
        result
    }

    fn lambda_body(
        &mut self,
        method: &mut Method<'a>,
        lambda: &'a Lambda<'a>,
        expected: Option<Kind>,
        base: u16,
    ) -> Result<Kind, Error> {
        let mut target = self.lambda_target(expected);
        if std::mem::take(&mut self.infer_lambda_result)
            && let Some(target) = &mut target
        {
            target.kind = None;
            target.result = None;
        }
        let offset = Name {
            text: "{",
            offset: lambda.offset,
        };
        // Parameters, with the implicit `it` for a single unnamed parameter.
        let mut parameters: Vec<(&'a str, Kind)> = Vec::new();
        if let Some(target) = &target
            && lambda.parameters.is_empty()
            && !lambda.explicit
        {
            match target.parameters.as_slice() {
                [] => {}
                [kind] => parameters.push(("it", *kind)),
                _ => {
                    return Err(at(
                        offset,
                        "Name this lambda's parameters, such as { a, b -> ... }.",
                    ));
                }
            }
        } else {
            if let Some(target) = &target
                && target.parameters.len() != lambda.parameters.len()
            {
                return Err(at(
                    offset,
                    "This lambda has the wrong number of parameters for where it is used.",
                ));
            }
            for (position, &(name, annotation)) in lambda.parameters.iter().enumerate() {
                let declared = annotation
                    .map(|annotation| resolve_type(&self.type_scope, annotation))
                    .transpose()?;
                let wanted = target.as_ref().map(|target| target.parameters[position]);
                let kind = match (declared, wanted) {
                    (Some(declared), Some(wanted)) => {
                        let fits = self.lambda_input_fits(
                            declared,
                            wanted,
                            target.as_ref().is_some_and(|target| target.raw_parameters),
                        );
                        if !fits {
                            return Err(at(
                                annotation.unwrap(),
                                "This parameter type does not match where the lambda is used.",
                            ));
                        }
                        declared
                    }
                    (Some(declared), None) => declared,
                    (None, Some(wanted)) => wanted,
                    (None, None) => {
                        return Err(at(
                            name,
                            "Give this lambda parameter a type, such as { x: Int -> x * 2 }.",
                        ));
                    }
                };
                parameters.push((name.text, kind));
            }
        }

        // Variables from the enclosing method that the body uses.
        let mut used = HashSet::new();
        globals::collect_statements(&lambda.body, &mut used);
        if let Some(value) = &lambda.value {
            globals::collect_expr(value, &mut used);
        }
        let mut captures: Vec<Binding<'a>> = used
            .into_iter()
            .filter(|name| *name != "this" && !parameters.iter().any(|(param, _)| param == name))
            .filter_map(|name| {
                let mut binding = method.binding(Name {
                    text: name,
                    offset: 0,
                })?;
                if let Some(&(_, narrowed)) = method
                    .type_facts
                    .iter()
                    .rev()
                    .find(|(slot, _)| *slot == binding.slot)
                {
                    binding.kind = narrowed;
                } else if method.non_null_slots.contains(&binding.slot) {
                    binding.kind = binding.kind.non_null();
                }
                Some(binding)
            })
            .collect();
        captures.sort_by_key(|binding| binding.slot);
        let this_binding = method
            .binding(Name {
                text: "this",
                offset: 0,
            })
            .filter(|_| {
                self.this_ready
                    && (self.current_class.is_some() || self.current_interface.is_some())
            });

        // The body, as a private static method.
        let mut body = Method::new(Vec::new());
        let mut implementation = String::from("(");
        let mut capture_descriptors = String::new();
        if let Some(this) = this_binding {
            let slot = body.local(self.local_type(this.kind));
            body.variables.push(Binding {
                slot,
                mutable: false,
                ..this
            });
            capture_descriptors.push_str(&this.kind.descriptor(&self.symbols.types));
        }
        for capture in &captures {
            let boxed = method.boxed_slots.contains(&capture.slot);
            let stored = if boxed {
                Kind::List(super::ListKind::for_element(capture.kind).unwrap())
            } else {
                body.captured.push(capture.name);
                capture.kind
            };
            let slot = body.local(self.local_type(stored));
            if boxed {
                body.boxed_slots.push(slot);
            }
            body.variables.push(Binding {
                name: capture.name,
                slot,
                kind: capture.kind,
                mutable: boxed,
            });
            capture_descriptors.push_str(&stored.descriptor(&self.symbols.types));
        }
        implementation.push_str(&capture_descriptors);
        for &(name, kind) in &parameters {
            let slot = body.local(self.local_type(kind));
            body.variables.push(Binding {
                name,
                slot,
                kind,
                mutable: false,
            });
            implementation.push_str(&kind.descriptor(&self.symbols.types));
        }
        let declared_result = target.as_ref().and_then(|target| target.result);
        body.result = declared_result.flatten();
        self.mark_line(&mut body, offset.offset);
        let falls_through = self.block(&mut body, &lambda.body)?;
        if let Some(value) = &lambda.value {
            self.mark_line(&mut body, value.offset());
        }
        let result = if falls_through {
            match (&lambda.value, declared_result) {
                (Some(value), Some(Some(result))) => {
                    let kind = self.expression_as(&mut body, value, 0, Some(result))?;
                    if !kind.is_reference()
                        && matches!(
                            target.as_ref().and_then(|target| target.kind),
                            Some(Kind::Java(_))
                        )
                        && matches!(result.non_null(), Kind::Java(_))
                        && self.symbols.fits(kind, result)
                    {
                        // Concrete Java SAM signatures can name boxed Number
                        // types even though the Mammoth expression is primitive.
                        self.box_value(&mut body, kind);
                    } else {
                        self.coerce(&mut body, value, kind, result, 0)?;
                    }
                    body.code.push(result.return_opcode());
                    Some(result)
                }
                (Some(value), Some(None)) => {
                    if let Some(kind) = self.value_or_nothing(&mut body, value)? {
                        body.code.push(kind.pop_opcode());
                    }
                    body.code.push(0xb1);
                    None
                }
                (Some(value), None) => {
                    let kind = self.value_or_nothing(&mut body, value)?;
                    if kind == Some(Kind::Null) {
                        return Err(at(offset, "Give this lambda a result type."));
                    }
                    // Every call would return a new value of one capture
                    // type, letting values of different element types mix.
                    if kind.is_some_and(|kind| self.symbols.types.has_capture(kind)) {
                        return Err(Error {
                            offset: value.offset(),
                            message: WILDCARD_ESCAPE,
                        });
                    }
                    body.code.push(kind.map_or(0xb1, Kind::return_opcode));
                    kind
                }
                (None, Some(Some(_))) => {
                    return Err(at(offset, "This lambda must end with a value."));
                }
                (None, _) => {
                    body.code.push(0xb1);
                    None
                }
            }
        } else {
            declared_result.flatten()
        };
        implementation.push(')');
        implementation.push_str(&result.map_or_else(
            || "V".into(),
            |kind| kind.descriptor(&self.symbols.types).into_owned(),
        ));

        let target = match target {
            Some(target) => target,
            None => {
                let kinds = parameters.iter().map(|&(_, kind)| kind).collect();
                let kind = self.symbols.types.function_kind(kinds, result);
                self.lambda_target(Some(kind))
                    .ok_or_else(|| at(offset, "Lambdas can have at most two parameters for now."))?
            }
        };
        let name = format!("lambda${}", self.lambda_count);
        self.lambda_count += 1;
        let name_index = self.pool.ascii(&name);
        let descriptor_index = self.pool.ascii(&implementation);
        body.write(
            &mut self.synthetic_methods,
            0x100a, // private static synthetic
            name_index,
            descriptor_index,
            self.code_name,
            self.stack_map_name,
        )?;
        self.synthetic_count += 1;

        // The type the function object has at this call site.
        let instantiated = {
            let mut text = String::from("(");
            for (jtype, &(_, kind)) in target.sam_parameters.iter().zip(&parameters) {
                text.push_str(&self.instantiated(jtype, kind));
            }
            text.push(')');
            match (&target.sam_result, result) {
                (JType::Void, _) | (_, None) => text.push('V'),
                (jtype, Some(kind)) => text.push_str(&self.instantiated(jtype, kind)),
            }
            text
        };

        // Push captured values, then create the function object.
        let depth = method.stack.len();
        let mut slots = 0;
        if let Some(this) = this_binding {
            method.access(0x19, this.slot);
            method.stack.push(self.local_type(this.kind));
            slots += 1;
        }
        for capture in &captures {
            if method.boxed_slots.contains(&capture.slot) {
                method.access(0x19, capture.slot); // the shared array
                let array = Kind::List(super::ListKind::for_element(capture.kind).unwrap());
                let local = self.local_type(array);
                method.stack.push(local);
                slots += 1;
                continue;
            }
            self.load(
                method,
                Name {
                    text: capture.name,
                    offset: lambda.offset,
                },
                base + slots,
            )?;
            let local = self.local_type(capture.kind);
            method.push_stack(local);
            slots += capture.kind.slots();
        }
        method.stack.truncate(depth);
        method.max_stack = method.max_stack.max(base + slots.max(1));
        let call_site = format!("({capture_descriptors})L{};", target.interface);
        let indy = self.invoke_dynamic(
            &target.method,
            &call_site,
            &target.erased,
            &name,
            &implementation,
            &instantiated,
        );
        instruction(&mut method.code, 0xba, indy); // invokedynamic
        method.code.extend([0, 0]);
        Ok(target.kind.unwrap_or_else(|| {
            let kinds = parameters.iter().map(|&(_, kind)| kind).collect();
            self.symbols.types.function_kind(kinds, result)
        }))
    }

    /// The descriptor for one position of the instantiated method type.
    fn instantiated(&self, jtype: &JType, kind: Kind) -> String {
        if matches!(jtype, JType::Class(_) | JType::Array(_)) {
            match kind {
                Kind::Int => "Ljava/lang/Integer;".into(),
                Kind::Long => "Ljava/lang/Long;".into(),
                Kind::Double => "Ljava/lang/Double;".into(),
                Kind::Bool => "Ljava/lang/Boolean;".into(),
                Kind::Char => "Ljava/lang/Character;".into(),
                kind => kind.descriptor(&self.symbols.types).into_owned(),
            }
        } else {
            kind.descriptor(&self.symbols.types).into_owned()
        }
    }

    /// The constant pool tag for methods of the class being written:
    /// `InterfaceMethodref` inside an interface, `Methodref` otherwise.
    pub(super) fn own_method_tag(&self) -> u8 {
        if self.current_interface.is_some() {
            11
        } else {
            10
        }
    }

    /// Adds a `LambdaMetafactory` call site and returns its constant.
    fn invoke_dynamic(
        &mut self,
        name: &str,
        call_site: &str,
        erased: &str,
        implementation: &str,
        implementation_descriptor: &str,
        instantiated: &str,
    ) -> u16 {
        let metafactory = *self.metafactory.get_or_insert_with(|| {
            let factory = self.pool.class("java/lang/invoke/LambdaMetafactory");
            let reference = self.pool.member(
                10,
                factory,
                "metafactory",
                "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodHandle;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
            );
            method_handle(&mut self.pool, 6, reference)
        });
        let erased_index = self.pool.ascii(erased);
        let erased_type = self.pool.reference(16, &[erased_index]);
        let tag = self.own_method_tag();
        let target = self.pool.member(
            tag,
            self.this_class,
            implementation,
            implementation_descriptor,
        );
        let handle = method_handle(&mut self.pool, 6, target);
        let instantiated_index = self.pool.ascii(instantiated);
        let instantiated_type = self.pool.reference(16, &[instantiated_index]);
        let bootstrap = self.bootstraps.len() as u16;
        self.bootstraps
            .push(vec![metafactory, erased_type, handle, instantiated_type]);
        let name_index = self.pool.ascii(name);
        let type_index = self.pool.ascii(call_site);
        let name_and_type = self.pool.reference(12, &[name_index, type_index]);
        self.pool.reference(18, &[bootstrap, name_and_type])
    }

    /// Emits a lambda's final expression. A call to a function without a
    /// result counts as a statement and gives `None`.
    pub(super) fn value_or_nothing(
        &mut self,
        method: &mut Method<'a>,
        value: &'a Expr<'a>,
    ) -> Result<Option<Kind>, Error> {
        // A branch value can be nested inside a call with operands waiting.
        let base = method.stack.len() as u16;
        match value {
            Expr::Call(name, args)
                if name.text == "print" && !self.symbols.functions.contains_key("print") =>
            {
                self.print_call(method, *name, args)?;
                Ok(None)
            }
            Expr::Call(name, args) => self.call(method, *name, args, base),
            Expr::MethodCall(receiver, name, args) => {
                self.method_call(method, receiver, *name, args, base)
            }
            Expr::When(offset, subject, branches) => {
                self.when_value(method, *offset, subject.as_deref(), branches, None, base)
            }
            Expr::Safe(inner) => self.safe_access(method, inner, base),
            _ => self.expression(method, value, base).map(Some),
        }
    }

    /// Calls a function value of type `Kind::Function(index)` that is on the stack.
    pub(super) fn invoke_function(
        &mut self,
        method: &mut Method<'a>,
        index: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let signature = self.symbols.types.function_type(index);
        let (interface, member, erased) = signature.interface().unwrap();
        if args.len() != signature.parameters.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let class = self.runtime_class(interface);
        method.stack.push(Local::Object(class));
        let object = Local::Object(self.runtime_class("java/lang/Object"));
        for (arg, &expected) in args.iter().zip(&signature.parameters) {
            let at = method.stack.len() as u16;
            let kind = self.expression_as(method, arg, at, Some(expected))?;
            self.coerce(method, arg, kind, expected, at)?;
            self.box_value(method, expected);
            method.max_stack = method.max_stack.max(at + expected.slots());
            method.stack.push(object);
        }
        method.stack.truncate(base as usize);
        let reference = self.runtime_member(interface, member, erased, 11);
        instruction(&mut method.code, 0xb9, reference); // invokeinterface
        method.code.extend([args.len() as u8 + 1, 0]);
        method.max_stack = method.max_stack.max(base + 1);
        if let Some(result) = signature.result {
            if result.is_reference() {
                if result != Kind::Any {
                    let class = self.instance_class(result.non_null());
                    instruction(&mut method.code, 0xc0, class); // checkcast
                }
            } else {
                self.unbox_element(method, result, base);
            }
            method.max_stack = method.max_stack.max(base + result.slots());
        }
        Ok(signature.result)
    }

    /// Calls a variable, field, or top-level variable that holds a function.
    pub(super) fn call_value(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Option<Kind>>, Error> {
        let kind = if let Some(binding) = method.binding(name) {
            binding.kind
        } else if let Some(field) = self.fields.get(name.text) {
            field.kind
        } else if let Some(global) = self.symbols.global(name.text) {
            global.kind
        } else {
            return Ok(None);
        };
        match kind {
            Kind::Function(index) => {
                self.load(method, name, base)?;
                self.invoke_function(method, index, name, args, base)
                    .map(Some)
            }
            Kind::Nullable(super::Reference::Function(_)) => Err(at(
                name,
                "Check for null before calling this function value.",
            )),
            _ => Ok(None),
        }
    }
}

fn method_handle(pool: &mut super::Pool, kind: u8, reference: u16) -> u16 {
    let index = pool.entry(15);
    pool.bytes.push(kind);
    shorts(&mut pool.bytes, &[reference]);
    index
}
