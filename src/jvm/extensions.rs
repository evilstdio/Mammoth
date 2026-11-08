//! Extension functions such as `fun Text.shout(): Text`. Each compiles to a
//! static function whose first parameter is `this`. A member of the receiver
//! always wins. Only names that some extension declares reach this module, so
//! other method calls pay nothing.

use super::{Emitter, Error, Expr, Kind, Method, Name};

impl<'a> Emitter<'a> {
    /// The hidden function for extension `name` that accepts a `receiver`.
    fn find_extension(&self, name: &str, receiver: Kind) -> Option<&'a str> {
        let symbols = self.symbols;
        let candidates = symbols.extensions.get(name)?;
        let fits = |hidden: &&'a str, exact: bool| {
            let declared = symbols.functions[*hidden].parameters[0].1;
            if symbols.types.contains_parameter(declared) {
                let mut map = std::collections::HashMap::new();
                symbols.infer_from(declared, receiver, &mut map);
                let target = symbols.types.replace_parameters(declared, &map);
                !symbols.types.contains_parameter(target)
                    && (symbols.types.same_kind(target, receiver)
                        || (!exact && symbols.fits(receiver, target)))
            } else if exact {
                symbols.types.same_kind(declared, receiver)
            } else {
                symbols.fits(receiver, declared)
            }
        };
        // A receiver written with concrete types beats a generic one.
        let concrete = |hidden: &&&'a str| {
            !symbols
                .types
                .contains_parameter(symbols.functions[**hidden].parameters[0].1)
        };
        candidates
            .iter()
            .filter(concrete)
            .find(|hidden| fits(hidden, true))
            .or_else(|| candidates.iter().find(|hidden| fits(hidden, true)))
            .or_else(|| {
                candidates
                    .iter()
                    .filter(concrete)
                    .find(|hidden| fits(hidden, false))
            })
            .or_else(|| candidates.iter().find(|hidden| fits(hidden, false)))
            .copied()
    }

    /// Calls the extension `name` on the receiver already on the stack.
    /// Returns `None` when no extension of that name fits it.
    #[allow(clippy::option_option)]
    pub(super) fn extension_on_stack(
        &mut self,
        method: &mut Method<'a>,
        receiver: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let (plain, _) = super::native_generics::call_name(name);
        let Some(hidden) = self.find_extension(plain.text, receiver) else {
            return Ok(None);
        };
        let slot = method.local(self.local_type(receiver));
        method.access(receiver.store(), slot);
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(Expr::Temp(receiver, slot));
        full.extend(args.iter().cloned());
        let full: &'a [Expr<'a>] = Box::leak(full.into_boxed_slice());
        self.call_expected(
            method,
            Name {
                text: hidden,
                offset: name.offset,
            },
            full,
            base,
            expected,
        )
        .map(Some)
    }

    /// Calls a member named `name` if the receiver has one, and otherwise the
    /// extension function that fits the receiver's type.
    pub(super) fn member_or_extension(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let snapshot = method.snapshot();
        let allow_null = self.allow_null;
        let member_error = match self.member_call(method, receiver, name, args, base, expected) {
            Ok(result) => return Ok(result),
            Err(error) => error,
        };
        method.restore(snapshot);
        self.allow_null = false;
        // Learn the receiver's type without keeping the code.
        let probe = method.snapshot();
        let kind = self.expression(method, receiver, base);
        method.restore(probe);
        self.allow_null = allow_null;
        let Ok(kind) = kind else {
            return Err(member_error);
        };
        let (plain, explicit) = super::native_generics::call_name(name);
        // A fixed list reaches collection operations by copying it first.
        let (kind, receiver): (Kind, &'a Expr<'a>) = match kind {
            Kind::List(list) if self.find_extension(plain.text, kind).is_none() => {
                let copy = Expr::MethodCall(
                    Box::new(receiver.clone()),
                    Name {
                        text: "toMutableList",
                        offset: name.offset,
                    },
                    Vec::new(),
                );
                (Kind::MutableList(list), Box::leak(Box::new(copy)))
            }
            _ => (kind, receiver),
        };
        let Some(hidden) = self.find_extension(plain.text, kind) else {
            return Err(member_error);
        };
        let text: &'a str = match explicit {
            None => hidden,
            Some(_) => {
                Box::leak(format!("{hidden}{}", &name.text[plain.text.len()..]).into_boxed_str())
            }
        };
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(receiver.clone());
        full.extend(args.iter().cloned());
        let full: &'a [Expr<'a>] = Box::leak(full.into_boxed_slice());
        self.call_expected(
            method,
            Name {
                text,
                offset: name.offset,
            },
            full,
            base,
            expected,
        )
    }
}
