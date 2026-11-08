//! Extension functions such as `fun Text.shout(): Text`. Each compiles to a
//! static function whose first parameter is `this`. A member of the receiver
//! always wins. Only names that some extension declares reach this module, so
//! other method calls pay nothing.

use super::{Emitter, Error, Expr, Kind, Method, Name, at};

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
                symbols.functions[*hidden]
                    .type_parameters
                    .iter()
                    .filter(|(_, parameter)| {
                        let probe = std::collections::HashMap::from([(*parameter, Kind::Any)]);
                        symbols.types.replace_parameters(declared, &probe) != declared
                    })
                    .all(|(_, parameter)| map.contains_key(parameter))
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

    /// `listOf(1, 2)`, `mutableListOf`, `setOf` and `mapOf("a" to 1)`, unless
    /// the program declares a function with that name.
    pub(super) fn collection_builder(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let (plain, explicit) = super::native_generics::call_name(name);
        if !matches!(
            plain.text,
            "listOf"
                | "mutableListOf"
                | "setOf"
                | "mapOf"
                | "mutableMapOf"
                | "arrayOf"
                | "intArrayOf"
                | "longArrayOf"
                | "doubleArrayOf"
                | "charArrayOf"
                | "booleanArrayOf"
        ) || self.symbols.functions.contains_key(plain.text)
            || method.binding(plain).is_some()
        {
            return Ok(None);
        }
        let named = |text: &'a str| Name {
            text,
            offset: name.offset,
        };
        let list = || Expr::List(name.offset, args.to_vec());
        if plain.text.ends_with("rrayOf") {
            let literal: &'a Expr<'a> = Box::leak(Box::new(list()));
            return self
                .expression_as(method, literal, base, expected)
                .map(Some);
        }
        let is_map = matches!(plain.text, "mapOf" | "mutableMapOf");
        if explicit.is_none()
            && !args.is_empty()
            && matches!(plain.text, "listOf" | "mutableListOf")
        {
            let element = match expected.map(Kind::non_null) {
                Some(Kind::MutableList(list)) => Some(list),
                _ => self.element_with_nulls(method, args),
            };
            if element.is_some() {
                return self
                    .mutable_list(method, args, base, element, name.offset)
                    .map(Some);
            }
        }
        let call = if args.is_empty() {
            let written = name.text.get(plain.text.len()..).unwrap_or("");
            if explicit.is_none() {
                if !is_map && plain.text != "setOf" {
                    // The destination's type gives the element type.
                    let element = match expected.map(Kind::non_null) {
                        Some(Kind::MutableList(list)) => Some(list),
                        _ => None,
                    };
                    return self
                        .mutable_list(method, args, base, element, name.offset)
                        .map(Some);
                }
                return Err(at(
                    name,
                    "Give the types of an empty collection, such as setOf<Int>().",
                ));
            }
            let class = match plain.text {
                "listOf" | "mutableListOf" => "MutableList",
                "setOf" => "__LinkedHashSet",
                _ => "__LinkedHashMap",
            };
            let text: &'a str = Box::leak(format!("{class}{written}").into_boxed_str());
            Expr::Call(named(text), Vec::new())
        } else if is_map {
            let pairs = Expr::MethodCall(Box::new(list()), named("toMutableList"), Vec::new());
            Expr::Call(named("__mapOf"), vec![pairs])
        } else {
            let method_name = if plain.text == "setOf" {
                "toSet"
            } else {
                "toMutableList"
            };
            Expr::MethodCall(Box::new(list()), named(method_name), Vec::new())
        };
        let call: &'a Expr<'a> = Box::leak(Box::new(call));
        self.expression_as(method, call, base, expected).map(Some)
    }

    /// The nullable element type of `listOf(1, null)`, when an item is `null`.
    fn element_with_nulls(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
    ) -> Option<super::ListKind> {
        if !args.iter().any(|arg| matches!(arg, Expr::Null(_))) {
            return None;
        }
        let first = args.iter().find(|arg| !matches!(arg, Expr::Null(_)))?;
        let saved = method.snapshot();
        let base = method.stack.len() as u16;
        let kind = self.expression(method, first, base);
        method.restore(saved);
        super::ListKind::for_element(kind.ok()?.nullable()?)
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
        let found = self.find_extension(plain.text, receiver);
        let found: Option<&'a str> =
            found.map(|hidden| hidden.split("$ov$").next().unwrap_or(hidden));
        let Some(hidden) = found else {
            // A fixed list reaches collection operations by copying it first.
            if let Kind::List(list) = receiver
                && self
                    .find_extension(plain.text, Kind::MutableList(list))
                    .is_some()
            {
                let copy = Name {
                    text: "toMutableList",
                    offset: name.offset,
                };
                if let Some(Some(copied)) =
                    self.extension_on_stack(method, receiver, copy, &[], base, None)?
                {
                    return self.extension_on_stack(method, copied, name, args, base, expected);
                }
            }
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
        // A member that exists but cannot be used here must not be replaced.
        if member_error.message.contains("protected") || member_error.message.contains("private") {
            return Err(member_error);
        }
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
        // Overloads of an extension are chosen among themselves by the call.
        let hidden = hidden.split("$ov$").next().unwrap_or(hidden);
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
