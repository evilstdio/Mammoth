//! `static fun`, `static var`, and `static let` members of Mammoth classes.
//!
//! Static members belong to the class rather than to an object. They compile
//! to JVM static methods and fields with their plain names, so Java code and
//! frameworks such as Mixin see `Counter.bump()` and `Counter.total`. Static
//! fields start in declaration order in the class's `<clinit>`. Code outside
//! the class names the class, as in `Counter.bump()`; code inside the class
//! and its subclasses can leave the class name out. Static methods have no
//! `this`.
use super::*;

impl<'a> Symbols<'a> {
    /// The class among `class` and its superclasses that declares static
    /// method `name`, and its signature.
    pub(super) fn find_static_method(
        &self,
        class: usize,
        name: &str,
    ) -> Option<(usize, &Signature<'a>)> {
        self.ancestors(class).find_map(|owner| {
            self.static_methods[owner]
                .get(name)
                .map(|found| (owner, found))
        })
    }

    /// The class among `class` and its superclasses that declares static
    /// field `name`, and the field.
    pub(super) fn find_static_field(
        &self,
        class: usize,
        name: &str,
    ) -> Option<(usize, StaticField<'a>)> {
        self.ancestors(class).find_map(|owner| {
            self.static_fields[owner]
                .iter()
                .find(|field| field.name == name)
                .map(|field| (owner, *field))
        })
    }
}

impl<'a> Emitter<'a> {
    /// The Mammoth class that `receiver` names, when it is a bare class name
    /// rather than a value, as in `Counter.bump()`.
    pub(super) fn static_class(&self, method: &Method<'a>, receiver: &Expr<'a>) -> Option<usize> {
        let Expr::Variable(name) = receiver else {
            return None;
        };
        if method.binding(*name).is_some()
            || self.fields.contains_key(name.text)
            || self.symbols.global(name.text).is_some()
        {
            return None;
        }
        match self.type_scope.names.get(name.text) {
            Some(&Kind::Object(class)) => Some(class),
            _ => None,
        }
    }

    fn check_static_access(
        &self,
        owner: usize,
        private: bool,
        name: Name<'_>,
    ) -> Result<(), Error> {
        if private && self.current_class != Some(owner) {
            return Err(at(name, "This static member is private to its class."));
        }
        Ok(())
    }

    /// Calls static method `name` of Mammoth class `class` (or a superclass).
    /// `Ok(None)` when there is no such static method.
    pub(super) fn static_call(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Option<Kind>>, Error> {
        let symbols = self.symbols;
        let Some((owner, signature)) = symbols.find_static_method(class, name.text) else {
            return Ok(None);
        };
        let private = symbols.class_declarations[owner]
            .static_methods
            .iter()
            .any(|function| function.name.text == name.text && function.private);
        self.check_static_access(owner, private, name)?;
        self.call_arguments(method, name, args, signature, base)?;
        let class_index = self.object_class(Kind::Object(owner));
        let reference = self
            .pool
            .member_for(10, class_index, name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb8, reference); // invokestatic
        method.max_stack = method
            .max_stack
            .max(base + signature.result.map_or(0, Kind::slots));
        Ok(Some(signature.result))
    }

    fn static_field_reference(&mut self, owner: usize, field: StaticField<'a>) -> u16 {
        let class = self.object_class(Kind::Object(owner));
        let descriptor = field.kind.descriptor(&self.symbols.types);
        self.pool.member_for(9, class, field.name, &descriptor)
    }

    /// Reads static field `name` of Mammoth class `class`, or `Ok(None)`.
    pub(super) fn read_static_field(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let Some((owner, field)) = self.symbols.find_static_field(class, name.text) else {
            return Ok(None);
        };
        self.check_static_access(owner, field.private, name)?;
        if self.pending_statics.contains(&(owner, field.name)) {
            return Err(at(
                name,
                "This static field is read before its starting value is set, so it would still be null or zero. Declare it above this line.",
            ));
        }
        let reference = self.static_field_reference(owner, field);
        instruction(&mut method.code, 0xb2, reference); // getstatic
        method.max_stack = method.max_stack.max(base + field.kind.slots().max(1));
        Ok(Some(field.kind))
    }

    /// Stores `value` into static field `name` of Mammoth class `class`.
    /// Returns `false` when there is no such static field.
    pub(super) fn write_static_field(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        value: &'a Expr<'a>,
    ) -> Result<bool, Error> {
        let Some((owner, field)) = self.symbols.find_static_field(class, name.text) else {
            return Ok(false);
        };
        self.check_static_access(owner, field.private, name)?;
        if !field.mutable {
            return Err(at(
                name,
                "This static field is read-only. Declare it with static var to change it.",
            ));
        }
        let base = method.stack.len() as u16;
        let kind = self.expression_as(method, value, base, Some(field.kind))?;
        self.coerce(method, value, kind, field.kind, base)?;
        let reference = self.static_field_reference(owner, field);
        instruction(&mut method.code, 0xb3, reference); // putstatic
        Ok(true)
    }

    /// Emits the static fields, static methods, and `<clinit>` of class
    /// `index`. Returns the number of fields and methods written.
    pub(super) fn static_members(
        &mut self,
        index: usize,
        class: &'a crate::syntax::Class<'a>,
        fields: &mut Vec<u8>,
        methods: &mut Vec<u8>,
    ) -> Result<(usize, usize), Error> {
        let symbols = self.symbols;
        for (field, declaration) in symbols.static_fields[index]
            .iter()
            .zip(&class.static_fields)
        {
            let name = self.pool.ascii(field.name);
            let descriptor = self.pool.ascii(&field.kind.descriptor(&symbols.types));
            let mut access = 0x0008 | if field.private { 0x0002 } else { 0x0001 };
            if !field.mutable {
                access |= 0x0010; // final
            }
            let start = fields.len();
            shorts(fields, &[access, name, descriptor, 0]);
            if symbols.types.needs_signature(field.kind) {
                let text = symbols.types.signature_type(field.kind, false);
                self.signature_attribute(fields, start, &text, class.name)?;
            }
            self.annotate_field(fields, start, &declaration.annotations)?;
        }
        let (code_name, stack_map_name) = (self.code_name, self.stack_map_name);
        let mut method_count = 0;
        for function in &class.static_methods {
            let signature = &symbols.static_methods[index][function.name.text];
            let mut method = Method::new(Vec::new());
            method.result = signature.result;
            self.mark_line(&mut method, function.name.offset);
            self.bind_parameters(&mut method, signature);
            if self.block(&mut method, &function.body)? {
                if signature.result.is_some() {
                    return Err(at(
                        function.name,
                        "This function must return a value on every path.",
                    ));
                }
                method.code.push(0xb1);
            }
            let name = self.pool.ascii(function.name.text);
            let descriptor = self.pool.ascii(&signature.descriptor);
            let access = 0x0008 | if function.private { 0x0002 } else { 0x0001 };
            let start = methods.len();
            method.write(methods, access, name, descriptor, code_name, stack_map_name)?;
            self.method_signature(methods, start, signature, function.name)?;
            self.annotate_method(methods, start, &function.annotations)?;
            method_count += 1;
        }
        if !class.static_initializers.is_empty() {
            let mut initializer = Method::new(Vec::new());
            self.pending_statics = class
                .static_fields
                .iter()
                .filter(|field| field.value.is_some())
                .map(|field| (index, field.name.text))
                .collect();
            for step in &class.static_initializers {
                match step {
                    crate::syntax::Initializer::Field(position) => {
                        let field = symbols.static_fields[index][*position];
                        let Some(value) = &class.static_fields[*position].value else {
                            continue;
                        };
                        self.mark_line(&mut initializer, value.offset());
                        let kind =
                            self.expression_as(&mut initializer, value, 0, Some(field.kind))?;
                        self.coerce(&mut initializer, value, kind, field.kind, 0)?;
                        let reference = self.static_field_reference(index, field);
                        instruction(&mut initializer.code, 0xb3, reference); // putstatic
                        self.pending_statics
                            .retain(|&(_, pending)| pending != field.name);
                    }
                    crate::syntax::Initializer::Block(statements) => {
                        if !self.scoped_block(&mut initializer, statements, &[])? {
                            return Err(Error {
                                offset: statements.last().map_or(0, Statement::offset),
                                message: "A static init block cannot return.",
                            });
                        }
                    }
                }
            }
            self.pending_statics.clear();
            initializer.code.push(0xb1);
            let name = self.pool.ascii("<clinit>");
            let descriptor = self.pool.ascii("()V");
            initializer.write(methods, 0x0008, name, descriptor, code_name, stack_map_name)?;
            method_count += 1;
        }
        Ok((class.static_fields.len(), method_count))
    }
}
