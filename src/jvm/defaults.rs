//! Default and named arguments. A parameter's default value is compiled into a
//! hidden helper, `name$default$index`, that receives the earlier parameters
//! it mentions. A call that leaves out or names arguments is rewritten here
//! into an ordinary positional call, so every call path shares one meaning.

use super::{Emitter, Error, Expr, Kind, Method, Name, Signature, at, native_generics};

/// Where a call's default-value helpers live.
#[derive(Clone, Copy)]
pub(super) enum DefaultHost {
    Function,
    /// A static method of this class.
    Static(usize),
    /// A constructor of this class.
    Constructor(usize),
    /// A method of the object on top of the stack, which has this type.
    Instance(Kind),
}

impl<'a> Emitter<'a> {
    fn default_signature(&self, host: DefaultHost, helper: &str) -> Option<&'a Signature<'a>> {
        let symbols = self.symbols;
        match host {
            DefaultHost::Function => symbols.functions.get(helper),
            DefaultHost::Static(class) | DefaultHost::Constructor(class) => symbols
                .find_static_method(class, helper)
                .map(|(_, signature)| signature),
            DefaultHost::Instance(kind) => {
                if let Some(interface) = symbols.types.interface_root(kind) {
                    symbols.interfaces[interface].get(helper)
                } else {
                    let class = symbols.types.native_class(kind)?;
                    symbols
                        .find_method(class, helper)
                        .map(|(_, signature)| signature)
                }
            }
        }
    }

    /// Whether evaluating `expr` twice, or at another time, cannot be noticed.
    fn is_pure(&self, method: &Method<'a>, expr: &Expr<'a>) -> bool {
        match expr {
            Expr::Int(..)
            | Expr::Long(..)
            | Expr::Double(..)
            | Expr::Char(..)
            | Expr::Bool(..)
            | Expr::Null(_)
            | Expr::Lambda(_)
            | Expr::Temp(..) => true,
            Expr::Variable(name) => method.binding(*name).is_some_and(|local| !local.mutable),
            _ => false,
        }
    }

    /// Stores `value` in a new local and returns an expression that reads it.
    fn temporary(
        &mut self,
        method: &mut Method<'a>,
        value: &'a Expr<'a>,
        parameter: Kind,
    ) -> Result<Expr<'a>, Error> {
        let depth = method.stack.len() as u16;
        let expected = (!self.symbols.types.contains_parameter(parameter)).then_some(parameter);
        let mut kind = self.expression_as(method, value, depth, expected)?;
        if let Some(expected) = expected {
            self.coerce(method, value, kind, expected, depth)?;
            kind = expected;
        }
        let slot = method.local(self.local_type(kind));
        method.access(kind.store(), slot);
        Ok(Expr::Temp(kind, slot))
    }

    /// Gathers the arguments of a `vararg` parameter into one list.
    fn complete_varargs(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        signature: &Signature<'a>,
        args: &'a [Expr<'a>],
        host: DefaultHost,
    ) -> Result<&'a [Expr<'a>], Error> {
        let fixed = signature.parameters.len() - 1;
        let mut plain = signature.clone();
        plain.vararg = false;
        // `f(1, *rest)` passes an existing list.
        if args.len() == fixed + 1
            && let Expr::Spread(inner) = &args[fixed]
        {
            let mut direct = args[..fixed].to_vec();
            direct.push((**inner).clone());
            let direct: &'a [Expr<'a>] = Box::leak(direct.into_boxed_slice());
            return self.complete_args(method, name, &plain, direct, host);
        }
        if args
            .iter()
            .any(|arg| matches!(arg, Expr::Named(..) | Expr::Spread(_)))
        {
            return Err(at(
                name,
                "Pass a vararg parameter's values by position, or one list as *items.",
            ));
        }
        let count = args.len().min(fixed);
        let mut full = args[..count].to_vec();
        if args.len() >= fixed {
            full.push(Expr::List(name.offset, args[fixed..].to_vec()));
        }
        let full: &'a [Expr<'a>] = Box::leak(full.into_boxed_slice());
        self.complete_args(method, name, &plain, full, host)
    }

    /// Returns `args` in parameter order with every omitted argument filled
    /// in. A call that already lists each parameter is returned unchanged.
    pub(super) fn complete_args(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        signature: &Signature<'a>,
        args: &'a [Expr<'a>],
        host: DefaultHost,
    ) -> Result<&'a [Expr<'a>], Error> {
        let parameters = signature.parameters.as_slice();
        let named = args.iter().any(|arg| matches!(arg, Expr::Named(..)));
        if signature.vararg {
            return self.complete_varargs(method, name, signature, args, host);
        }
        if args.len() == parameters.len() && !named {
            return Ok(args);
        }
        let wrong_count = || at(name, "This call has the wrong number of arguments.");
        let count = parameters.len();
        let mut bound: Vec<Option<&'a Expr<'a>>> = vec![None; count];
        let mut source_order = Vec::with_capacity(args.len());
        let mut seen_named = false;
        for (position, arg) in args.iter().enumerate() {
            let index = if let Expr::Named(label, value) = arg {
                seen_named = true;
                let index = parameters
                    .iter()
                    .position(|(parameter, _)| parameter.text == label.text)
                    .ok_or_else(|| at(*label, "This function has no parameter with that name."))?;
                if bound[index].is_some() {
                    return Err(at(*label, "This parameter already has a value."));
                }
                bound[index] = Some(value);
                source_order.push(index);
                continue;
            } else if matches!(arg, Expr::Lambda(_))
                && position + 1 == args.len()
                && position + 1 < count
                && !matches!(parameters[position].1.non_null(), Kind::Function(_))
                && bound[count - 1].is_none()
            {
                // A trailing lambda belongs to the last parameter.
                count - 1
            } else if seen_named {
                return Err(at(
                    name,
                    "Write positional arguments before named arguments.",
                ));
            } else if position < count {
                position
            } else {
                return Err(wrong_count());
            };
            if bound[index].is_some() {
                return Err(wrong_count());
            }
            bound[index] = Some(arg);
            source_order.push(index);
        }

        // Find each omitted parameter's helper and what it reads.
        let base = native_generics::call_name(name).0.text;
        let base = if matches!(host, DefaultHost::Constructor(_)) {
            "new"
        } else {
            base
        };
        let mut helpers: Vec<Option<(Name<'a>, &'a Signature<'a>)>> = vec![None; count];
        let mut read_by_default = vec![false; count];
        for index in 0..count {
            if bound[index].is_some() {
                continue;
            }
            let text: &'a str = Box::leak(format!("{base}$default${index}").into_boxed_str());
            let signature = self.default_signature(host, text).ok_or_else(wrong_count)?;
            helpers[index] = Some((
                Name {
                    text,
                    offset: name.offset,
                },
                signature,
            ));
            for (read, _) in &signature.parameters {
                let earlier = parameters
                    .iter()
                    .position(|(parameter, _)| parameter.text == read.text)
                    .expect("a default helper only receives earlier parameters");
                read_by_default[earlier] = true;
            }
        }

        // Arguments keep their written order when they have side effects.
        let reordered = source_order.windows(2).any(|pair| pair[0] > pair[1]);
        let impure = |emitter: &Self, index: usize, method: &Method<'a>| {
            bound[index].is_some_and(|value| !emitter.is_pure(method, value))
        };
        let shared = (0..count).any(|index| read_by_default[index] && impure(self, index, method));
        let many = (0..count)
            .filter(|&index| impure(self, index, method))
            .count();
        let use_temporaries = shared || (reordered && many > 1);
        let mut values: Vec<Option<Expr<'a>>> = (0..count).map(|_| None).collect();
        if use_temporaries {
            for &index in &source_order {
                if impure(self, index, method) {
                    let value = bound[index].unwrap();
                    values[index] = Some(self.temporary(method, value, parameters[index].1)?);
                }
            }
        }
        for index in 0..count {
            if values[index].is_none()
                && let Some(value) = bound[index]
            {
                values[index] = Some(value.clone());
            }
        }

        // The helper of an instance method needs the receiver again.
        let receiver = match host {
            DefaultHost::Instance(kind) if helpers.iter().any(Option::is_some) => {
                method.code.push(0x59); // dup
                let depth = method.stack.len() as u16;
                method.max_stack = method.max_stack.max(depth + 2);
                let slot = method.local(self.local_type(kind));
                method.access(0x3a, slot); // astore
                Some(Expr::Temp(kind, slot))
            }
            _ => None,
        };
        for index in 0..count {
            let Some((helper, signature)) = helpers[index] else {
                continue;
            };
            let inputs: Vec<Expr<'a>> = signature
                .parameters
                .iter()
                .map(|(read, _)| {
                    let earlier = parameters
                        .iter()
                        .position(|(parameter, _)| parameter.text == read.text)
                        .unwrap();
                    values[earlier]
                        .clone()
                        .expect("defaults only read earlier parameters")
                })
                .collect();
            let call = match host {
                DefaultHost::Function => Expr::Call(helper, inputs),
                DefaultHost::Static(class) | DefaultHost::Constructor(class) => {
                    Expr::StaticHelper(class, helper, inputs)
                }
                DefaultHost::Instance(_) => Expr::MethodCall(
                    Box::new(receiver.clone().expect("the receiver was saved")),
                    helper,
                    inputs,
                ),
            };
            values[index] = Some(if read_by_default[index] {
                let call: &'a Expr<'a> = Box::leak(Box::new(call));
                self.temporary(method, call, parameters[index].1)?
            } else {
                call
            });
        }
        let completed: Vec<Expr<'a>> = values.into_iter().map(Option::unwrap).collect();
        Ok(Box::leak(completed.into_boxed_slice()))
    }
}
