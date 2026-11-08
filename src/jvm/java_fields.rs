//! Instance fields that a Mammoth class inherits from a Java superclass.
//!
//! Public fields are visible everywhere. Protected fields follow the JVM's
//! rules for a subclass in another package: only code in a Mammoth class that
//! extends the declaring Java class may use them, and only through a receiver
//! whose static type is that class or one of its subclasses. Package-private
//! and private Java fields are never visible, because Mammoth classes live in
//! their own packages.
use super::*;
use crate::classfile::{ACC_FINAL, ACC_PRIVATE, ACC_PROTECTED, ACC_PUBLIC, ACC_STATIC};
use interop::JType;

/// A Java field that the current code may use.
pub(super) struct JavaFieldAccess {
    jtype: JType,
    mutable: bool,
}

impl Types<'_> {
    /// The nearest non-static field named `name` in the Java class `binary`
    /// or its superclasses: its declaring class, access flags, and type.
    fn java_instance_field_member(&self, binary: &str, name: &str) -> Option<(String, u16, JType)> {
        self.java_instance_field_info(binary, name)
            .map(|(owner, access, jtype, _)| (owner, access, jtype))
    }

    /// Like `java_instance_field_member`, also returning the field's generic
    /// signature when it has one.
    fn java_instance_field_info(
        &self,
        binary: &str,
        name: &str,
    ) -> Option<(String, u16, JType, Option<String>)> {
        let mut current = Some(binary.to_owned());
        while let Some(class) = current {
            let info = self.java_class(&class)?;
            if let Some(field) = info
                .fields
                .iter()
                .find(|field| field.name == name && field.access & ACC_STATIC == 0)
            {
                return Some((
                    info.name.clone(),
                    field.access,
                    interop::parse_field(&field.descriptor)?,
                    field.signature.clone(),
                ));
            }
            current = info.super_name.clone();
        }
        None
    }
}

impl Symbols<'_> {
    /// The type of inherited Java field `name` seen through a receiver of
    /// Mammoth type `receiver`, with the type arguments of a generic Java
    /// superclass substituted, such as `T value` read as `Text` in a class
    /// extending `Holder<Text>`. `None` when the field is not generic.
    /// A read (`site` given) sees wildcard captures; a write sees the
    /// wildcard's bound, which the original signature then checks.
    fn java_field_generic(
        &self,
        receiver: Kind,
        name: &str,
        site: Option<(usize, usize)>,
    ) -> Option<Kind> {
        let class = self.types.native_class(receiver.non_null())?;
        let root = self.java_root(class)?;
        let (owner, _, _, signature) = self.types.java_instance_field_info(&root, name)?;
        let gtype = generics::parse_field_signature(&signature?)?;
        let view = self.java_super_view(receiver)?;
        let map = self.types.substitution(view, &owner);
        match site {
            Some(site) => self.types.result_kind(&gtype, &map, site),
            None => self.types.generic_kind(&gtype, &map),
        }
    }
}

impl<'a> Emitter<'a> {
    /// Resolves field `name` that Mammoth class `class` inherits from its Java
    /// superclass, checking visibility for a receiver of static class `class`.
    /// `Ok(None)` means there is no such Java field.
    fn inherited_java_field(
        &self,
        class: usize,
        name: Name<'_>,
    ) -> Result<Option<JavaFieldAccess>, Error> {
        let symbols = self.symbols;
        let Some(root) = symbols.java_root(class) else {
            return Ok(None);
        };
        let Some((owner, access, jtype)) =
            symbols.types.java_instance_field_member(&root, name.text)
        else {
            return Ok(None);
        };
        if access & ACC_PRIVATE != 0 || access & (ACC_PUBLIC | ACC_PROTECTED) == 0 {
            return Err(at(
                name,
                "This Java field is not visible here. Only public and protected Java fields can be used.",
            ));
        }
        if access & ACC_PROTECTED != 0 {
            let current = self.current_class.filter(|&current| {
                symbols
                    .java_root(current)
                    .is_some_and(|root| symbols.types.is_java_subclass(&root, &owner))
            });
            let Some(current) = current else {
                return Err(at(
                    name,
                    "This Java field is protected. Use it inside a class that extends its Java class.",
                ));
            };
            if !symbols.ancestors(class).any(|ancestor| ancestor == current) {
                return Err(at(
                    name,
                    "Access this protected Java field through this class or one of its subclasses.",
                ));
            }
        }
        if symbols.types.kind_of(&jtype).is_none() {
            return Err(at(
                name,
                "This Java field's type cannot be used from Mammoth.",
            ));
        }
        Ok(Some(JavaFieldAccess {
            mutable: access & ACC_FINAL == 0,
            jtype,
        }))
    }

    /// Whether `class` inherits a visible Java field named `name`.
    pub(super) fn inherited_java_field_exists(
        &self,
        class: usize,
        name: Name<'_>,
    ) -> Result<bool, Error> {
        Ok(self.inherited_java_field(class, name)?.is_some())
    }

    /// The constant for field `name` referenced through Mammoth class `class`.
    fn java_field_reference(&mut self, class: usize, name: &str, jtype: &JType) -> u16 {
        let owner = self.object_class(Kind::Object(class));
        self.pool.member_for(9, owner, name, &jtype.descriptor())
    }

    /// Reads an inherited Java field of the receiver on top of the stack,
    /// whose static class is Mammoth class `class`.
    pub(super) fn read_inherited_java_field(
        &mut self,
        method: &mut Method<'a>,
        receiver: Kind,
        class: usize,
        name: Name<'a>,
        base: u16,
        allow_null: bool,
    ) -> Result<Option<Kind>, Error> {
        let Some(field) = self.inherited_java_field(class, name)? else {
            return Ok(None);
        };
        let reference = self.java_field_reference(class, name.text, &field.jtype);
        instruction(&mut method.code, 0xb4, reference); // getfield
        let site = self.site(name);
        self.last_generic_result = self
            .symbols
            .java_field_generic(receiver, name.text, Some(site));
        let kind = self
            .convert_from_java(method, &field.jtype, base, allow_null)
            .ok_or_else(|| at(name, "This field has no value."))?;
        Ok(Some(kind))
    }

    /// `name` used without a receiver inside a Mammoth class that inherits a
    /// Java field with that name.
    pub(super) fn implicit_java_field(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let Some(class) = self.current_class else {
            return Ok(None);
        };
        if method
            .binding(Name {
                text: "this",
                offset: 0,
            })
            .is_none()
        {
            return Ok(None);
        }
        if self.inherited_java_field(class, name)?.is_none() {
            return Ok(None);
        }
        if !self.this_ready {
            return Err(at(
                name,
                "Fields cannot be used before the superclass constructor runs.",
            ));
        }
        method.access(0x19, 0);
        method.max_stack = method.max_stack.max(base + 1);
        let allow_null = std::mem::take(&mut self.allow_null);
        let receiver = self.symbols.types.self_kind(class);
        self.read_inherited_java_field(method, receiver, class, name, base, allow_null)
    }

    /// Stores `value` into an inherited Java field of the receiver on top of
    /// the stack (at `base`), whose static class is Mammoth class `class`.
    /// Returns `false` when the class inherits no Java field with that name.
    pub(super) fn write_inherited_java_field(
        &mut self,
        method: &mut Method<'a>,
        receiver: Kind,
        class: usize,
        name: Name<'a>,
        value: &'a Expr<'a>,
        base: u16,
    ) -> Result<bool, Error> {
        let Some(field) = self.inherited_java_field(class, name)? else {
            return Ok(false);
        };
        if !field.mutable {
            return Err(at(
                name,
                "This Java field is final, so it cannot be changed.",
            ));
        }
        let declared = self
            .symbols
            .java_field_generic(receiver, name.text, None)
            .unwrap_or_else(|| self.symbols.types.kind_of(&field.jtype).unwrap());
        let expected = declared.nullable().unwrap_or(declared);
        let expected = if declared.is_reference() {
            expected
        } else {
            declared
        };
        method.push_stack(self.local_type(Kind::Object(class)));
        let kind = self.expression_as(method, value, base + 1, Some(expected))?;
        self.coerce(method, value, kind, expected, base + 1)?;
        match (&field.jtype, expected) {
            (JType::Byte, Kind::Int) => method.code.push(0x91), // i2b
            (JType::Short, Kind::Int) => method.code.push(0x93), // i2s
            (JType::Float, Kind::Double) => method.code.push(0x90), // d2f
            _ => self.convert_to_java(method, expected, &field.jtype, base + 1),
        }
        method.pop_stack();
        let reference = self.java_field_reference(class, name.text, &field.jtype);
        instruction(&mut method.code, 0xb5, reference); // putfield
        Ok(true)
    }

    /// Assigns `name = value` without a receiver inside a Mammoth class that
    /// inherits a Java field with that name.
    pub(super) fn assign_implicit_java_field(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        value: &'a Expr<'a>,
    ) -> Result<bool, Error> {
        let Some(class) = self.current_class else {
            return Ok(false);
        };
        if method
            .binding(Name {
                text: "this",
                offset: 0,
            })
            .is_none()
            || self.inherited_java_field(class, name)?.is_none()
        {
            return Ok(false);
        }
        if !self.this_ready {
            return Err(at(
                name,
                "Fields cannot be used before the superclass constructor runs.",
            ));
        }
        method.access(0x19, 0);
        method.max_stack = method.max_stack.max(1);
        let receiver = self.symbols.types.self_kind(class);
        self.write_inherited_java_field(method, receiver, class, name, value, 0)
    }
}
