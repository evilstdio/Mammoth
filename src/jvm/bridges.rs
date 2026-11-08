//! Bridge methods for Mammoth generic supertypes.
//!
//! A generic interface or superclass method compiles to one erased JVM
//! method, such as `$get()Ljava/lang/Object;` for `fun get(): T`. A class that
//! implements `Producer<Int>` declares `$get()I`, so the JVM would not see the
//! two as related. Each such pair gets a bridge with the erased descriptor that
//! casts or unboxes the arguments, calls the real method, and boxes the result.
use super::*;

impl<'a> Symbols<'a> {
    /// Java must see boxed overrides of generic methods: an int-returning
    /// method alone cannot implement Source<Integer>.next in Java source.
    /// Keep a primitive forwarding entry for Mammoth calls, and put the body
    /// in a boxed entry so overrides from either language dispatch correctly.
    pub(super) fn interface_body_signature(&self, interface: usize, name: &str) -> Signature<'a> {
        let mut signature = self.interfaces[interface][name].clone();
        for view in self
            .interface_ancestors(Kind::Interface(interface))
            .into_iter()
            .skip(1)
        {
            let root = self.types.interface_root(view).unwrap();
            let Some(parent) = self.interfaces[root].get(name) else {
                continue;
            };
            for ((_, kind), (_, inherited)) in
                signature.parameters.iter_mut().zip(&parent.parameters)
            {
                if !kind.is_reference() && inherited.is_reference() {
                    *kind = kind.nullable().unwrap();
                }
            }
            if let (Some(kind), Some(inherited)) = (signature.result, parent.result)
                && !kind.is_reference()
                && inherited.is_reference()
            {
                signature.result = kind.nullable();
            }
        }
        signature.descriptor = String::from("(");
        signature.slots = 0;
        for (_, kind) in &signature.parameters {
            signature.descriptor.push_str(&kind.descriptor(&self.types));
            signature.slots += kind.slots();
        }
        signature.descriptor.push(')');
        signature.descriptor.push_str(
            &signature
                .result
                .map_or(Cow::Borrowed("V"), |kind| kind.descriptor(&self.types)),
        );
        signature
    }

    /// Parent descriptors that a specialized child-interface override must
    /// continue to implement, such as Object-returning Source<T>.next.
    pub(super) fn interface_bridges(&self, interface: usize, name: &str) -> Vec<Signature<'a>> {
        let implementation = &self.interfaces[interface][name];
        let body = self.interface_body_signature(interface, name);
        let mut bridges: Vec<Signature<'a>> = Vec::new();
        if body.descriptor != implementation.descriptor {
            bridges.push(implementation.clone());
        }
        for kind in self
            .interface_ancestors(Kind::Interface(interface))
            .into_iter()
            .skip(1)
        {
            let root = self.types.interface_root(kind).unwrap();
            if let Some(required) = self.interfaces[root].get(name) {
                for signature in [required.clone(), self.interface_body_signature(root, name)] {
                    if signature.descriptor != body.descriptor
                        && !bridges
                            .iter()
                            .any(|other| other.descriptor == signature.descriptor)
                    {
                        bridges.push(signature);
                    }
                }
            }
        }
        bridges
    }

    /// Erased signatures that `name` must also answer to in `class`: methods of
    /// interfaces the class lists itself, and methods of its Mammoth
    /// superclasses (and of their interfaces) that the class overrides. Only signatures whose
    /// descriptors differ from the implementation are returned.
    pub(super) fn native_bridges(&self, class: usize, name: &str) -> Vec<Signature<'a>> {
        let Some((_, implementation)) = self.find_method(class, name) else {
            return Vec::new();
        };
        let mut candidates: Vec<Signature<'a>> = Vec::new();
        for &listed in &self.implementations[class] {
            for kind in self.interface_ancestors(listed) {
                if let Some(interface) = self.types.interface_root(kind)
                    && let Some(required) = self.interfaces[interface].get(name)
                {
                    candidates.push(required.clone());
                    candidates.push(self.interface_body_signature(interface, name));
                }
            }
        }
        if self.methods[class].contains_key(name) {
            for owner in self.ancestors(class).skip(1) {
                if let Some(parent) = self.methods[owner].get(name)
                    && !self.method_flags[owner][name].0
                {
                    candidates.push(parent.clone());
                }
                // An override of a default method that a superclass inherits
                // from its interface, such as `tag(Int)` for `Named<Int>`
                // listed by the superclass, must answer to the erased
                // interface method too, or calls would reach the default.
                for &listed in &self.implementations[owner] {
                    for kind in self.interface_ancestors(listed) {
                        if let Some(interface) = self.types.interface_root(kind)
                            && let Some(required) = self.interfaces[interface].get(name)
                        {
                            candidates.push(required.clone());
                            candidates.push(self.interface_body_signature(interface, name));
                        }
                    }
                }
            }
        }
        let mut bridges: Vec<Signature<'a>> = Vec::new();
        for signature in candidates {
            if signature.descriptor != implementation.descriptor
                && !bridges
                    .iter()
                    .any(|existing| existing.descriptor == signature.descriptor)
            {
                bridges.push(signature);
            }
        }
        bridges
    }

    /// Methods required by the class's own Mammoth interfaces that it inherits
    /// rather than declares, and which therefore need bridges in this class.
    pub(super) fn inherited_bridge_names(&self, class: usize) -> Vec<&'a str> {
        let mut names = Vec::new();
        for &listed in &self.implementations[class] {
            for kind in self.interface_ancestors(listed) {
                let Some(interface) = self.types.interface_root(kind) else {
                    continue;
                };
                for method in &self.interface_declarations[interface].methods {
                    let name = method.name.text;
                    if !self.methods[class].contains_key(name) && !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        names
    }
}

impl<'a> Emitter<'a> {
    /// Writes a bridge with the descriptor of `erased` that forwards to the
    /// implementation `signature` of method `name` in `class`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn native_bridge(
        &mut self,
        output: &mut Vec<u8>,
        receiver: Kind,
        this_class: u16,
        name: &'a str,
        signature: &Signature<'a>,
        erased: &Signature<'_>,
        code_name: u16,
        stack_map_name: u16,
    ) -> Result<(), Error> {
        let mut bridge = Method::new(vec![Local::Object(this_class)]);
        self.mark_bridge(&mut bridge);
        let mut slots = Vec::with_capacity(erased.parameters.len());
        for &(_, kind) in &erased.parameters {
            let local = self.local_type(kind);
            slots.push(bridge.local(local));
        }
        bridge.access(0x19, 0);
        bridge.stack.push(Local::Object(this_class));
        for ((&(_, from), &(_, to)), slot) in erased
            .parameters
            .iter()
            .zip(&signature.parameters)
            .zip(slots)
        {
            let depth = bridge.stack.len() as u16;
            bridge.access(from.load(), slot);
            bridge.max_stack = bridge.max_stack.max(depth + from.slots());
            if from.descriptor(&self.symbols.types) != to.descriptor(&self.symbols.types) {
                self.cast_top(&mut bridge, from, to, depth);
            }
            let local = self.local_type(to);
            bridge.push_stack(local);
        }
        bridge.stack.clear();
        // Bridges may target a boxed entry with the same name as the primitive
        // entry cached by source calls. Resolve by the full JVM descriptor.
        let owner = self.object_class(receiver);
        let tag = if self.symbols.is_interface(receiver) {
            11
        } else {
            10
        };
        let target = self
            .pool
            .member(tag, owner, &format!("${name}"), &signature.descriptor);
        if self.symbols.types.interface_root(receiver).is_some() {
            instruction(&mut bridge.code, 0xb9, target);
            bridge.code.extend([(signature.slots + 1) as u8, 0]);
        } else {
            instruction(&mut bridge.code, 0xb6, target);
        }
        match (signature.result, erased.result) {
            (Some(actual), Some(wanted)) => {
                bridge.max_stack = bridge.max_stack.max(actual.slots().max(1) + 1);
                if actual.descriptor(&self.symbols.types) != wanted.descriptor(&self.symbols.types)
                {
                    self.cast_top(&mut bridge, actual, wanted, 0);
                }
                bridge.code.push(wanted.return_opcode());
            }
            _ => bridge.code.push(0xb1),
        }
        bridge.max_stack = bridge.max_stack.max(1);
        let name = self.pool.ascii(&format!("${name}"));
        let descriptor = self.pool.ascii(&erased.descriptor);
        bridge.write(output, 0x1041, name, descriptor, code_name, stack_map_name)?; // public synthetic bridge
        Ok(())
    }
}
