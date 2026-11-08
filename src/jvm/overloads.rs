//! Overloaded functions and methods. Several declarations of one name in a
//! scope get hidden names built from their parameter types, such as
//! `add$ov$Int_Text`, and each call picks one from its arguments. Only names
//! that are overloaded somewhere reach this module.

use super::{Emitter, Error, Expr, Kind, Method, Name, Signature, at};

/// Stand-ins for a lambda's parameter count when none is written.
const IMPLICIT_ONE: usize = usize::MAX;
const IMPLICIT_ZERO: usize = usize::MAX - 1;

impl<'a> Emitter<'a> {
    fn overloads_of(&self, name: &str) -> Option<&'a Vec<&'a str>> {
        let plain = name.split('<').next().unwrap_or(name);
        self.symbols.overloads.get(plain)
    }

    /// The types of the arguments, learned without keeping any code. `None`
    /// stands for a lambda or a value whose type depends on its destination.
    fn probe_arguments(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
    ) -> Vec<(Option<&'a str>, Option<Kind>, Option<usize>)> {
        args.iter()
            .map(|arg| {
                let (label, value) = match arg {
                    Expr::Named(label, value) => (Some(label.text), value.as_ref()),
                    other => (None, other),
                };
                if let Expr::Lambda(lambda) = value {
                    // Without a written list, `{ ... }` takes one `it` or none.
                    let arity = if lambda.explicit {
                        lambda.parameters.len()
                    } else {
                        let mut names = std::collections::HashSet::new();
                        super::globals::collect_statements(&lambda.body, &mut names);
                        if let Some(value) = &lambda.value {
                            super::globals::collect_expr(value, &mut names);
                        }
                        if names.contains("it") {
                            IMPLICIT_ONE
                        } else {
                            IMPLICIT_ZERO
                        }
                    };
                    return (label, None, Some(arity));
                }
                let saved = method.snapshot();
                let allow_null = std::mem::take(&mut self.allow_null);
                let base = method.stack.len() as u16;
                let kind = self.expression(method, value, base).ok();
                method.restore(saved);
                self.allow_null = allow_null;
                (label, kind, None)
            })
            .collect()
    }

    /// The conversion cost of passing `actual` to `parameter`, or `None`.
    fn argument_cost(&self, actual: Kind, parameter: Kind) -> Option<u32> {
        let types = &self.symbols.types;
        if types.same_kind(actual, parameter) {
            return Some(0);
        }
        if actual == Kind::Null {
            return matches!(parameter, Kind::Nullable(_)).then_some(1);
        }
        if types.contains_parameter(parameter) {
            if matches!(parameter.non_null(), Kind::TypeParameter(_)) {
                return Some(6);
            }
            // A generic shape like `Iterable<T>` must really match.
            let mut map = std::collections::HashMap::new();
            self.symbols.infer_from(parameter, actual, &mut map);
            let target = types.replace_parameters(parameter, &map);
            return (!types.contains_parameter(target)
                && (types.same_kind(actual, target) || self.symbols.fits(actual, target)))
            .then_some(3);
        }
        match (actual, parameter) {
            (Kind::Int, Kind::Long) | (Kind::Long, Kind::Double) => return Some(1),
            (Kind::Int, Kind::Double) => return Some(2),
            _ => {}
        }
        self.symbols.fits(actual, parameter).then_some(2)
    }

    fn overload_score(
        &self,
        signature: &Signature<'a>,
        arguments: &[(Option<&'a str>, Option<Kind>, Option<usize>)],
    ) -> Option<u32> {
        let parameters = &signature.parameters;
        if arguments.len() > parameters.len() {
            return None;
        }
        let mut total = 0;
        for (position, (label, kind, lambda)) in arguments.iter().enumerate() {
            let index = match label {
                Some(label) => parameters
                    .iter()
                    .position(|(name, _)| name.text == *label)?,
                None => position,
            };
            let parameter = parameters[index].1;
            if let Some(arity) = lambda {
                if let Kind::Function(function) = parameter.non_null() {
                    let wanted = self.symbols.types.function_type(function);
                    match *arity {
                        IMPLICIT_ONE if wanted.parameters.len() != 1 => return None,
                        IMPLICIT_ZERO if wanted.parameters.len() > 1 => return None,
                        // A `{ ... }` ignoring its input can still take one.
                        IMPLICIT_ZERO => total += wanted.parameters.len() as u32,
                        IMPLICIT_ONE => {}
                        written if written != wanted.parameters.len() => return None,
                        _ => {}
                    }
                } else if !self.symbols.types.contains_parameter(parameter)
                    && !matches!(parameter.non_null(), Kind::Java(_))
                {
                    return None;
                }
            } else if let Some(kind) = kind {
                total += self.argument_cost(*kind, parameter)?;
            }
        }
        // Each parameter left out needs a default, and costs a little so an
        // exact match wins.
        Some(total + (parameters.len() - arguments.len()) as u32 * 8)
    }

    fn choose_overload(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        candidates: Vec<(&'a str, &'a Signature<'a>)>,
    ) -> Result<Name<'a>, Error> {
        let arguments = self.probe_arguments(method, args);
        let mut best: Vec<(&'a str, u32)> = Vec::new();
        for (hidden, signature) in candidates {
            let Some(score) = self.overload_score(signature, &arguments) else {
                continue;
            };
            match best.first() {
                Some(&(_, top)) if score > top => {}
                Some(&(_, top)) if score == top => best.push((hidden, score)),
                _ => best = vec![(hidden, score)],
            }
        }
        match best.as_slice() {
            [] => Err(at(
                name,
                "No version of this function accepts these arguments.",
            )),
            [(hidden, _)] => {
                let plain = name.text.split('<').next().unwrap_or(name.text);
                let suffix = &name.text[plain.len()..];
                let text: &'a str = if suffix.is_empty() {
                    hidden
                } else {
                    Box::leak(format!("{hidden}{suffix}").into_boxed_str())
                };
                Ok(Name {
                    text,
                    offset: name.offset,
                })
            }
            _ => Err(at(
                name,
                "This call matches more than one version of the function. Make the argument types exact.",
            )),
        }
    }

    /// The constructor of `class` that takes these arguments.
    pub(super) fn constructor_for(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        delegating: bool,
    ) -> Result<&'a Signature<'a>, Error> {
        let symbols = self.symbols;
        let secondary = &symbols.secondary[class];
        let primary = &symbols.constructors[class];
        if symbols.class_declarations[class].private_primary && self.current_class != Some(class) {
            return Err(at(name, "This constructor is private to its class."));
        }
        if secondary.is_empty() {
            return Ok(primary);
        }
        let arguments = self.probe_arguments(method, args);
        let mut best: Vec<(&'a Signature<'a>, u32)> = Vec::new();
        let hidden = symbols.class_declarations[class].no_primary && !delegating;
        let header = (!hidden).then_some(primary);
        for signature in header.into_iter().chain(secondary) {
            // Only the header's constructor has default values.
            if !std::ptr::eq(signature, primary) && signature.parameters.len() != args.len() {
                continue;
            }
            let Some(score) = self.overload_score(signature, &arguments) else {
                continue;
            };
            match best.first() {
                Some(&(_, top)) if score > top => {}
                Some(&(_, top)) if score == top => best.push((signature, score)),
                _ => best = vec![(signature, score)],
            }
        }
        match best.as_slice() {
            [] => Err(at(
                name,
                "No constructor of this class accepts these arguments.",
            )),
            [(signature, _)] => Ok(*signature),
            _ => {
                // An argument that could not be typed has the real problem.
                for arg in args {
                    if !matches!(arg, Expr::Lambda(_) | Expr::Named(..)) {
                        let saved = method.snapshot();
                        let base = method.stack.len() as u16;
                        let result = self.expression(method, arg, base);
                        method.restore(saved);
                        result?;
                    }
                }
                Err(at(
                    name,
                    "This call matches more than one constructor. Make the argument types exact.",
                ))
            }
        }
    }

    /// Resolves a bare call to one overload, if its name is overloaded here.
    pub(super) fn overloaded_function(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
    ) -> Result<Option<Name<'a>>, Error> {
        let Some(hidden) = self.overloads_of(name.text) else {
            return Ok(None);
        };
        let symbols = self.symbols;
        let current = self.current_class;
        let mut candidates = Vec::new();
        for &text in hidden {
            let signature = current
                .and_then(|class| symbols.find_method(class, text).map(|found| found.1))
                .or_else(|| {
                    current.and_then(|class| {
                        symbols.find_static_method(class, text).map(|found| found.1)
                    })
                })
                .or_else(|| symbols.functions.get(text));
            if let Some(signature) = signature {
                candidates.push((text, signature));
            }
        }
        if candidates.is_empty() {
            return Ok(None);
        }
        self.choose_overload(method, name, args, candidates)
            .map(Some)
    }

    /// Resolves `receiver.name(...)` to one overload of the receiver's type.
    pub(super) fn overloaded_method(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
    ) -> Result<Option<Name<'a>>, Error> {
        let Some(hidden) = self.overloads_of(name.text) else {
            return Ok(None);
        };
        let symbols = self.symbols;
        let mut candidates = Vec::new();
        let is_super = matches!(receiver, Expr::Variable(variable) if variable.text == "super");
        if let Some(class) = self.static_class(method, receiver) {
            for &text in hidden {
                if let Some((_, signature)) = symbols.find_static_method(class, text) {
                    candidates.push((text, signature));
                }
            }
            if !candidates.is_empty() {
                return self
                    .choose_overload(method, name, args, candidates)
                    .map(Some);
            }
        }
        let kind = if is_super {
            let class = self
                .current_class
                .and_then(|class| symbols.superclasses[class]);
            class.map(Kind::Object)
        } else {
            let saved = method.snapshot();
            let allow_null = std::mem::take(&mut self.allow_null);
            let base = method.stack.len() as u16;
            let kind = self.expression(method, receiver, base).ok();
            method.restore(saved);
            self.allow_null = allow_null;
            kind.map(Kind::non_null)
        };
        let Some(kind) = kind else {
            return Ok(None);
        };
        for &text in hidden {
            let signature = if let Some(class) = symbols.types.native_class(kind) {
                symbols.find_method(class, text).map(|found| found.1)
            } else {
                symbols
                    .types
                    .interface_root(kind)
                    .and_then(|interface| symbols.interfaces[interface].get(text))
            };
            if let Some(signature) = signature {
                candidates.push((text, signature));
            }
        }
        if candidates.is_empty() {
            return Ok(None);
        }
        self.choose_overload(method, name, args, candidates)
            .map(Some)
    }
}
