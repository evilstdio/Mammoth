//! Members generated for `enum` and `data class` declarations.

use super::{
    Emitter, Error, FieldInfo, Kind, ListKind, Local, Method, Name, at, instruction, push_int,
    shorts,
};
use crate::syntax::Class;

impl<'a> Emitter<'a> {
    /// Adds an enum's value fields, its static initializer, `values()`, and
    /// `valueOf(name)`. Returns the number of fields and methods added.
    pub(super) fn enum_members(
        &mut self,
        index: usize,
        class: &'a Class<'a>,
        fields: &mut Vec<u8>,
        methods: &mut Vec<u8>,
    ) -> Result<(usize, usize), Error> {
        let values = class.enum_values.as_ref().expect("only enums have values");
        let binary = self.symbols.types.binary_name(Kind::Object(index));
        let descriptor = format!("L{binary};");
        let array_descriptor = format!("[L{binary};");
        let this_class = self.this_class;
        // Fields: one per value, plus the array returned by values().
        for (name, _) in values {
            let name_index = self.pool.ascii(name.text);
            let type_index = self.pool.ascii(&descriptor);
            shorts(fields, &[0x4019, name_index, type_index, 0]); // public static final enum
        }
        let values_name = self.pool.ascii("$VALUES");
        let values_type = self.pool.ascii(&array_descriptor);
        shorts(fields, &[0x101a, values_name, values_type, 0]); // private static final synthetic
        let all = self
            .pool
            .member_for(9, this_class, "$VALUES", &array_descriptor);

        // static initializer
        let signature = &self.symbols.constructors[index];
        let constructor_descriptor = format!("(Ljava/lang/String;I{}", &signature.descriptor[1..]);
        let init = self
            .pool
            .member_for(10, this_class, "<init>", &constructor_descriptor);
        let mut clinit = Method::new(Vec::new());
        for (ordinal, (name, args)) in values.iter().enumerate() {
            let uninitialized = Local::Uninitialized(clinit.code.len() as u16);
            instruction(&mut clinit.code, 0xbb, this_class); // new
            clinit.code.push(0x59); // dup
            let text = self.pool.ascii(name.text);
            let constant = self.pool.reference(8, &[text]);
            super::load_constant(&mut clinit.code, constant);
            self.push_int32(&mut clinit, ordinal as i32);
            let string = self.refs.string;
            clinit.stack.extend([
                uninitialized,
                uninitialized,
                Local::Object(string),
                Local::Int,
            ]);
            clinit.max_stack = clinit.max_stack.max(4);
            self.call_arguments(&mut clinit, *name, args, signature, 4)?;
            clinit.stack.clear();
            instruction(&mut clinit.code, 0xb7, init); // invokespecial
            let field = self.pool.member_for(9, this_class, name.text, &descriptor);
            instruction(&mut clinit.code, 0xb3, field); // putstatic
        }
        self.push_int32(&mut clinit, values.len() as i32);
        instruction(&mut clinit.code, 0xbd, this_class); // anewarray
        for (ordinal, (name, _)) in values.iter().enumerate() {
            clinit.code.push(0x59); // dup
            self.push_int32(&mut clinit, ordinal as i32);
            let field = self.pool.member_for(9, this_class, name.text, &descriptor);
            instruction(&mut clinit.code, 0xb2, field); // getstatic
            clinit.code.push(0x53); // aastore
        }
        instruction(&mut clinit.code, 0xb3, all); // putstatic
        clinit.code.push(0xb1);
        clinit.max_stack = clinit.max_stack.max(4);
        let name = self.pool.ascii("<clinit>");
        let kind = self.pool.ascii("()V");
        clinit.write(
            methods,
            0x0008,
            name,
            kind,
            self.code_name,
            self.stack_map_name,
        )?;

        // values()
        let mut all_values = Method::new(Vec::new());
        instruction(&mut all_values.code, 0xb2, all); // getstatic
        let array_class = self.pool.class(&array_descriptor);
        let clone = self
            .pool
            .member_for(10, array_class, "clone", "()Ljava/lang/Object;");
        instruction(&mut all_values.code, 0xb6, clone);
        instruction(&mut all_values.code, 0xc0, array_class); // checkcast
        all_values.code.push(0xb0);
        all_values.max_stack = 1;
        let name = self.pool.ascii("values");
        let kind = self.pool.ascii(&format!("(){array_descriptor}"));
        all_values.write(
            methods,
            0x0009,
            name,
            kind,
            self.code_name,
            self.stack_map_name,
        )?;

        // valueOf(name)
        let string = self.refs.string;
        let mut value_of = Method::new(vec![Local::Object(string)]);
        super::load_constant(&mut value_of.code, this_class); // ldc Class
        value_of.access(0x19, 0);
        let lookup = self.runtime_member(
            "java/lang/Enum",
            "valueOf",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;",
            10,
        );
        instruction(&mut value_of.code, 0xb8, lookup);
        instruction(&mut value_of.code, 0xc0, this_class); // checkcast
        value_of.code.push(0xb0);
        value_of.max_stack = 2;
        let name = self.pool.ascii("valueOf");
        let kind = self
            .pool
            .ascii(&format!("(Ljava/lang/String;){descriptor}"));
        value_of.write(
            methods,
            0x0009,
            name,
            kind,
            self.code_name,
            self.stack_map_name,
        )?;
        Ok((values.len() + 1, 3))
    }

    /// Generates `toString`, `equals`, and `hashCode` for a data class from
    /// the fields its constructor declares, unless the class defines them.
    /// Returns the number of methods added.
    pub(super) fn data_members(
        &mut self,
        index: usize,
        class: &'a Class<'a>,
        methods: &mut Vec<u8>,
    ) -> Result<usize, Error> {
        let symbols = self.symbols;
        let declared = &symbols.methods[index];
        let fields: Vec<&'a FieldInfo<'a>> = class
            .parameters
            .iter()
            .filter_map(|(parameter, _)| {
                symbols.class_fields[index]
                    .iter()
                    .find(|field| field.name == parameter.text)
            })
            .collect();
        let this_class = self.this_class;
        let mut added = 0;
        if !declared.contains_key("toString") {
            let mut method = Method::new(vec![Local::Object(this_class)]);
            let builder = self.refs.builder;
            instruction(&mut method.code, 0xbb, builder); // new
            method.code.push(0x59); // dup
            let opening = self.text_constant_owned(&format!("{}(", class.name.text));
            super::load_constant(&mut method.code, opening);
            let init = self.runtime_member(
                "java/lang/StringBuilder",
                "<init>",
                "(Ljava/lang/String;)V",
                10,
            );
            instruction(&mut method.code, 0xb7, init);
            for (position, field) in fields.iter().enumerate() {
                let label = if position == 0 {
                    format!("{}=", field.name)
                } else {
                    format!(", {}=", field.name)
                };
                let label = self.text_constant_owned(&label);
                super::load_constant(&mut method.code, label);
                self.append_value(&mut method, Kind::Text, 1);
                method.access(0x19, 0);
                let reference = self.own_field(field);
                instruction(&mut method.code, 0xb4, reference); // getfield
                self.append_value(&mut method, field.kind, 1);
            }
            let closing = self.text_constant_owned(")");
            super::load_constant(&mut method.code, closing);
            self.append_value(&mut method, Kind::Text, 1);
            instruction(&mut method.code, 0xb6, self.refs.to_string);
            method.code.push(0xb0);
            method.max_stack = 4;
            let name = self.pool.ascii("toString");
            let kind = self.pool.ascii("()Ljava/lang/String;");
            method.write(
                methods,
                0x0001,
                name,
                kind,
                self.code_name,
                self.stack_map_name,
            )?;
            added += 1;
        }
        if !declared.contains_key("equals") {
            let object = self.runtime_class("java/lang/Object");
            let mut method = Method::new(vec![Local::Object(this_class), Local::Object(object)]);
            method.code.extend([0x2a, 0x2b]); // aload_0, aload_1
            let different = method.code.len();
            instruction(&mut method.code, 0xa6, 0); // if_acmpne
            method.code.extend([0x04, 0xac]); // iconst_1, ireturn
            method.patch_branch(different, method.code.len(), 0)?;
            method.frame(2);
            method.code.push(0x2b); // aload_1
            instruction(&mut method.code, 0xc1, this_class); // instanceof
            let same_class = method.code.len();
            instruction(&mut method.code, 0x9a, 0); // ifne
            method.code.extend([0x03, 0xac]); // iconst_0, ireturn
            method.patch_branch(same_class, method.code.len(), 0)?;
            method.frame(2);
            method.code.push(0x2b); // aload_1
            instruction(&mut method.code, 0xc0, this_class); // checkcast
            let other = method.local(Local::Object(this_class));
            method.access(0x3a, other);
            let mut unequal = Vec::new();
            for field in &fields {
                let reference = self.own_field(field);
                method.access(0x19, 0);
                instruction(&mut method.code, 0xb4, reference);
                method.access(0x19, other);
                instruction(&mut method.code, 0xb4, reference);
                let opcode = match field.kind {
                    Kind::Int | Kind::Bool | Kind::Char => 0xa0, // if_icmpne
                    Kind::Long => {
                        method.code.push(0x94); // lcmp
                        0x9a // ifne
                    }
                    Kind::Double => {
                        let compare =
                            self.runtime_member("java/lang/Double", "compare", "(DD)I", 10);
                        instruction(&mut method.code, 0xb8, compare);
                        0x9a
                    }
                    _ => {
                        let equals = self.runtime_member(
                            "java/util/Objects",
                            "equals",
                            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
                            10,
                        );
                        instruction(&mut method.code, 0xb8, equals);
                        0x99 // ifeq
                    }
                };
                unequal.push(method.code.len());
                instruction(&mut method.code, opcode, 0);
            }
            method.code.extend([0x04, 0xac]); // iconst_1, ireturn
            method.patch_branches(&unequal, 0)?;
            method.frame(3);
            method.code.extend([0x03, 0xac]); // iconst_0, ireturn
            method.max_stack = 4;
            let name = self.pool.ascii("equals");
            let kind = self.pool.ascii("(Ljava/lang/Object;)Z");
            method.write(
                methods,
                0x0001,
                name,
                kind,
                self.code_name,
                self.stack_map_name,
            )?;
            added += 1;
        }
        if !declared.contains_key("hashCode") {
            let mut method = Method::new(vec![Local::Object(this_class)]);
            push_int(&mut method.code, 0);
            for field in &fields {
                push_int(&mut method.code, 31);
                method.code.push(0x68); // imul
                method.access(0x19, 0);
                let reference = self.own_field(field);
                instruction(&mut method.code, 0xb4, reference);
                let hash = match field.kind {
                    Kind::Int | Kind::Char => None,
                    Kind::Bool => Some(("java/lang/Boolean", "(Z)I")),
                    Kind::Long => Some(("java/lang/Long", "(J)I")),
                    Kind::Double => Some(("java/lang/Double", "(D)I")),
                    _ => Some(("java/util/Objects", "(Ljava/lang/Object;)I")),
                };
                if let Some((owner, descriptor)) = hash {
                    let reference = self.runtime_member(owner, "hashCode", descriptor, 10);
                    instruction(&mut method.code, 0xb8, reference);
                }
                method.code.push(0x60); // iadd
            }
            method.code.push(0xac);
            method.max_stack = 4;
            let name = self.pool.ascii("hashCode");
            let kind = self.pool.ascii("()I");
            method.write(
                methods,
                0x0001,
                name,
                kind,
                self.code_name,
                self.stack_map_name,
            )?;
            added += 1;
        }
        Ok(added)
    }

    fn own_field(&mut self, field: &'a FieldInfo<'a>) -> u16 {
        let descriptor = field.kind.descriptor(&self.symbols.types);
        self.pool
            .member_for(9, self.this_class, field.name, &descriptor)
    }

    fn text_constant_owned(&mut self, text: &str) -> u16 {
        let utf8 = self.pool.utf8(&super::modified_utf8(text));
        self.pool.reference(8, &[utf8])
    }

    /// `Type.Value` for a value of a Mammoth enum.
    pub(super) fn enum_value(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        if !self.symbols.enum_values[class]
            .as_ref()
            .is_some_and(|values| values.contains(&name.text))
        {
            return Err(at(name, "This enum has no value with that name."));
        }
        let kind = Kind::Object(class);
        let owner = self.object_class(kind);
        let descriptor = kind.descriptor(&self.symbols.types);
        let reference = self.pool.member_for(9, owner, name.text, &descriptor);
        instruction(&mut method.code, 0xb2, reference); // getstatic
        method.max_stack = method.max_stack.max(base + 1);
        Ok(kind)
    }

    /// `Type.values()` or `Type.valueOf(name)` on a Mammoth enum.
    pub(super) fn enum_static_call(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        args: &'a [crate::syntax::Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let kind = Kind::Object(class);
        let owner = self.object_class(kind);
        let descriptor = kind.descriptor(&self.symbols.types);
        match (name.text, args) {
            ("values", []) => {
                let reference =
                    self.pool
                        .member_for(10, owner, "values", &format!("()[{descriptor}"));
                instruction(&mut method.code, 0xb8, reference);
                method.max_stack = method.max_stack.max(base + 1);
                Ok(Some(Kind::List(ListKind::Object(class))))
            }
            ("valueOf", [text]) => {
                let actual = self.expression(method, text, base)?;
                self.symbols.require(text, actual, Kind::Text)?;
                let reference = self.pool.member_for(
                    10,
                    owner,
                    "valueOf",
                    &format!("(Ljava/lang/String;){descriptor}"),
                );
                instruction(&mut method.code, 0xb8, reference);
                Ok(Some(kind))
            }
            _ => Err(at(
                name,
                "An enum type has values() and valueOf(name). Call other methods on a value.",
            )),
        }
    }

    /// The enum class named by `receiver`, such as `Color` in `Color.Red`.
    pub(super) fn enum_owner(
        &self,
        method: &Method<'a>,
        receiver: &crate::syntax::Expr<'a>,
    ) -> Option<usize> {
        let crate::syntax::Expr::Variable(name) = receiver else {
            return None;
        };
        if method.binding(*name).is_some()
            || self.fields.contains_key(name.text)
            || self.symbols.global(name.text).is_some()
        {
            return None;
        }
        match self.symbols.types.names.get(name.text) {
            Some(&Kind::Object(class)) if self.symbols.enum_values[class].is_some() => Some(class),
            _ => None,
        }
    }
}
