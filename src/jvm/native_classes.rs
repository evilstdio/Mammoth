//! Applied Mammoth class types share one erased JVM class. Their arguments
//! remain part of the checked type and are substituted at member accesses.
use super::defaults::DefaultHost;
use super::*;

#[derive(Clone, PartialEq)]
pub(super) struct Application {
    /// A class index, or an interface index when `interface` is set.
    pub class: usize,
    pub arguments: Vec<Kind>,
    pub interface: bool,
}

impl<'a> Types<'a> {
    pub(super) fn contains_application(&self, kind: Kind) -> bool {
        match kind {
            Kind::Applied(_) | Kind::AppliedInterface(_) => true,
            Kind::Nullable(_) => self.contains_application(kind.non_null()),
            Kind::List(list) | Kind::MutableList(list) => self.contains_application(list.element()),
            Kind::Java(_) => self
                .type_arguments(kind)
                .into_iter()
                .any(|kind| self.contains_application(kind)),
            Kind::Function(index) => {
                let function = self.function_type(index);
                function
                    .parameters
                    .into_iter()
                    .chain(function.result)
                    .any(|kind| self.contains_application(kind))
            }
            _ => false,
        }
    }
    fn intern_application(&self, application: Application) -> usize {
        let mut table = self.applications.borrow_mut();
        table
            .iter()
            .position(|entry| *entry == application)
            .unwrap_or_else(|| {
                table.push(application);
                table.len() - 1
            })
    }

    pub(super) fn apply_class(&self, class: usize, arguments: Vec<Kind>) -> Kind {
        Kind::Applied(self.intern_application(Application {
            class,
            arguments,
            interface: false,
        }))
    }

    pub(super) fn apply_interface(&self, interface: usize, arguments: Vec<Kind>) -> Kind {
        Kind::AppliedInterface(self.intern_application(Application {
            class: interface,
            arguments,
            interface: true,
        }))
    }

    /// The Mammoth interface behind an interface type, with or without arguments.
    pub(super) fn interface_root(&self, kind: Kind) -> Option<usize> {
        match kind.non_null() {
            Kind::Interface(index) => Some(index),
            Kind::AppliedInterface(index) => Some(self.applications.borrow()[index].class),
            _ => None,
        }
    }

    pub(super) fn interface_scope(&self, interface: usize) -> Cow<'_, Types<'a>> {
        let parameters = self.interface_parameters.borrow();
        if parameters[interface].is_empty() {
            return Cow::Borrowed(self);
        }
        let mut scope = self.clone();
        for &(name, kind) in &parameters[interface] {
            scope.names.insert(name.text, kind);
        }
        Cow::Owned(scope)
    }

    pub(super) fn resolve_interface_application(
        &self,
        interface: usize,
        name: Name<'_>,
    ) -> Result<Kind, Error> {
        let (_, args) = native_generics::call_name(name);
        let args = args.unwrap_or_default();
        if args.len() != self.interface_parameters.borrow()[interface].len() || args.is_empty() {
            return Err(at(
                name,
                "Give one type argument for each interface type parameter.",
            ));
        }
        let arguments = args
            .into_iter()
            .enumerate()
            .map(|(position, annotation)| {
                if annotation.text == "*" {
                    return Ok(self.star(true, interface, position));
                }
                let kind = resolve_type(self, annotation)?;
                if matches!(kind, Kind::Nullable(_)) {
                    return Err(at(
                        annotation,
                        "Interface type arguments must be non-null. Use T? in a member type for a missing value.",
                    ));
                }
                Ok(kind)
            })
            .collect::<Result<_, _>>()?;
        let kind = self.apply_interface(interface, arguments);
        self.defer_bound_check(kind, name.offset);
        Ok(kind)
    }

    pub(super) fn application(&self, index: usize) -> Application {
        self.applications.borrow()[index].clone()
    }

    pub(super) fn native_class(&self, kind: Kind) -> Option<usize> {
        match kind {
            Kind::Object(class) => Some(class),
            Kind::Applied(index) => Some(self.applications.borrow()[index].class),
            _ => None,
        }
    }

    pub(super) fn class_scope(&self, class: usize) -> Cow<'_, Types<'a>> {
        let parameters = self.class_parameters.borrow();
        if parameters[class].is_empty() {
            return Cow::Borrowed(self);
        }
        let mut scope = self.clone();
        for &(name, kind) in &parameters[class] {
            scope.names.insert(name.text, kind);
        }
        Cow::Owned(scope)
    }

    pub(super) fn self_kind(&self, class: usize) -> Kind {
        let args: Vec<_> = self.class_parameters.borrow()[class]
            .iter()
            .map(|(_, kind)| *kind)
            .collect();
        if args.is_empty() {
            Kind::Object(class)
        } else {
            self.apply_class(class, args)
        }
    }

    pub(super) fn class_map(&self, kind: Kind) -> HashMap<Kind, Kind> {
        let (Kind::Applied(index) | Kind::AppliedInterface(index)) = kind.non_null() else {
            return HashMap::new();
        };
        let application = self.application(index);
        let parameters = if application.interface {
            self.interface_parameters.borrow()
        } else {
            self.class_parameters.borrow()
        };
        parameters[application.class]
            .iter()
            .map(|(_, kind)| *kind)
            .zip(application.arguments)
            .collect()
    }

    pub(super) fn resolve_application(&self, class: usize, name: Name<'_>) -> Result<Kind, Error> {
        let (_, args) = native_generics::call_name(name);
        let args = args.unwrap_or_default();
        if args.len() != self.class_parameters.borrow()[class].len() || args.is_empty() {
            return Err(at(
                name,
                "Give one type argument for each class type parameter.",
            ));
        }
        let arguments = args.into_iter().enumerate().map(|(position, annotation)| {
            if annotation.text == "*" {
                return Ok(self.star(false, class, position));
            }
            let kind = resolve_type(self, annotation)?;
            if matches!(kind, Kind::Nullable(_)) {
                return Err(at(annotation, "Class type arguments must be non-null. Use T? for a nullable field or parameter."));
            }
            Ok(kind)
        }).collect::<Result<_, _>>()?;
        let kind = self.apply_class(class, arguments);
        self.defer_bound_check(kind, name.offset);
        Ok(kind)
    }
}

impl Symbols<'_> {
    pub(super) fn class_signature(&self, class: usize) -> String {
        let mut text = String::new();
        let parameters = self.types.class_parameters.borrow();
        if !parameters[class].is_empty() {
            text.push('<');
            for &(name, kind) in &parameters[class] {
                text.push_str(&self.types.formal_parameter(name.text, kind));
            }
            text.push('>');
        }
        drop(parameters);
        let parent = self.superclass_kinds[class]
            .or(self.java_supers[class].map(Kind::Java))
            .unwrap_or(Kind::Any);
        text.push_str(&self.types.signature_type(parent, false));
        for &interface in &self.implementations[class] {
            text.push_str(&self.types.signature_type(interface, false));
        }
        text
    }
    pub(super) fn constructor_result(
        &self,
        name: Name<'_>,
        args: &[Expr<'_>],
        scope: &dyn Fn(&str) -> Option<Kind>,
    ) -> Option<Kind> {
        let (base, explicit) = native_generics::call_name(name);
        let &Kind::Object(class) = self.types.names.get(base.text)? else {
            return None;
        };
        let parameters = self.types.class_parameters.borrow()[class].clone();
        if parameters.is_empty() {
            return Some(Kind::Object(class));
        }
        if explicit.is_some() {
            return self.types.resolve_application(class, name).ok();
        }
        let mut map = HashMap::new();
        let mut fitting = std::iter::once(&self.constructors[class])
            .chain(&self.secondary[class])
            .filter(|signature| signature.parameters.len() == args.len());
        let signature = fitting.next()?;
        if fitting.next().is_some() {
            return None;
        }
        for ((_, pattern), arg) in signature.parameters.iter().zip(args) {
            self.infer_from(*pattern, self.static_kind(arg, scope)?, &mut map);
        }
        let arguments = parameters
            .into_iter()
            .map(|(_, kind)| map.get(&kind).copied())
            .collect::<Option<Vec<_>>>()?;
        Some(self.types.apply_class(class, arguments))
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn signature_attribute(
        &mut self,
        output: &mut Vec<u8>,
        start: usize,
        text: &str,
        name: Name<'_>,
    ) -> Result<(), Error> {
        if text.len() > u16::MAX as usize {
            return Err(at(name, "This generic signature is too long for the JVM."));
        }
        let count = u16::from_be_bytes([output[start + 6], output[start + 7]]) + 1;
        output[start + 6..start + 8].copy_from_slice(&count.to_be_bytes());
        let attribute = self.pool.ascii("Signature");
        let signature = self.pool.ascii(text);
        shorts(output, &[attribute]);
        word(output, 2);
        shorts(output, &[signature]);
        Ok(())
    }

    pub(super) fn method_signature(
        &mut self,
        output: &mut Vec<u8>,
        start: usize,
        signature: &Signature<'_>,
        name: Name<'_>,
    ) -> Result<(), Error> {
        if !signature.type_parameters.is_empty()
            || signature
                .parameters
                .iter()
                .map(|(_, kind)| *kind)
                .chain(signature.result)
                .any(|kind| self.symbols.types.needs_signature(kind))
        {
            let text = self.symbols.types.generic_signature(signature);
            self.signature_attribute(output, start, &text, name)?;
        }
        Ok(())
    }
    pub(super) fn generic_construct(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let (local, _) = native_generics::call_name(name);
        let Some(&Kind::Object(class)) = self.symbols.types.names.get(local.text) else {
            return Ok(None);
        };
        if self.symbols.types.class_parameters.borrow()[class].is_empty() {
            return Ok(None);
        }
        if self.symbols.class_declarations[class].inner_of.is_some()
            && !std::mem::take(&mut self.inner_ready)
        {
            return self.create_inner(method, class, name, args, base, None);
        }
        if method.binding(local).is_some()
            || self.fields.contains_key(local.text)
            || self.symbols.global(local.text).is_some()
        {
            return Ok(None);
        }
        if self.symbols.is_abstract_class(class) {
            return Err(at(
                name,
                "This class is abstract. Create one of its subclasses instead.",
            ));
        }
        let signature = self.constructor_for(method, class, name, args, false)?;
        let args = self.complete_args(
            method,
            name,
            signature,
            args,
            DefaultHost::Constructor(class),
        )?;
        let mut inference = signature.clone();
        inference.type_parameters = self.symbols.types.class_parameters.borrow()[class].clone();
        inference.result = Some(self.symbols.types.self_kind(class));
        let map = self.infer_call(method, name, args, base, &inference, expected)?;
        let arguments = inference
            .type_parameters
            .iter()
            .map(|(_, kind)| map[kind])
            .collect();
        let kind = self.symbols.types.apply_class(class, arguments);
        let receiver = self.object_class(kind);
        let constructor = self.member(Some(kind), "<init>", &signature.descriptor);
        let uninitialized = Local::Uninitialized(method.code.len() as u16);
        method.max_stack = method.max_stack.max(base + 2);
        instruction(&mut method.code, 0xbb, receiver);
        method.code.push(0x59);
        method.stack.extend([uninitialized, uninitialized]);
        self.adapted_arguments(method, args, base + 2, signature, &map)?;
        method.stack.truncate(base as usize);
        instruction(&mut method.code, 0xb7, constructor);
        Ok(Some(kind))
    }

    /// Calls a method through a parameterized interface such as `Producer<Int>`,
    /// substituting the interface's arguments into the erased signature.
    pub(super) fn applied_interface_method(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let interface = self.symbols.types.interface_root(kind).unwrap();
        let symbols = self.symbols;
        let signature = symbols.interfaces[interface]
            .get(name.text)
            .ok_or_else(|| at(name, "This type has no method with that name."))?;
        let args =
            self.complete_args(method, name, signature, args, DefaultHost::Instance(kind))?;
        let map = self.symbols.types.class_map(kind);
        method.push_stack(self.local_type(kind));
        self.adapted_arguments(method, args, base + 1, signature, &map)?;
        method.pop_stack();
        let reference = self.member(Some(kind), name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb9, reference); // invokeinterface
        method.code.extend([(signature.slots + 1) as u8, 0]);
        method.max_stack = method
            .max_stack
            .max(base + signature.result.map_or(1, Kind::slots));
        Ok(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            if erased != result {
                self.cast_top(method, erased, result, base);
            }
            result
        }))
    }

    pub(super) fn applied_method(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
    ) -> Result<Option<Kind>, Error> {
        let class = self.symbols.types.native_class(kind).unwrap();
        if self.symbols.find_method(class, name.text).is_none() {
            if let Some((_, field)) = self.symbols.find_field(class, name.text)
                && matches!(field.kind, Kind::Function(_))
            {
                let value = self.field_on(method, kind, name, base, allow_null)?;
                let Kind::Function(index) = value else {
                    unreachable!()
                };
                return self.invoke_function(method, index, name, args, base);
            }
            return self
                .java_inherited_call(method, kind, class, name, args, base, allow_null, None);
        }
        let signature = self.find_class_method(class, name)?;
        let args =
            self.complete_args(method, name, signature, args, DefaultHost::Instance(kind))?;
        let owner = self.symbols.find_method(class, name.text).unwrap().0;
        let map = self.symbols.member_map(kind, owner);
        method.push_stack(self.local_type(kind));
        self.adapted_arguments(method, args, base + 1, signature, &map)?;
        method.pop_stack();
        let reference = self.member(Some(kind), name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb6, reference);
        Ok(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            if erased != result {
                self.cast_top(method, erased, result, base);
            }
            result
        }))
    }
}
