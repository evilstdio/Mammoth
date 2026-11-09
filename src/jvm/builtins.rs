//! Built-in functions and methods on Mammoth's value types (Text, numbers, Char, Bool),
//! plus helpers for turning any value into text.

use super::{
    Comparison, Emitter, Error, Expr, Kind, ListKind, Local, Method, Name, at, instruction,
    load_constant,
};

const STRING: &str = "java/lang/String";
const MATH: &str = "java/lang/Math";

/// How a built-in is implemented once its receiver and arguments are on the stack.
#[derive(Clone, Copy)]
enum Op {
    /// `invokevirtual` on the receiver.
    Virtual(&'static str, &'static str, &'static str),
    /// `invokestatic`, with the receiver (if any) as the first argument.
    Static(&'static str, &'static str, &'static str),
    /// `invokestatic` followed by raw instructions.
    StaticThen(&'static str, &'static str, &'static str, &'static [u8]),
    /// Raw instructions such as a numeric conversion.
    Code(&'static [u8]),
    /// The value is already in the right form.
    Nop,
    /// Split text by a separator argument, matched literally.
    SplitLiteral,
    /// Split text by a fixed regular expression.
    SplitRegex(&'static str),
    /// A generated helper that catches an invalid numeric text conversion.
    ParseNullable(Kind),
}

struct Builtin {
    parameters: &'static [Kind],
    result: Option<Kind>,
    op: Op,
}

const fn builtin(parameters: &'static [Kind], result: Kind, op: Op) -> Builtin {
    Builtin {
        parameters,
        result: Some(result),
        op,
    }
}

fn text_method(name: &str, arity: usize) -> Option<Builtin> {
    use Kind::{Bool, Char, Double, Int, Long, Text};
    const TEXT_LIST: Kind = Kind::List(ListKind::Text);
    const CHAR_LIST: Kind = Kind::List(ListKind::Char);
    Some(match (name, arity) {
        ("toIntOrNull", 0) => builtin(&[], Int.nullable().unwrap(), Op::ParseNullable(Int)),
        ("toLongOrNull", 0) => builtin(&[], Long.nullable().unwrap(), Op::ParseNullable(Long)),
        ("toDoubleOrNull", 0) => {
            builtin(&[], Double.nullable().unwrap(), Op::ParseNullable(Double))
        }
        ("length", 0) => builtin(&[], Int, Op::Virtual(STRING, "length", "()I")),
        ("uppercase", 0) => builtin(
            &[],
            Text,
            Op::Virtual(STRING, "toUpperCase", "()Ljava/lang/String;"),
        ),
        ("lowercase", 0) => builtin(
            &[],
            Text,
            Op::Virtual(STRING, "toLowerCase", "()Ljava/lang/String;"),
        ),
        ("trim", 0) => builtin(
            &[],
            Text,
            Op::Virtual(STRING, "strip", "()Ljava/lang/String;"),
        ),
        ("contains", 1) => builtin(
            &[Text],
            Bool,
            Op::Virtual(STRING, "contains", "(Ljava/lang/CharSequence;)Z"),
        ),
        ("startsWith", 1) => builtin(
            &[Text],
            Bool,
            Op::Virtual(STRING, "startsWith", "(Ljava/lang/String;)Z"),
        ),
        ("endsWith", 1) => builtin(
            &[Text],
            Bool,
            Op::Virtual(STRING, "endsWith", "(Ljava/lang/String;)Z"),
        ),
        ("indexOf", 1) => builtin(
            &[Text],
            Int,
            Op::Virtual(STRING, "indexOf", "(Ljava/lang/String;)I"),
        ),
        ("lastIndexOf", 1) => builtin(
            &[Text],
            Int,
            Op::Virtual(STRING, "lastIndexOf", "(Ljava/lang/String;)I"),
        ),
        ("substring", 1) => builtin(
            &[Int],
            Text,
            Op::Virtual(STRING, "substring", "(I)Ljava/lang/String;"),
        ),
        ("substring", 2) => builtin(
            &[Int, Int],
            Text,
            Op::Virtual(STRING, "substring", "(II)Ljava/lang/String;"),
        ),
        ("replace", 2) => builtin(
            &[Text, Text],
            Text,
            Op::Virtual(
                STRING,
                "replace",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;",
            ),
        ),
        ("split", 1) => builtin(&[Text], TEXT_LIST, Op::SplitLiteral),
        ("lines", 0) => builtin(&[], TEXT_LIST, Op::SplitRegex("\\R")),
        ("words", 0) => builtin(&[], TEXT_LIST, Op::SplitRegex("\\s+")),
        ("isEmpty", 0) => builtin(&[], Bool, Op::Virtual(STRING, "isEmpty", "()Z")),
        ("isBlank", 0) => builtin(&[], Bool, Op::Virtual(STRING, "isBlank", "()Z")),
        ("repeat", 1) => builtin(
            &[Int],
            Text,
            Op::Virtual(STRING, "repeat", "(I)Ljava/lang/String;"),
        ),
        ("get" | "charAt", 1) => builtin(&[Int], Char, Op::Virtual(STRING, "charAt", "(I)C")),
        ("chars", 0) => builtin(&[], CHAR_LIST, Op::Virtual(STRING, "toCharArray", "()[C")),
        ("compareTo", 1) => builtin(
            &[Text],
            Int,
            Op::Virtual(STRING, "compareTo", "(Ljava/lang/String;)I"),
        ),
        ("equalsIgnoreCase", 1) => builtin(
            &[Text],
            Bool,
            Op::Virtual(STRING, "equalsIgnoreCase", "(Ljava/lang/String;)Z"),
        ),
        ("toInt", 0) => builtin(
            &[],
            Int,
            Op::Static("java/lang/Integer", "parseInt", "(Ljava/lang/String;)I"),
        ),
        ("toLong", 0) => builtin(
            &[],
            Long,
            Op::Static("java/lang/Long", "parseLong", "(Ljava/lang/String;)J"),
        ),
        ("toDouble", 0) => builtin(
            &[],
            Double,
            Op::Static("java/lang/Double", "parseDouble", "(Ljava/lang/String;)D"),
        ),
        ("toText", 0) => builtin(&[], Text, Op::Nop),
        _ => return None,
    })
}

fn number_method(kind: Kind, name: &str) -> Option<Builtin> {
    use Kind::{Bool, Char, Double, Int, Long, Text};
    Some(match (kind, name) {
        (Int, "toInt") | (Char, "toInt" | "code") => builtin(&[], Int, Op::Nop),
        (Int, "toLong") => builtin(&[], Long, Op::Code(&[0x85])),
        (Int, "toDouble") => builtin(&[], Double, Op::Code(&[0x87])),
        (Int, "toChar") => builtin(&[], Char, Op::Code(&[0x92])),
        (Int, "toText") => builtin(
            &[],
            Text,
            Op::Static(STRING, "valueOf", "(I)Ljava/lang/String;"),
        ),
        (Int, "abs") => builtin(&[], Int, Op::Static(MATH, "abs", "(I)I")),
        (Long, "toInt") => builtin(&[], Int, Op::Code(&[0x88])),
        (Long, "toLong") => builtin(&[], Long, Op::Nop),
        (Long, "toDouble") => builtin(&[], Double, Op::Code(&[0x8a])),
        (Long, "toText") => builtin(
            &[],
            Text,
            Op::Static(STRING, "valueOf", "(J)Ljava/lang/String;"),
        ),
        (Long, "abs") => builtin(&[], Long, Op::Static(MATH, "abs", "(J)J")),
        (Double, "toInt") => builtin(&[], Int, Op::Code(&[0x8e])),
        (Double, "toLong") => builtin(&[], Long, Op::Code(&[0x8f])),
        (Double, "toDouble") => builtin(&[], Double, Op::Nop),
        (Double, "toText") => builtin(
            &[],
            Text,
            Op::Static(STRING, "valueOf", "(D)Ljava/lang/String;"),
        ),
        (Double, "abs") => builtin(&[], Double, Op::Static(MATH, "abs", "(D)D")),
        (Double, "round") => builtin(
            &[],
            Int,
            Op::StaticThen(MATH, "round", "(D)J", &[0x88]), // l2i
        ),
        (Double, "floor") => builtin(&[], Double, Op::Static(MATH, "floor", "(D)D")),
        (Double, "ceil") => builtin(&[], Double, Op::Static(MATH, "ceil", "(D)D")),
        (Double, "isNaN") => builtin(&[], Bool, Op::Static("java/lang/Double", "isNaN", "(D)Z")),
        (Char, "toChar") => builtin(&[], Char, Op::Nop),
        (Char, "toText") => builtin(
            &[],
            Text,
            Op::Static(STRING, "valueOf", "(C)Ljava/lang/String;"),
        ),
        (Char, "isDigit") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isDigit", "(C)Z"),
        ),
        (Char, "isLetter") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isLetter", "(C)Z"),
        ),
        (Char, "isLetterOrDigit") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isLetterOrDigit", "(C)Z"),
        ),
        (Char, "isWhitespace") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isWhitespace", "(C)Z"),
        ),
        (Char, "isUpperCase") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isUpperCase", "(C)Z"),
        ),
        (Char, "isLowerCase") => builtin(
            &[],
            Bool,
            Op::Static("java/lang/Character", "isLowerCase", "(C)Z"),
        ),
        (Char, "uppercase") => builtin(
            &[],
            Char,
            Op::Static("java/lang/Character", "toUpperCase", "(C)C"),
        ),
        (Char, "lowercase") => builtin(
            &[],
            Char,
            Op::Static("java/lang/Character", "toLowerCase", "(C)C"),
        ),
        (Char, "digitToInt") => builtin(
            &[],
            Int,
            Op::Static("java/lang/Character", "getNumericValue", "(C)I"),
        ),
        (Bool, "toText") => builtin(
            &[],
            Text,
            Op::Static(STRING, "valueOf", "(Z)Ljava/lang/String;"),
        ),
        _ => return None,
    })
}

/// The result type of a built-in method, if `kind` has one named `name`.
pub(super) fn builtin_result(kind: Kind, name: &str, arity: usize) -> Option<Option<Kind>> {
    let spec = match kind {
        Kind::Text => text_method(name, arity),
        Kind::Int | Kind::Long | Kind::Double | Kind::Char | Kind::Bool if arity == 0 => {
            number_method(kind, name)
        }
        _ => None,
    }?;
    Some(spec.result)
}

impl<'a> Emitter<'a> {
    /// Calls a method of a primitive's Java box, such as `compareTo` or
    /// `hashCode` on an `Int` (java.lang.Integer), as Kotlin allows.
    fn boxed_method(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let class = super::Types::boxed_class(kind).expect("primitive kinds have boxes");
        let boxed = self.symbols.types.java_kind(class);
        if self
            .symbols
            .types
            .java_methods(class, name.text, false)
            .is_empty()
        {
            return Err(at(
                name,
                if name.text == "invoke" {
                    "This value is not a function, so it cannot be called."
                } else {
                    "This value has no method with that name."
                },
            ));
        }
        self.box_value(method, kind);
        method.max_stack = method.max_stack.max(base + 1);
        self.java_instance_call(method, boxed, name, args, base, false)
    }

    /// Calls a built-in method on a Text, number, Char, or Bool receiver that is
    /// already on the stack. Returns `None` when `kind` has no built-in methods.
    pub(super) fn builtin_method(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Option<Kind>>, Error> {
        let spec = match kind {
            Kind::Text => text_method(name.text, args.len()),
            Kind::Int | Kind::Long | Kind::Double | Kind::Char | Kind::Bool => {
                let spec = if args.is_empty() {
                    number_method(kind, name.text)
                } else {
                    None
                };
                if spec.is_none() {
                    return self.boxed_method(method, kind, name, args, base).map(Some);
                }
                spec
            }
            _ => return Ok(None),
        };
        let Some(spec) = spec else {
            // Fall back to the methods of java.lang.String.
            return Ok(None);
        };
        method.push_stack(self.local_type(kind));
        self.builtin_arguments(method, name, args, spec.parameters, base + kind.slots())?;
        method.pop_stack();
        self.apply(method, spec.op)?;
        if let Some(result) = spec.result {
            method.max_stack = method.max_stack.max(base + result.slots());
        }
        Ok(Some(spec.result))
    }

    fn builtin_arguments(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        parameters: &[Kind],
        base: u16,
    ) -> Result<(), Error> {
        if args.len() != parameters.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let depth = method.stack.len();
        for (arg, &expected) in args.iter().zip(parameters) {
            let at = method.stack.len() as u16;
            let actual = self.expression_as(method, arg, at, Some(expected))?;
            self.coerce(method, arg, actual, expected, at)?;
            method.push_stack(self.local_type(expected));
        }
        let _ = base;
        method.stack.truncate(depth);
        Ok(())
    }

    fn apply(&mut self, method: &mut Method<'a>, op: Op) -> Result<(), Error> {
        match op {
            Op::Virtual(owner, member, descriptor) => {
                let reference = self.runtime_member(owner, member, descriptor, 10);
                instruction(&mut method.code, 0xb6, reference);
            }
            Op::Static(owner, member, descriptor) => {
                let reference = self.runtime_member(owner, member, descriptor, 10);
                instruction(&mut method.code, 0xb8, reference);
            }
            Op::StaticThen(owner, member, descriptor, code) => {
                let reference = self.runtime_member(owner, member, descriptor, 10);
                instruction(&mut method.code, 0xb8, reference);
                method.code.extend_from_slice(code);
            }
            Op::Code(code) => method.code.extend_from_slice(code),
            Op::ParseNullable(kind) => {
                let parser = self.nullable_parser(kind)?;
                instruction(&mut method.code, 0xb8, parser);
            }
            Op::Nop => {}
            Op::SplitLiteral => {
                let quote = self.runtime_member(
                    "java/util/regex/Pattern",
                    "quote",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    10,
                );
                instruction(&mut method.code, 0xb8, quote);
                self.split(method);
            }
            Op::SplitRegex(pattern) => {
                let index = self.text_constant(pattern);
                load_constant(&mut method.code, index);
                method.max_stack = method.max_stack.max(method.stack.len() as u16 + 2);
                self.split(method);
            }
        }
        Ok(())
    }

    fn nullable_parser(&mut self, kind: Kind) -> Result<u16, Error> {
        if let Some(&reference) = self.nullable_parsers.get(&kind) {
            return Ok(reference);
        }
        let owner =
            super::Types::boxed_class(kind).expect("numeric text parsers use boxed numbers");
        let name = match kind {
            Kind::Int => "$toIntOrNull",
            Kind::Long => "$toLongOrNull",
            Kind::Double => "$toDoubleOrNull",
            _ => unreachable!("only numeric text parsers are generated"),
        };
        let descriptor = format!("(Ljava/lang/String;)L{owner};");
        let class = self.runtime_class(owner);
        let parse = self.pool.member_for(10, class, "valueOf", &descriptor);
        let exception = self.runtime_class("java/lang/NumberFormatException");
        let mut helper = Method::new(vec![Local::Object(self.refs.string)]);
        helper.access(0x19, 0);
        instruction(&mut helper.code, 0xb8, parse);
        let protected_end = helper.code.len();
        helper.code.push(0xb0); // areturn
        let handler = helper.code.len();
        helper.stack.push(Local::Object(exception));
        helper.frame(1);
        helper.stack.clear();
        helper.code.extend([0x57, 0x01, 0xb0]); // pop exception, return null
        helper
            .exceptions
            .push([0, protected_end as u16, handler as u16, exception]);
        helper.max_stack = 1;
        let name_index = self.pool.ascii(name);
        let descriptor_index = self.pool.ascii(&descriptor);
        let mut encoded = Vec::new();
        helper.write(
            &mut encoded,
            0x100a,
            name_index,
            descriptor_index,
            self.code_name,
            self.stack_map_name,
        )?;
        self.synthetic_methods.extend(encoded);
        self.synthetic_count += 1;
        let tag = self.own_method_tag();
        let reference = self
            .pool
            .member_for(tag, self.this_class, name, &descriptor);
        self.nullable_parsers.insert(kind, reference);
        Ok(reference)
    }

    fn split(&mut self, method: &mut Method<'a>) {
        let split = self.runtime_member(
            STRING,
            "split",
            "(Ljava/lang/String;)[Ljava/lang/String;",
            10,
        );
        instruction(&mut method.code, 0xb6, split);
    }

    pub(super) fn text_constant(&mut self, text: &'static str) -> u16 {
        if let Some(&index) = self.strings.get(text) {
            return index;
        }
        let utf8 = self.pool.utf8(&super::modified_utf8(text));
        let index = self.pool.reference(8, &[utf8]);
        self.strings.insert(text, index);
        index
    }

    /// Calls a built-in function. Returns `None` when `name` is not built in.
    pub(super) fn builtin_function(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Option<Kind>>, Error> {
        use Kind::{Double, Int, Long, Text};
        let simple = |parameters: &'static [Kind], result: Option<Kind>, op: Op| {
            Some(Builtin {
                parameters,
                result,
                op,
            })
        };
        let spec = match (name.text, args.len()) {
            ("sqrt", 1) => simple(&[Double], Some(Double), Op::Static(MATH, "sqrt", "(D)D")),
            ("pow", 2) => simple(
                &[Double, Double],
                Some(Double),
                Op::Static(MATH, "pow", "(DD)D"),
            ),
            ("random", 0) => simple(&[], Some(Double), Op::Static(MATH, "random", "()D")),
            ("exit", 1) => simple(&[Int], None, Op::Static("java/lang/System", "exit", "(I)V")),
            ("sleep", 1) => simple(
                &[Long],
                None,
                Op::Static("java/lang/Thread", "sleep", "(J)V"),
            ),
            ("currentTimeMillis", 0) => simple(
                &[],
                Some(Long),
                Op::Static("java/lang/System", "currentTimeMillis", "()J"),
            ),
            ("abs" | "min" | "max", _) => return self.math_overload(method, name, args, base),
            ("randomInt", 2) => {
                let current = self.runtime_member(
                    "java/util/concurrent/ThreadLocalRandom",
                    "current",
                    "()Ljava/util/concurrent/ThreadLocalRandom;",
                    10,
                );
                instruction(&mut method.code, 0xb8, current);
                let random = self.runtime_class("java/util/concurrent/ThreadLocalRandom");
                method.stack.push(Local::Object(random));
                self.builtin_arguments(method, name, args, &[Int, Int], base + 1)?;
                method.stack.pop();
                let next = self.runtime_member(
                    "java/util/concurrent/ThreadLocalRandom",
                    "nextInt",
                    "(II)I",
                    10,
                );
                instruction(&mut method.code, 0xb6, next);
                method.max_stack = method.max_stack.max(base + 3);
                return Ok(Some(Some(Int)));
            }
            ("input", 0 | 1) => {
                if let [prompt] = args {
                    self.print_to_out(method, prompt, base, "print")?;
                    instruction(&mut method.code, 0xb2, self.refs.out);
                    let flush = self.runtime_member("java/io/PrintStream", "flush", "()V", 10);
                    instruction(&mut method.code, 0xb6, flush);
                }
                self.read_line(method, base);
                return Ok(Some(Some(Text)));
            }
            _ => None,
        };
        let Some(spec) = spec else {
            return Ok(None);
        };
        self.builtin_arguments(method, name, args, spec.parameters, base)?;
        self.apply(method, spec.op)?;
        let slots = spec
            .parameters
            .iter()
            .map(|kind| kind.slots())
            .sum::<u16>()
            .max(spec.result.map_or(0, Kind::slots));
        method.max_stack = method.max_stack.max(base + slots);
        Ok(Some(spec.result))
    }

    fn math_overload(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Option<Kind>>, Error> {
        let expected = if name.text == "abs" { 1 } else { 2 };
        if args.len() != expected {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let mut kind = self.expression(method, &args[0], base)?;
        if !kind.is_numeric() {
            return Err(Error {
                offset: args[0].offset(),
                message: "Expected an Int, Long, or Double here.",
            });
        }
        if let [_, right] = args {
            method.push_stack(self.local_type(kind));
            let rhs = self.expression(method, right, base + kind.slots())?;
            method.pop_stack();
            if !rhs.is_numeric() {
                return Err(Error {
                    offset: right.offset(),
                    message: "Expected an Int, Long, or Double here.",
                });
            }
            kind = self.widen_pair(method, kind, rhs, base);
        }
        let member: &'static str = match name.text {
            "abs" => "abs",
            "min" => "min",
            _ => "max",
        };
        let descriptor = match (kind, expected) {
            (Kind::Int, 1) => "(I)I",
            (Kind::Long, 1) => "(J)J",
            (_, 1) => "(D)D",
            (Kind::Int, _) => "(II)I",
            (Kind::Long, _) => "(JJ)J",
            _ => "(DD)D",
        };
        let reference = self.runtime_member(MATH, member, descriptor, 10);
        instruction(&mut method.code, 0xb8, reference);
        method.max_stack = method.max_stack.max(base + kind.slots() * expected as u16);
        Ok(Some(Some(kind)))
    }

    /// Reads one line from standard input without buffering ahead, so later reads
    /// still see the remaining input. Returns empty text at the end of input.
    fn read_line(&mut self, method: &mut Method<'a>, base: u16) {
        method.max_stack = method.max_stack.max(base + 3);
        let data = self.runtime_class("java/io/DataInputStream");
        instruction(&mut method.code, 0xbb, data);
        method.code.push(0x59); // dup
        let stdin = self.runtime_member("java/lang/System", "in", "Ljava/io/InputStream;", 9);
        instruction(&mut method.code, 0xb2, stdin);
        let init = self.runtime_member(
            "java/io/DataInputStream",
            "<init>",
            "(Ljava/io/InputStream;)V",
            10,
        );
        instruction(&mut method.code, 0xb7, init);
        let read = self.runtime_member(
            "java/io/DataInputStream",
            "readLine",
            "()Ljava/lang/String;",
            10,
        );
        instruction(&mut method.code, 0xb6, read);
        let empty = self.text_constant("");
        load_constant(&mut method.code, empty);
        let or_empty = self.runtime_member(
            "java/util/Objects",
            "toString",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;",
            10,
        );
        instruction(&mut method.code, 0xb8, or_empty);
        // readLine maps each byte to one character; decode those bytes properly.
        let latin1 = self.runtime_member(
            "java/nio/charset/StandardCharsets",
            "ISO_8859_1",
            "Ljava/nio/charset/Charset;",
            9,
        );
        instruction(&mut method.code, 0xb2, latin1);
        let bytes = self.runtime_member(STRING, "getBytes", "(Ljava/nio/charset/Charset;)[B", 10);
        instruction(&mut method.code, 0xb6, bytes);
        let string = self.refs.string;
        instruction(&mut method.code, 0xbb, string);
        method.code.extend([0x5a, 0x5f]); // dup_x1, swap
        let decode = self.runtime_member(STRING, "<init>", "([B)V", 10);
        instruction(&mut method.code, 0xb7, decode);
    }

    /// Evaluates `value` and prints it to standard output with `member`
    /// (`print` or `println`).
    pub(super) fn print_to_out(
        &mut self,
        method: &mut Method<'a>,
        value: &'a Expr<'a>,
        base: u16,
        member: &'static str,
    ) -> Result<(), Error> {
        instruction(&mut method.code, 0xb2, self.refs.out);
        method.stack.push(Local::Object(self.refs.stream));
        let kind = self.expression(method, value, base + 1)?;
        method.stack.pop();
        self.print_value(method, kind, base + 1, member);
        Ok(())
    }

    /// With [PrintStream, value] on the stack, prints the value.
    pub(super) fn print_value(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        _base: u16,
        member: &'static str,
    ) {
        let descriptor = match kind {
            Kind::Text | Kind::Nullable(super::Reference::Text) => "(Ljava/lang/String;)V",
            Kind::Int => "(I)V",
            Kind::Bool => "(Z)V",
            Kind::Double => "(D)V",
            Kind::Long => "(J)V",
            Kind::Char => "(C)V",
            Kind::List(list) => {
                self.array_to_string(method, list);
                "(Ljava/lang/String;)V"
            }
            _ => "(Ljava/lang/Object;)V",
        };
        let reference = self.runtime_member("java/io/PrintStream", member, descriptor, 10);
        instruction(&mut method.code, 0xb6, reference);
    }

    /// With [StringBuilder, value] on the stack, appends the value.
    pub(super) fn append_value(&mut self, method: &mut Method<'a>, kind: Kind, _base: u16) {
        let descriptor = match kind {
            Kind::Text | Kind::Nullable(super::Reference::Text) => {
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
            }
            Kind::Int => "(I)Ljava/lang/StringBuilder;",
            Kind::Bool => "(Z)Ljava/lang/StringBuilder;",
            Kind::Double => "(D)Ljava/lang/StringBuilder;",
            Kind::Long => "(J)Ljava/lang/StringBuilder;",
            Kind::Char => "(C)Ljava/lang/StringBuilder;",
            Kind::List(list) => {
                self.array_to_string(method, list);
                "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
            }
            _ => "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        };
        let reference = self.runtime_member("java/lang/StringBuilder", "append", descriptor, 10);
        instruction(&mut method.code, 0xb6, reference);
    }

    fn array_to_string(&mut self, method: &mut Method<'a>, list: ListKind) {
        let descriptor = match list {
            ListKind::Int => "([I)Ljava/lang/String;",
            ListKind::Long => "([J)Ljava/lang/String;",
            ListKind::Double => "([D)Ljava/lang/String;",
            ListKind::Bool => "([Z)Ljava/lang/String;",
            ListKind::Char => "([C)Ljava/lang/String;",
            _ => "([Ljava/lang/Object;)Ljava/lang/String;",
        };
        // Lists of lists print every level, as in [[1, 2], [3, 4]].
        let member = if matches!(list.element(), Kind::List(_)) {
            "deepToString"
        } else {
            "toString"
        };
        let reference = self.runtime_member("java/util/Arrays", member, descriptor, 10);
        instruction(&mut method.code, 0xb8, reference);
    }

    /// With two references on the stack, emits a comparison and returns the
    /// (branch-if-true, branch-if-false) opcodes for its integer result.
    pub(super) fn compare_references(
        &mut self,
        _method_unused: &mut Method<'a>,
        left: &Expr<'_>,
        lhs: Kind,
        rhs: Kind,
        comparison: Comparison,
    ) -> Result<(u8, u8), Error> {
        let method = _method_unused;
        match comparison {
            Comparison::Same => Ok((0xa5, 0xa6)),
            Comparison::NotSame => Ok((0xa6, 0xa5)),
            Comparison::Eq | Comparison::Ne => {
                let equals = self.runtime_member(
                    "java/util/Objects",
                    "equals",
                    "(Ljava/lang/Object;Ljava/lang/Object;)Z",
                    10,
                );
                instruction(&mut method.code, 0xb8, equals);
                Ok(if matches!(comparison, Comparison::Eq) {
                    (0x9a, 0x99)
                } else {
                    (0x99, 0x9a)
                })
            }
            _ if lhs == Kind::Text && rhs == Kind::Text => {
                let compare = self.runtime_member(STRING, "compareTo", "(Ljava/lang/String;)I", 10);
                instruction(&mut method.code, 0xb6, compare);
                Ok(match comparison {
                    Comparison::Lt => (0x9b, 0x9c),
                    Comparison::Le => (0x9e, 0x9d),
                    Comparison::Gt => (0x9d, 0x9e),
                    _ => (0x9c, 0x9b),
                })
            }
            _ => Err(Error {
                offset: left.offset(),
                message: "Only numbers, characters, and non-null text can be ordered with < and >.",
            }),
        }
    }
}
