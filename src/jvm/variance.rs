//! Declaration-site variance for interface type parameters.
//!
//! `interface Source<out T>` promises that `T` only flows out of the
//! interface, as a method result, so a `Source<Dog>` can be used as a
//! `Source<Animal>`. `interface Sink<in T>` promises that `T` only flows in,
//! as a method parameter, so a `Sink<Animal>` can be used as a `Sink<Dog>`.
//!
//! The JVM erases type arguments, so variance needs no runtime support: a
//! covariant or contravariant view calls the same erased method, and every
//! value that reaches it was checked against the declared types.
use super::*;
use crate::syntax::Variance;

const OUT_IN_PARAMETER: &str =
    "An 'out' type parameter cannot appear in a parameter type. Remove 'out' or change the method.";
const IN_INVARIANT: &str = "A type parameter marked 'out' or 'in' cannot appear inside an invariant type such as MutableList<T>.";
const IN_IN_RESULT: &str =
    "An 'in' type parameter cannot appear in a result type. Remove 'in' or change the method.";

/// Where a type occurs: a positive position produces values, a negative one
/// consumes them, and an invariant one does both.
#[derive(Clone, Copy, PartialEq)]
enum Position {
    Positive,
    Negative,
    Invariant,
}

impl Position {
    fn flip(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
            Self::Invariant => Self::Invariant,
        }
    }

    fn through(self, variance: Variance) -> Self {
        match variance {
            Variance::Out => self,
            Variance::In => self.flip(),
            Variance::Invariant => Self::Invariant,
        }
    }
}

impl<'a> Types<'a> {
    /// The hidden type parameter that stands for `*` at `position` of a
    /// generic class or interface. Values of this type can be read as the
    /// parameter's bound but never written.
    pub(super) fn star(&self, interface: bool, owner: usize, position: usize) -> Kind {
        if let Some(&(.., kind)) = self
            .stars
            .borrow()
            .iter()
            .find(|entry| entry.0 == interface && entry.1 == owner && entry.2 == position)
        {
            return kind;
        }
        let declared = if interface {
            self.interface_parameters.borrow()[owner][position].1
        } else {
            self.class_parameters.borrow()[owner][position].1
        };
        let bounds: Vec<Kind> = self
            .all_bounds(declared)
            .into_iter()
            .filter(|&bound| !self.contains_parameter(bound))
            .collect();
        let index = self.parameters.borrow().len();
        self.parameters.borrow_mut().push("*");
        self.bounds.borrow_mut().push(bounds);
        self.variances.borrow_mut().push(Variance::Invariant);
        let kind = Kind::TypeParameter(index);
        self.stars
            .borrow_mut()
            .push((interface, owner, position, kind));
        kind
    }

    /// The hidden type that `*` stands for in a Java type such as
    /// `ArrayList<*>`. Values read from it are only known to be objects.
    pub(super) fn java_star(&self) -> Kind {
        if let Some(&(.., kind)) = self
            .stars
            .borrow()
            .iter()
            .find(|entry| entry.1 == usize::MAX)
        {
            return kind;
        }
        let index = self.parameters.borrow().len();
        self.parameters.borrow_mut().push("*");
        self.bounds.borrow_mut().push(Vec::new());
        self.variances.borrow_mut().push(Variance::Invariant);
        let kind = Kind::TypeParameter(index);
        self.stars.borrow_mut().push((false, usize::MAX, 0, kind));
        kind
    }

    /// The Java class written in full, such as `java.util.Map.Entry`.
    pub(super) fn qualified_java_class(&self, text: &str) -> Option<Kind> {
        let mut binary = text.replace('.', "/");
        loop {
            if self.java_class(&binary).is_some() {
                return Some(self.java_kind(&binary));
            }
            let split = binary.rfind('/')?;
            binary.replace_range(split..=split, "$");
        }
    }

    /// Whether `kind` is a type parameter of the built-in library.
    pub(super) fn is_library_parameter(&self, kind: Kind) -> bool {
        matches!(kind, Kind::TypeParameter(index) if self.parameters.borrow()[index].starts_with("__"))
    }

    pub(super) fn is_star(&self, kind: Kind) -> bool {
        matches!(kind.non_null(), Kind::TypeParameter(index) if self.parameters.borrow()[index] == "*")
    }

    /// Whether substituting `map` into a parameter type would require a
    /// value of a star-projected type, which no value is known to have.
    pub(super) fn writes_star(&self, kind: Kind, map: &HashMap<Kind, Kind>) -> bool {
        map.iter().any(|(&parameter, &argument)| {
            self.is_star(argument) && {
                let mut probe = HashMap::new();
                probe.insert(parameter, Kind::Any);
                self.replace_parameters(kind, &probe) != kind
            }
        })
    }

    fn variance(&self, kind: Kind) -> Variance {
        match kind {
            Kind::TypeParameter(index) => self.variances.borrow()[index],
            _ => Variance::Invariant,
        }
    }

    /// Checks that each `out` and `in` type parameter of an interface is used
    /// only where its variance allows in one method signature.
    pub(super) fn check_variance(
        &self,
        signature: &Signature<'_>,
        name: Name<'_>,
    ) -> Result<(), Error> {
        for &(_, kind) in &signature.parameters {
            self.check_position(kind, Position::Negative, name)?;
        }
        if let Some(result) = signature.result {
            self.check_position(result, Position::Positive, name)?;
        }
        for &(_, parameter) in &signature.type_parameters {
            for bound in self.all_bounds(parameter) {
                self.check_position(bound, Position::Negative, name)?;
            }
        }
        Ok(())
    }

    /// A field can always be read; a `var` can be written as well.
    pub(super) fn check_field_variance(
        &self,
        kind: Kind,
        mutable: bool,
        name: Name<'_>,
    ) -> Result<(), Error> {
        let position = if mutable {
            Position::Invariant
        } else {
            Position::Positive
        };
        self.check_position(kind, position, name)
    }

    /// A parent type is a promise about everything the child can be used as.
    pub(super) fn check_parent_variance(&self, kind: Kind, name: Name<'_>) -> Result<(), Error> {
        self.check_position(kind, Position::Positive, name)
    }

    fn check_position(&self, kind: Kind, position: Position, name: Name<'_>) -> Result<(), Error> {
        match kind {
            Kind::TypeParameter(_) => match (self.variance(kind), position) {
                (Variance::Out | Variance::In, Position::Invariant) => Err(at(name, IN_INVARIANT)),
                (Variance::Out, Position::Negative) => Err(at(name, OUT_IN_PARAMETER)),
                (Variance::In, Position::Positive) => Err(at(name, IN_IN_RESULT)),
                _ => Ok(()),
            },
            Kind::Nullable(_) => self.check_position(kind.non_null(), position, name),
            Kind::AppliedInterface(index) => {
                let application = self.application(index);
                let parameters = self.interface_parameters.borrow()[application.class].clone();
                for (&(_, parameter), &argument) in parameters.iter().zip(&application.arguments) {
                    let inner = position.through(self.variance(parameter));
                    self.check_position(argument, inner, name)?;
                }
                Ok(())
            }
            Kind::Applied(index) => {
                let application = self.application(index);
                let parameters = self.class_parameters.borrow()[application.class].clone();
                for (&(_, parameter), &argument) in parameters.iter().zip(&application.arguments) {
                    let inner = position.through(self.variance(parameter));
                    self.check_position(argument, inner, name)?;
                }
                Ok(())
            }
            Kind::List(list) | Kind::MutableList(list) => {
                self.check_position(list.element(), Position::Invariant, name)
            }
            Kind::Java(_) => {
                for argument in self.type_arguments(kind) {
                    self.check_position(argument, Position::Invariant, name)?;
                }
                Ok(())
            }
            Kind::Function(index) => {
                let function = self.function_type(index);
                for &parameter in &function.parameters {
                    self.check_position(parameter, position.flip(), name)?;
                }
                match function.result {
                    Some(result) => self.check_position(result, position, name),
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}

impl Symbols<'_> {
    /// Whether interface type `actual` can be used where `expected` is
    /// required, honoring the variance of each type parameter.
    pub(super) fn applied_fits(&self, actual: Kind, expected: Kind) -> bool {
        if self.types.same_kind(actual, expected) {
            return true;
        }
        let (
            Kind::AppliedInterface(left) | Kind::Applied(left),
            Kind::AppliedInterface(right) | Kind::Applied(right),
        ) = (actual.non_null(), expected.non_null())
        else {
            return false;
        };
        let (left, right) = (self.types.application(left), self.types.application(right));
        if left.class != right.class || left.interface != right.interface {
            return false;
        }
        let parameters = if left.interface {
            self.types.interface_parameters.borrow()[left.class].clone()
        } else {
            self.types.class_parameters.borrow()[left.class].clone()
        };
        parameters
            .iter()
            .zip(left.arguments.iter().zip(&right.arguments))
            .all(|(&(_, parameter), (&actual, &expected))| {
                if self.types.is_star(expected) {
                    // Every argument fits a star projection.
                    return true;
                }
                match self.types.variance(parameter) {
                    Variance::Invariant => self.types.same_kind(actual, expected),
                    Variance::Out => self.argument_fits(actual, expected),
                    Variance::In => self.argument_fits(expected, actual),
                }
            })
    }

    /// Whether type argument `actual` is a subtype of type argument
    /// `expected`. Arguments are boxed at runtime, so `Int` fits `Any` and
    /// `Int?`, but never another primitive such as `Long`.
    fn argument_fits(&self, actual: Kind, expected: Kind) -> bool {
        if self.types.same_kind(actual, expected) {
            return true;
        }
        if matches!(expected, Kind::Nullable(_))
            && self.argument_fits(actual.non_null(), expected.non_null())
        {
            return true;
        }
        if !expected.is_reference() || actual == Kind::Null {
            return false;
        }
        if matches!(actual, Kind::Nullable(_)) && !matches!(expected, Kind::Nullable(_)) {
            return false;
        }
        self.fits(actual, expected)
    }
}
