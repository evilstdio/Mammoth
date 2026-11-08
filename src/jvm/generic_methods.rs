//! Generic member calls combine the receiver's arguments with the method's
//! own arguments, while invoking the same erased JVM method for every call.
use super::defaults::DefaultHost;
use super::*;

pub(super) struct MemberCall<'a> {
    pub receiver: Kind,
    pub name: Name<'a>,
    pub args: &'a [Expr<'a>],
    pub base: u16,
    pub expected: Option<Kind>,
    pub special: bool,
}

impl<'a> Emitter<'a> {
    pub(super) fn generic_method(
        &mut self,
        method: &mut Method<'a>,
        mut call: MemberCall<'a>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let direct_owner = call.receiver;
        call.receiver = self.symbols.method_receiver(call.receiver, call.name.text);
        let (name, explicit) = native_generics::call_name(call.name);
        let signature = match call.receiver {
            kind if self.symbols.types.interface_root(kind).is_some() => {
                let index = self.symbols.types.interface_root(kind).unwrap();
                self.symbols.interfaces[index].get(name.text)
            }
            kind if self.symbols.types.native_class(kind).is_some() => {
                let class = self.symbols.types.native_class(kind).unwrap();
                self.symbols
                    .find_method(class, name.text)
                    .map(|(_, signature)| signature)
            }
            _ => return Ok(None),
        };
        let Some(signature) = signature else {
            return Ok(None);
        };
        let body = self
            .symbols
            .types
            .interface_root(call.receiver)
            .filter(|_| call.special)
            .map(|root| self.symbols.interface_body_signature(root, name.text));
        let abi = body.as_ref().unwrap_or(signature);
        if signature.type_parameters.is_empty() {
            if explicit.is_some() {
                return Err(at(call.name, "This method does not take type arguments."));
            }
            return Ok(None);
        }
        if let Some(class) = self.symbols.types.native_class(call.receiver) {
            self.find_class_method(class, name)?;
        }
        let mut map = match self.symbols.types.native_class(call.receiver) {
            Some(class) => {
                let owner = self.symbols.find_method(class, name.text).unwrap().0;
                self.symbols.member_map(call.receiver, owner)
            }
            None => self.symbols.types.class_map(call.receiver),
        };
        call.args = self.complete_args(
            method,
            call.name,
            &signature.parameters,
            call.args,
            DefaultHost::Instance(call.receiver),
        )?;
        let mut inference = signature.clone();
        for (_, kind) in &mut inference.parameters {
            *kind = self.symbols.types.replace_parameters(*kind, &map);
        }
        inference.result = inference
            .result
            .map(|kind| self.symbols.types.replace_parameters(kind, &map));
        method.push_stack(self.local_type(call.receiver));
        let own = self.infer_call_in(
            method,
            call.name,
            call.args,
            call.base + 1,
            &inference,
            call.expected,
            &map,
        )?;
        map.extend(own);
        self.adapted_arguments_to(method, call.args, call.base + 1, signature, &map, abi)?;
        method.pop_stack();
        let owner = if call.special {
            direct_owner
        } else {
            call.receiver
        };
        let reference = if body.is_some() {
            let class = self.object_class(owner);
            self.pool
                .member(11, class, &format!("${}", name.text), &abi.descriptor)
        } else {
            self.member(Some(owner), name.text, &signature.descriptor)
        };
        let opcode = if call.special {
            0xb7
        } else if self.symbols.types.interface_root(call.receiver).is_some() {
            0xb9
        } else {
            0xb6
        };
        instruction(&mut method.code, opcode, reference);
        if opcode == 0xb9 {
            method.code.extend([(signature.slots + 1) as u8, 0]);
        }
        method.max_stack = method
            .max_stack
            .max(call.base + signature.result.map_or(1, Kind::slots));
        Ok(Some(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            let returned = abi.result.unwrap();
            if returned != result {
                self.cast_top(method, returned, result, call.base);
            }
            result
        })))
    }

    pub(super) fn implicit_generic_method(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let (local, explicit) = native_generics::call_name(name);
        if method.binding(local).is_some()
            || self.fields.contains_key(local.text)
            || self.symbols.global(local.text).is_some()
        {
            return Ok(None);
        }
        let Some(class) = self.current_class.filter(|_| self.this_ready) else {
            return Ok(None);
        };
        if method
            .binding(Name {
                text: "this",
                offset: name.offset,
            })
            .is_none()
        {
            return Ok(None);
        }
        let Some((_, signature)) = self.symbols.find_method(class, local.text) else {
            return Ok(None);
        };
        if signature.type_parameters.is_empty() {
            if explicit.is_some() {
                return Err(at(name, "This method does not take type arguments."));
            }
            return Ok(None);
        }
        method.access(0x19, 0);
        self.generic_method(
            method,
            MemberCall {
                receiver: self.symbols.types.self_kind(class),
                name,
                args,
                base,
                expected,
                special: false,
            },
        )
    }
}

impl Symbols<'_> {
    pub(super) fn generic_member_result(
        &self,
        receiver: Kind,
        name: Name<'_>,
        args: &[Expr<'_>],
        scope: &dyn Fn(&str) -> Option<Kind>,
    ) -> Option<Kind> {
        let receiver = self.method_receiver(receiver, name.text);
        let (name, explicit) = native_generics::call_name(name);
        let signature = match receiver {
            kind if self.types.interface_root(kind).is_some() => {
                self.interfaces[self.types.interface_root(kind).unwrap()].get(name.text)?
            }
            kind => {
                self.find_method(self.types.native_class(kind)?, name.text)?
                    .1
            }
        };
        let mut map = match self.types.native_class(receiver) {
            Some(class) => self.member_map(receiver, self.find_method(class, name.text)?.0),
            None => self.types.class_map(receiver),
        };
        if let Some(explicit) = explicit {
            if explicit.len() != signature.type_parameters.len() {
                return None;
            }
            for ((_, kind), annotation) in signature.type_parameters.iter().zip(explicit) {
                map.insert(*kind, resolve_type(&self.types, annotation).ok()?);
            }
        }
        for ((_, kind), arg) in signature.parameters.iter().zip(args) {
            if let Some(actual) = self.static_kind(arg, scope) {
                self.infer_from(self.types.replace_parameters(*kind, &map), actual, &mut map);
            }
        }
        if signature
            .type_parameters
            .iter()
            .any(|(_, kind)| !map.contains_key(kind))
        {
            return None;
        }
        Some(self.types.replace_parameters(signature.result?, &map))
    }
}
