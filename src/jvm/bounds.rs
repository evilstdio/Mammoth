//! Type parameter bounds, such as `T: Comparable<T>` or `A: Animal`.
//!
//! A bound is one class or interface type. Generic code may use a value of
//! type `T` wherever its bound is expected and may call the bound's methods
//! and read its fields. Every type argument, whether written or inferred,
//! must be a subtype of the bound after substituting the other arguments. A
//! primitive argument such as `Int` satisfies a Java bound that its boxed
//! class implements, so `Int` satisfies `Comparable<Int>`.
//!
//! As in Java, a bounded type parameter erases to its bound in JVM
//! descriptors, so generated signatures stay consistent for Java callers.
use super::*;

const UNSATISFIED: &str = "This type argument does not satisfy the bound of its type parameter.";

impl<'a> Types<'a> {
    /// Adds type parameters to `scope` without bounds and returns them.
    pub(super) fn push_parameters(
        &self,
        scope: &mut Types<'a>,
        names: &[Name<'a>],
    ) -> Result<TypeParameters<'a>, Error> {
        let mut declared = Vec::with_capacity(names.len());
        for &name in names {
            let index = self.parameters.borrow().len();
            let kind = Kind::TypeParameter(index);
            declare_type(scope, name, kind)?;
            self.parameters.borrow_mut().push(name.text);
            self.bounds.borrow_mut().push(Vec::new());
            self.variances
                .borrow_mut()
                .push(crate::syntax::Variance::Invariant);
            declared.push((name, kind));
        }
        Ok(declared)
    }

    /// Resolves the bounds of already declared type parameters in `scope`,
    /// which contains the parameters themselves so bounds such as
    /// `Comparable<T>` may refer to them.
    pub(super) fn resolve_bounds(
        &self,
        scope: &Types<'a>,
        parameters: &TypeParameters<'a>,
        bounds: &[Vec<Name<'a>>],
    ) -> Result<(), Error> {
        for (&(_, kind), written) in parameters.iter().zip(bounds) {
            let mut resolved = Vec::with_capacity(written.len());
            for (position, &annotation) in written.iter().enumerate() {
                let bound = resolve_type(scope, annotation)?;
                let valid = matches!(
                    bound,
                    Kind::Object(_)
                        | Kind::Applied(_)
                        | Kind::Interface(_)
                        | Kind::AppliedInterface(_)
                        | Kind::Any
                ) || matches!(bound, Kind::Java(index)
                        if !self.java_name(index).starts_with('['));
                if !valid {
                    return Err(at(
                        annotation,
                        "A bound must be a class or interface type, such as T: Comparable<T>.",
                    ));
                }
                if position > 0 && !self.is_interface_bound(bound) {
                    return Err(at(
                        annotation,
                        "Only the first bound can be a class. Later bounds must be interfaces.",
                    ));
                }
                if resolved
                    .iter()
                    .any(|&other| self.erased_bound(other) == self.erased_bound(bound))
                {
                    return Err(at(annotation, "This bound is already listed here."));
                }
                resolved.push(bound);
            }
            let Kind::TypeParameter(index) = kind else {
                unreachable!()
            };
            self.bounds.borrow_mut()[index] = resolved;
        }
        Ok(())
    }

    /// Whether a bound is an interface type, Mammoth or Java.
    pub(super) fn is_interface_bound(&self, bound: Kind) -> bool {
        match bound {
            Kind::Interface(_) | Kind::AppliedInterface(_) => true,
            Kind::Java(index) => self.is_java_interface(&self.java_name(index)),
            _ => false,
        }
    }

    /// The JVM class a bound erases to.
    fn erased_bound(&self, bound: Kind) -> Kind {
        match bound {
            Kind::Applied(application) => {
                Kind::Object(self.applications.borrow()[application].class)
            }
            Kind::AppliedInterface(application) => {
                Kind::Interface(self.applications.borrow()[application].class)
            }
            Kind::Java(java) => Kind::Java(self.java_index(&self.java_name(java))),
            bound => bound,
        }
    }

    /// Declares type parameters together with their bounds.
    pub(super) fn declare_parameters(
        &self,
        scope: &mut Types<'a>,
        names: &[Name<'a>],
        bounds: &[Vec<Name<'a>>],
    ) -> Result<TypeParameters<'a>, Error> {
        let declared = self.push_parameters(scope, names)?;
        self.resolve_bounds(scope, &declared, bounds)?;
        Ok(declared)
    }

    /// The first bound of a type parameter, which decides its erasure.
    pub(super) fn bound(&self, kind: Kind) -> Option<Kind> {
        match kind.non_null() {
            Kind::TypeParameter(index) => self.bounds.borrow()[index].first().copied(),
            _ => None,
        }
    }

    /// Every bound of a type parameter, in the order written.
    pub(super) fn all_bounds(&self, kind: Kind) -> Vec<Kind> {
        match kind.non_null() {
            Kind::TypeParameter(index) => self.bounds.borrow()[index].clone(),
            _ => Vec::new(),
        }
    }

    /// The type whose members a value of type `kind` offers: a bounded type
    /// parameter offers its bound's members.
    pub(super) fn bound_view(&self, kind: Kind) -> Kind {
        match self.bound(kind) {
            Some(bound) if matches!(kind, Kind::Nullable(_)) => bound.nullable().unwrap_or(bound),
            Some(bound) => bound,
            None => kind,
        }
    }

    /// The JVM erasure of a type parameter: its bound's class, or Object.
    pub(super) fn parameter_erasure(&self, index: usize) -> Kind {
        let first = self.bounds.borrow()[index].first().copied();
        match first {
            Some(Kind::Java(java)) if self.java_name(java) == "java/lang/Object" => Kind::Any,
            Some(bound) => self.erased_bound(bound),
            None => Kind::Any,
        }
    }

    /// One formal type parameter in a JVM signature, such as
    /// `T:Ljava/lang/Object;` or `T::Ljava/lang/Comparable<TT;>;`.
    pub(super) fn formal_parameter(&self, name: &str, kind: Kind) -> String {
        let bounds = self.all_bounds(kind);
        if bounds.is_empty() {
            return format!("{name}:Ljava/lang/Object;");
        }
        let mut text = name.to_owned();
        for (position, &bound) in bounds.iter().enumerate() {
            // A first interface bound leaves the class bound empty.
            if position == 0 && self.is_interface_bound(bound) {
                text.push(':');
            }
            text.push(':');
            text.push_str(&self.signature_type(bound, true));
        }
        text
    }

    /// Records a parameterized type written in the source so its arguments
    /// can be checked against bounds once every class is known.
    pub(super) fn defer_bound_check(&self, kind: Kind, offset: usize) {
        if matches!(kind, Kind::Java(_)) {
            if !self.type_arguments(kind).is_empty() {
                self.pending_bounds
                    .borrow_mut()
                    .push((kind, offset, self.module));
            }
            return;
        }
        let parameters = match kind {
            Kind::Applied(index) | Kind::AppliedInterface(index) => {
                let application = self.application(index);
                if application.interface {
                    self.interface_parameters.borrow()[application.class].clone()
                } else {
                    self.class_parameters.borrow()[application.class].clone()
                }
            }
            _ => return,
        };
        if parameters
            .iter()
            .any(|(_, kind)| self.bound(*kind).is_some())
        {
            self.pending_bounds
                .borrow_mut()
                .push((kind, offset, self.module));
        }
    }
}

impl<'a> Symbols<'a> {
    /// Whether a value of type `actual` can be used where `expected` is
    /// required, as `require` decides, without reporting an error.
    pub(super) fn fits(&self, actual: Kind, expected: Kind) -> bool {
        if self.types.same_kind(actual, expected) {
            return true;
        }
        if let Some(lower) = self.types.capture_lower(expected) {
            return actual == Kind::Null && matches!(expected, Kind::Nullable(_))
                || self.fits(actual, lower)
                || self.boxed_same(actual.non_null(), lower);
        }
        if self.types.bound(actual).is_some() {
            return self.bound_fits(actual, expected);
        }
        if !actual.is_reference() {
            return match expected.non_null() {
                Kind::Any => true,
                kind @ Kind::Java(_) => self.types.runtime_class_name(kind).is_some_and(|name| {
                    self.types
                        .is_java_subclass(Types::boxed_class(actual).unwrap(), &name)
                }),
                _ => false,
            };
        }
        self.require(&Expr::Null(0), actual, expected).is_ok()
    }

    /// Whether one of the bounds of type parameter `actual` fits `expected`.
    pub(super) fn bound_fits(&self, actual: Kind, expected: Kind) -> bool {
        self.types.all_bounds(actual).into_iter().any(|bound| {
            let view = if matches!(actual, Kind::Nullable(_)) {
                bound.nullable().unwrap_or(bound)
            } else {
                bound
            };
            bound == expected || self.fits(view, expected)
        })
    }

    /// The bound of type parameter `kind` that declares method `name`, or
    /// its first bound when none does.
    pub(super) fn member_bound(&self, kind: Kind, name: &str) -> Option<Kind> {
        let bounds = self.types.all_bounds(kind);
        let first = *bounds.first()?;
        let name = name.split('<').next().unwrap_or(name);
        Some(
            bounds
                .iter()
                .copied()
                .find(|&bound| self.declares_method(bound, name))
                .unwrap_or(first),
        )
    }

    /// Whether values of type `kind` offer a method called `name`.
    fn declares_method(&self, kind: Kind, name: &str) -> bool {
        if self.types.interface_root(kind).is_some() {
            return self.interface_method(kind, name).is_some();
        }
        if let Some(class) = self.types.native_class(kind) {
            return self.find_method(class, name).is_some();
        }
        match self.types.runtime_class_name(kind) {
            Some(class) => !self.types.java_methods(&class, name, false).is_empty(),
            None => false,
        }
    }

    /// Checks inferred or written type arguments against their bounds.
    pub(super) fn check_bounds(
        &self,
        parameters: &TypeParameters<'_>,
        map: &HashMap<Kind, Kind>,
        name: Name<'_>,
    ) -> Result<(), Error> {
        for &(_, parameter) in parameters {
            let Some(&actual) = map.get(&parameter) else {
                continue;
            };
            if self.types.is_star(actual) {
                continue;
            }
            for bound in self.types.all_bounds(parameter) {
                let bound = self.types.replace_parameters(bound, map);
                if !self.fits(actual, bound) {
                    return Err(at(name, UNSATISFIED));
                }
            }
        }
        Ok(())
    }

    /// Checks the arguments of one parameterized type against the bounds of
    /// its class or interface.
    pub(super) fn check_application(&self, kind: Kind, name: Name<'_>) -> Result<(), Error> {
        if let Kind::Java(index) = kind {
            let owner = self.types.java_name(index);
            let arguments = self.types.type_arguments(kind);
            let bounds = self
                .types
                .java_class(&owner)
                .and_then(|info| {
                    info.signature
                        .as_deref()
                        .and_then(super::generics::parse_class_bounds)
                })
                .unwrap_or_default();
            if arguments.len() != bounds.len() {
                return Err(at(
                    name,
                    "This Java type needs a different number of type arguments.",
                ));
            }
            let map = bounds
                .iter()
                .map(|(variable, _)| variable.clone())
                .zip(arguments.iter().copied())
                .collect();
            for ((_, bounds), actual) in bounds.iter().zip(arguments) {
                if bounds
                    .iter()
                    .any(|bound| !self.java_signature_accepts(bound, actual, &map, false))
                {
                    return Err(at(
                        name,
                        "This Java type argument does not satisfy its declared bounds.",
                    ));
                }
            }
            return Ok(());
        }
        let (Kind::Applied(index) | Kind::AppliedInterface(index)) = kind else {
            return Ok(());
        };
        let application = self.types.application(index);
        let parameters = if application.interface {
            self.types.interface_parameters.borrow()[application.class].clone()
        } else {
            self.types.class_parameters.borrow()[application.class].clone()
        };
        let map = parameters
            .iter()
            .map(|(_, kind)| *kind)
            .zip(application.arguments)
            .collect();
        self.check_bounds(&parameters, &map, name)
    }

    /// Checks every parameterized type recorded so far for `module`.
    pub(super) fn check_pending_bounds(&self, module: Option<&str>) -> Result<(), Error> {
        let pending: Vec<_> = {
            let mut all = self.types.pending_bounds.borrow_mut();
            let (mine, others): (Vec<_>, Vec<_>) =
                all.drain(..).partition(|(_, _, owner)| *owner == module);
            *all = others;
            mine
        };
        for (kind, offset, _) in pending {
            self.check_application(kind, Name { text: "", offset })?;
        }
        Ok(())
    }
}
