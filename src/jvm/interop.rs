//! Calls into Java classes found on the class path, without declarations.
//!
//! Members are looked up in the compiled class files. Overloads are chosen
//! like Java does: the candidate whose parameters need the cheapest
//! conversions (identity, then widening, then subtyping, then boxing) wins.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::classfile::{
    ACC_ABSTRACT, ACC_BRIDGE, ACC_INTERFACE, ACC_PROTECTED, ACC_PUBLIC, ACC_STATIC, ACC_SYNTHETIC,
    ACC_VARARGS, ClassInfo,
};

use super::{Emitter, Error, Expr, Kind, ListKind, Local, Method, Name, Types, at, instruction};

#[derive(Clone, PartialEq, Debug)]
pub(super) enum JType {
    Int,
    Long,
    Double,
    Float,
    Bool,
    Char,
    Byte,
    Short,
    Void,
    Class(String),
    Array(Box<JType>),
}

impl JType {
    pub(super) fn descriptor(&self) -> String {
        match self {
            Self::Int => "I".into(),
            Self::Long => "J".into(),
            Self::Double => "D".into(),
            Self::Float => "F".into(),
            Self::Bool => "Z".into(),
            Self::Char => "C".into(),
            Self::Byte => "B".into(),
            Self::Short => "S".into(),
            Self::Void => "V".into(),
            Self::Class(name) => format!("L{name};"),
            Self::Array(inner) => format!("[{}", inner.descriptor()),
        }
    }

    fn is_reference(&self) -> bool {
        matches!(self, Self::Class(_) | Self::Array(_))
    }
}

fn parse_type(chars: &mut std::str::Chars<'_>) -> Option<JType> {
    Some(match chars.next()? {
        'I' => JType::Int,
        'J' => JType::Long,
        'D' => JType::Double,
        'F' => JType::Float,
        'Z' => JType::Bool,
        'C' => JType::Char,
        'B' => JType::Byte,
        'S' => JType::Short,
        'V' => JType::Void,
        'L' => {
            let mut name = String::new();
            for ch in chars.by_ref() {
                if ch == ';' {
                    return Some(JType::Class(name));
                }
                name.push(ch);
            }
            return None;
        }
        '[' => JType::Array(Box::new(parse_type(chars)?)),
        _ => return None,
    })
}

pub(super) fn parse_method(descriptor: &str) -> Option<(Vec<JType>, JType)> {
    let mut chars = descriptor.chars();
    if chars.next()? != '(' {
        return None;
    }
    let mut parameters = Vec::new();
    loop {
        let rest = chars.as_str();
        if let Some(after) = rest.strip_prefix(')') {
            chars = after.chars();
            break;
        }
        parameters.push(parse_type(&mut chars)?);
    }
    Some((parameters, parse_type(&mut chars)?))
}

pub(super) fn parse_field(descriptor: &str) -> Option<JType> {
    parse_type(&mut descriptor.chars())
}

#[derive(Clone)]
pub(super) struct JavaMethod {
    pub name: String,
    pub descriptor: String,
    pub access: u16,
    pub parameters: Vec<JType>,
    pub result: JType,
    /// The class that declares the method.
    pub owner: String,
    /// The generic signature, if the method has one.
    pub signature: Option<String>,
}

impl JavaMethod {
    fn is_varargs(&self) -> bool {
        self.access & ACC_VARARGS != 0
    }
}

impl Types<'_> {
    pub(super) fn java_class(&self, binary: &str) -> Option<Rc<ClassInfo>> {
        self.classpath().load(binary)
    }

    pub(super) fn is_java_interface(&self, binary: &str) -> bool {
        self.java_class(binary)
            .is_some_and(|info| info.is_interface())
    }

    /// The class and every superclass and superinterface, nearest first.
    fn java_hierarchy(&self, binary: &str) -> Vec<Rc<ClassInfo>> {
        let mut seen = HashSet::new();
        let mut queue = std::collections::VecDeque::from([binary.to_owned()]);
        let mut found = Vec::new();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(info) = self.java_class(&name) else {
                continue;
            };
            if let Some(parent) = &info.super_name {
                queue.push_back(parent.clone());
            }
            queue.extend(info.interfaces.iter().cloned());
            found.push(info);
        }
        if found.first().is_some_and(|info| info.is_interface())
            && let Some(object) = self.java_class("java/lang/Object")
        {
            found.push(object);
        }
        found
    }

    pub(super) fn is_java_subclass(&self, sub: &str, sup: &str) -> bool {
        sub == sup
            || sup == "java/lang/Object"
            || self.java_hierarchy(sub).iter().any(|info| info.name == sup)
    }

    /// Public methods named `name`, including inherited ones, most derived first.
    /// Static methods come from the class and its superclasses, since Java
    /// does not inherit an interface's static methods.
    pub(super) fn java_methods(&self, binary: &str, name: &str, statics: bool) -> Vec<JavaMethod> {
        self.java_methods_visible(binary, name, statics, false)
    }

    /// Like `java_methods`, also including protected methods when
    /// `protected` (for code in a subclass).
    pub(super) fn java_methods_visible(
        &self,
        binary: &str,
        name: &str,
        statics: bool,
        protected: bool,
    ) -> Vec<JavaMethod> {
        let mut methods: Vec<JavaMethod> = Vec::new();
        let hierarchy = if statics {
            let mut chain = Vec::new();
            let mut current = self.java_class(binary);
            while let Some(info) = current {
                current = info
                    .super_name
                    .as_deref()
                    .and_then(|parent| self.java_class(parent));
                let interface = info.access & ACC_INTERFACE != 0;
                chain.push(info);
                if interface {
                    break;
                }
            }
            chain
        } else {
            self.java_hierarchy(binary)
        };
        let visible = if protected {
            ACC_PUBLIC | ACC_PROTECTED
        } else {
            ACC_PUBLIC
        };
        for info in hierarchy {
            for member in &info.methods {
                if member.name != name
                    || member.access & visible == 0
                    || member.access & (ACC_BRIDGE | ACC_SYNTHETIC) != 0
                    || (member.access & ACC_STATIC != 0) != statics
                    || methods
                        .iter()
                        .any(|method| method.descriptor == member.descriptor)
                {
                    continue;
                }
                let Some((parameters, result)) = parse_method(&member.descriptor) else {
                    continue;
                };
                methods.push(JavaMethod {
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    access: member.access,
                    parameters,
                    result,
                    owner: info.name.clone(),
                    signature: member.signature.clone(),
                });
            }
        }
        methods
    }

    /// Constructors a subclass can call: public and protected ones, even on
    /// an abstract class.
    pub(super) fn java_super_constructors(&self, binary: &str) -> Vec<JavaMethod> {
        let Some(info) = self.java_class(binary) else {
            return Vec::new();
        };
        info.methods
            .iter()
            .filter(|member| {
                member.name == "<init>" && member.access & (ACC_PUBLIC | ACC_PROTECTED) != 0
            })
            .filter_map(|member| {
                let (parameters, result) = parse_method(&member.descriptor)?;
                Some(JavaMethod {
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    access: member.access,
                    parameters,
                    result,
                    owner: info.name.clone(),
                    signature: member.signature.clone(),
                })
            })
            .collect()
    }

    /// Instance methods named `name` that a subclass can call or override:
    /// public and protected ones that are not static.
    pub(super) fn java_inherited_methods(&self, binary: &str, name: &str) -> Vec<JavaMethod> {
        let mut methods: Vec<JavaMethod> = Vec::new();
        for info in self.java_hierarchy(binary) {
            for member in &info.methods {
                if member.name != name
                    || member.access & (ACC_PUBLIC | ACC_PROTECTED) == 0
                    || member.access & (ACC_BRIDGE | ACC_SYNTHETIC | ACC_STATIC) != 0
                    || methods
                        .iter()
                        .any(|method| method.descriptor == member.descriptor)
                {
                    continue;
                }
                let Some((parameters, result)) = parse_method(&member.descriptor) else {
                    continue;
                };
                methods.push(JavaMethod {
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    access: member.access,
                    parameters,
                    result,
                    owner: info.name.clone(),
                    signature: member.signature.clone(),
                });
            }
        }
        methods
    }

    pub(super) fn java_constructors(&self, binary: &str) -> Vec<JavaMethod> {
        let Some(info) = self.java_class(binary) else {
            return Vec::new();
        };
        if info.access & ACC_ABSTRACT != 0 {
            return Vec::new();
        }
        info.methods
            .iter()
            .filter(|member| member.name == "<init>" && member.access & ACC_PUBLIC != 0)
            .filter_map(|member| {
                let (parameters, result) = parse_method(&member.descriptor)?;
                Some(JavaMethod {
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    access: member.access,
                    parameters,
                    result,
                    owner: info.name.clone(),
                    signature: member.signature.clone(),
                })
            })
            .collect()
    }

    /// A public field named `name` and its type.
    pub(super) fn java_field(&self, binary: &str, name: &str, statics: bool) -> Option<JType> {
        self.java_hierarchy(binary).iter().find_map(|info| {
            info.fields
                .iter()
                .find(|field| {
                    field.name == name
                        && field.access & ACC_PUBLIC != 0
                        && (field.access & ACC_STATIC != 0) == statics
                })
                .and_then(|field| parse_field(&field.descriptor))
        })
    }

    /// The declaring class, access flags and erased type of public field
    /// `name` (static or not) in `binary` or its supertypes.
    pub(super) fn java_public_field(
        &self,
        binary: &str,
        name: &str,
        statics: bool,
    ) -> Option<(String, u16, JType)> {
        self.java_hierarchy(binary).iter().find_map(|info| {
            let field = info.fields.iter().find(|field| {
                field.name == name
                    && field.access & ACC_PUBLIC != 0
                    && (field.access & ACC_STATIC != 0) == statics
            })?;
            Some((
                info.name.clone(),
                field.access,
                parse_field(&field.descriptor)?,
            ))
        })
    }

    /// The declaring class and erased type of static field `name` found in
    /// `binary` or its supertypes, public or (when `protected`) protected.
    pub(super) fn java_static_field_owner(
        &self,
        binary: &str,
        name: &str,
        protected: bool,
    ) -> Option<(String, JType)> {
        let visible = if protected {
            ACC_PUBLIC | ACC_PROTECTED
        } else {
            ACC_PUBLIC
        };
        self.java_hierarchy(binary).iter().find_map(|info| {
            let field = info.fields.iter().find(|field| {
                field.name == name && field.access & visible != 0 && field.access & ACC_STATIC != 0
            })?;
            Some((info.name.clone(), parse_field(&field.descriptor)?))
        })
    }

    /// The declaring class and generic type of public field `name`, when the
    /// field has a generic signature.
    pub(super) fn java_field_generic_type(
        &self,
        binary: &str,
        name: &str,
        statics: bool,
    ) -> Option<(String, super::generics::GType)> {
        self.java_hierarchy(binary).iter().find_map(|info| {
            // Callers have already checked that the field is visible.
            let field = info.fields.iter().find(|field| {
                field.name == name
                    && field.access & (ACC_PUBLIC | ACC_PROTECTED) != 0
                    && (field.access & ACC_STATIC != 0) == statics
            })?;
            let gtype = super::generics::parse_field_signature(field.signature.as_deref()?)?;
            Some((info.name.clone(), gtype))
        })
    }

    /// The abstract methods a class must provide to implement this interface.
    pub(super) fn java_abstract_methods(&self, binary: &str) -> Vec<JavaMethod> {
        let mut methods: Vec<JavaMethod> = Vec::new();
        let hierarchy = self.java_hierarchy(binary);
        // Any concrete method in the hierarchy implements an abstract one
        // with the same name and descriptor.
        let concrete: HashSet<(&str, &str)> = hierarchy
            .iter()
            .flat_map(|info| info.methods.iter())
            .filter(|member| member.access & (ACC_ABSTRACT | ACC_STATIC) == 0)
            .map(|member| (member.name.as_str(), member.descriptor.as_str()))
            .collect();
        for info in &hierarchy {
            for member in &info.methods {
                if member.access & ACC_STATIC != 0
                    || member.name.starts_with('<')
                    || member.access & ACC_ABSTRACT == 0
                {
                    continue;
                }
                let key = (member.name.as_str(), member.descriptor.as_str());
                if concrete.contains(&key)
                    || methods.iter().any(|method| {
                        method.name == member.name && method.descriptor == member.descriptor
                    })
                {
                    continue;
                }
                // Interfaces may redeclare Object's public methods abstractly.
                if matches!(
                    (member.name.as_str(), member.descriptor.as_str()),
                    ("equals", "(Ljava/lang/Object;)Z")
                        | ("hashCode", "()I")
                        | ("toString", "()Ljava/lang/String;")
                ) {
                    continue;
                }
                let Some((parameters, result)) = parse_method(&member.descriptor) else {
                    continue;
                };
                methods.push(JavaMethod {
                    name: member.name.clone(),
                    descriptor: member.descriptor.clone(),
                    access: member.access,
                    parameters,
                    result,
                    owner: info.name.clone(),
                    signature: member.signature.clone(),
                });
            }
        }
        methods
    }

    /// The Mammoth type for a value of a Java type.
    pub(super) fn kind_of(&self, jtype: &JType) -> Option<Kind> {
        Some(match jtype {
            JType::Int | JType::Byte | JType::Short => Kind::Int,
            JType::Long => Kind::Long,
            JType::Double | JType::Float => Kind::Double,
            JType::Bool => Kind::Bool,
            JType::Char => Kind::Char,
            JType::Void => return None,
            JType::Class(name) => self.java_kind(name),
            JType::Array(inner) => {
                let element = match inner.as_ref() {
                    JType::Int => Some(ListKind::Int),
                    JType::Long => Some(ListKind::Long),
                    JType::Double => Some(ListKind::Double),
                    JType::Bool => Some(ListKind::Bool),
                    JType::Char => Some(ListKind::Char),
                    JType::Class(name) => ListKind::for_element(self.java_kind(name)),
                    _ => None,
                };
                match element {
                    Some(element) => Kind::List(element),
                    // Arrays Mammoth cannot index yet are passed around as Java values.
                    None => Kind::Java(self.java_index(&jtype.descriptor())),
                }
            }
        })
    }

    /// The internal class name of a boxed primitive, if `kind` is primitive.
    pub(super) fn boxed_class(kind: Kind) -> Option<&'static str> {
        Some(match kind {
            Kind::Int => "java/lang/Integer",
            Kind::Long => "java/lang/Long",
            Kind::Double => "java/lang/Double",
            Kind::Bool => "java/lang/Boolean",
            Kind::Char => "java/lang/Character",
            _ => return None,
        })
    }

    /// The internal class name a Mammoth reference value has on the JVM.
    pub(super) fn runtime_class_name(&self, kind: Kind) -> Option<String> {
        if kind.is_nullable_primitive() {
            return Self::boxed_class(kind.non_null()).map(str::to_owned);
        }
        Some(match kind.non_null() {
            Kind::Text => "java/lang/String".into(),
            Kind::Any | Kind::TypeParameter(_) => "java/lang/Object".into(),
            Kind::Java(index) => self.java_name(index),
            Kind::MutableList(_) => "java/util/ArrayList".into(),
            Kind::Function(index) => self.function_type(index).interface()?.0.into(),
            _ => return None,
        })
    }

    /// How costly it is to pass a value of `kind` where Java expects `target`,
    /// or `None` if it is not allowed.
    pub(super) fn conversion_cost(&self, kind: Kind, target: &JType) -> Option<u8> {
        use JType as J;
        let boxing = |kind: Kind| -> Option<u8> {
            let J::Class(name) = target else { return None };
            let boxed = Self::boxed_class(kind)?;
            if name == boxed {
                Some(4)
            } else if self.is_java_subclass(boxed, name) {
                Some(5)
            } else {
                None
            }
        };
        match kind {
            Kind::Int => match target {
                J::Int => Some(0),
                J::Long => Some(1),
                J::Float | J::Double => Some(2),
                _ => boxing(kind),
            },
            Kind::Long => match target {
                J::Long => Some(0),
                J::Float | J::Double => Some(2),
                _ => boxing(kind),
            },
            Kind::Double => match target {
                J::Double => Some(0),
                // Mammoth has no Float type, so a Double also fits a Java
                // `float` parameter (rounded to float), after any `double`
                // overload. Minecraft APIs use float for hardness, volume, etc.
                J::Float => Some(3),
                _ => boxing(kind),
            },
            Kind::Char => match target {
                J::Char => Some(0),
                J::Int => Some(1),
                J::Long | J::Float | J::Double => Some(2),
                _ => boxing(kind),
            },
            Kind::Bool => match target {
                J::Bool => Some(0),
                _ => boxing(kind),
            },
            Kind::Null => target.is_reference().then_some(1),
            Kind::Nullable(_) if kind.is_nullable_primitive() => {
                let J::Class(expected) = target else {
                    return None;
                };
                let boxed = Self::boxed_class(kind.non_null())?;
                if boxed == expected {
                    Some(0)
                } else {
                    self.is_java_subclass(boxed, expected).then_some(2)
                }
            }
            Kind::Nullable(_) => self.conversion_cost(kind.non_null(), target),
            Kind::List(list) => match target {
                J::Array(inner) => {
                    let element = list.element();
                    match (element, inner.as_ref()) {
                        (Kind::Int, J::Int)
                        | (Kind::Long, J::Long)
                        | (Kind::Double, J::Double)
                        | (Kind::Bool, J::Bool)
                        | (Kind::Char, J::Char) => Some(0),
                        (element, J::Class(name)) if element.is_reference() => {
                            match self.runtime_class_name(element) {
                                Some(actual) if actual == *name => Some(0),
                                Some(actual) if self.is_java_subclass(&actual, name) => Some(2),
                                _ if name == "java/lang/Object" => Some(3),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
                J::Class(name) if name == "java/lang/Object" => Some(3),
                _ => None,
            },
            Kind::Object(_) | Kind::Interface(_) | Kind::Applied(_) | Kind::AppliedInterface(_) => {
                match target {
                    J::Class(name) if name == "java/lang/Object" => Some(3),
                    _ => None,
                }
            }
            Kind::Text
            | Kind::Any
            | Kind::Java(_)
            | Kind::MutableList(_)
            | Kind::Function(_)
            | Kind::TypeParameter(_) => {
                let actual = self.runtime_class_name(kind)?;
                match target {
                    J::Class(name) if *name == actual => Some(0),
                    J::Class(name) if self.is_java_subclass(&actual, name) => {
                        Some(if name == "java/lang/Object" { 3 } else { 2 })
                    }
                    J::Array(_) if target.descriptor() == actual => Some(0),
                    _ => None,
                }
            }
        }
    }
}

/// Picks the best overload for arguments of the given types.
/// An argument as seen by overload resolution.
#[derive(Clone, Copy)]
pub(super) enum Argument {
    Value(Kind),
    /// A lambda, with its parameter count when it lists its parameters.
    Lambda(Option<usize>),
}

/// Arguments whose type can depend on the parameter they are passed to.
fn context_sensitive(arg: &Expr<'_>) -> bool {
    matches!(
        arg,
        Expr::Call(..)
            | Expr::MethodCall(..)
            | Expr::List(..)
            | Expr::When(..)
            | Expr::Elvis(..)
            | Expr::Safe(..)
    )
}

fn overload_score(
    candidate: &JavaMethod,
    kinds: &[Argument],
    cost: &dyn Fn(Argument, &JType) -> Option<u8>,
) -> Option<(u32, bool)> {
    let parameters = &candidate.parameters;
    let mut score = None;
    if parameters.len() == kinds.len() {
        score = kinds
            .iter()
            .zip(parameters)
            .map(|(&argument, target)| cost(argument, target).map(u32::from))
            .sum::<Option<u32>>()
            .map(|score| (score, false));
    }
    if score.is_none()
        && candidate.is_varargs()
        && kinds.len() + 1 >= parameters.len()
        && let Some((JType::Array(component), fixed)) = parameters.split_last()
    {
        let fixed_cost = kinds
            .iter()
            .zip(fixed)
            .map(|(&argument, target)| cost(argument, target).map(u32::from))
            .sum::<Option<u32>>();
        let rest_cost = kinds[fixed.len()..]
            .iter()
            .map(|&argument| cost(argument, component).map(u32::from))
            .sum::<Option<u32>>();
        if let (Some(fixed_cost), Some(rest_cost)) = (fixed_cost, rest_cost) {
            score = Some((fixed_cost + rest_cost + 20, true));
        }
    }
    score
}

pub(super) fn select<'m>(
    candidates: &'m [JavaMethod],
    kinds: &[Argument],
    cost: &dyn Fn(Argument, &JType) -> Option<u8>,
) -> Option<(&'m JavaMethod, bool)> {
    candidates
        .iter()
        .filter_map(|candidate| {
            overload_score(candidate, kinds, cost).map(|(score, spread)| (candidate, spread, score))
        })
        .min_by_key(|(_, _, score)| *score)
        .map(|(candidate, spread, _)| (candidate, spread))
}

impl<'a> Emitter<'a> {
    /// Cost of passing a Mammoth object to a Java parameter it implements.
    fn object_cost(&self, kind: Kind, target: &JType) -> Option<u8> {
        let (Some(class), JType::Class(name)) =
            (self.symbols.types.native_class(kind.non_null()), target)
        else {
            return None;
        };
        let index = self.symbols.types.java_index(name);
        self.symbols
            .object_is(class, Kind::Java(index))
            .then_some(1)
    }

    /// Evaluates `args` once to learn their types, then discards the code.
    fn probe_kinds(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
        hints: &[Option<Kind>],
    ) -> Result<Vec<Argument>, Error> {
        let saved = method.snapshot();
        let mut kinds = Vec::with_capacity(args.len());
        let mut result = Ok(());
        for (position, arg) in args.iter().enumerate() {
            if let Expr::Lambda(lambda) = arg {
                let arity = (lambda.explicit || !lambda.parameters.is_empty())
                    .then_some(lambda.parameters.len());
                kinds.push(Argument::Lambda(arity));
                // Something must stand in for the argument on the stack.
                method.stack.push(Local::Null);
                continue;
            }
            let at = method.stack.len() as u16;
            match self.expression_as(method, arg, at, hints.get(position).copied().flatten()) {
                Ok(kind) => {
                    if kind != Kind::Null {
                        let local = self.local_type(kind);
                        method.push_stack(local);
                    } else {
                        method.stack.push(Local::Null);
                    }
                    kinds.push(Argument::Value(kind));
                }
                Err(error) => {
                    result = Err(error);
                    break;
                }
            }
        }
        method.restore(saved);
        result.map(|()| kinds)
    }

    fn java_probe_hints(
        &self,
        candidate: &JavaMethod,
        receiver: Option<Kind>,
        expected: Option<Kind>,
        count: usize,
        spread: Option<bool>,
    ) -> Vec<Option<Kind>> {
        let Some(signature) = candidate
            .signature
            .as_deref()
            .and_then(super::generics::parse_method_signature)
        else {
            return Vec::new();
        };
        let types = &self.symbols.types;
        let mut map = receiver
            .map(|kind| types.substitution(kind, &candidate.owner))
            .unwrap_or_default();
        for variable in &signature.variables {
            map.remove(variable);
        }
        let mut constraints = std::collections::HashMap::new();
        if let Some(expected) = expected {
            self.symbols.collect_java_result_constraints(
                &signature.result,
                expected.non_null(),
                &signature.variables,
                &mut constraints,
            );
        }
        if !self
            .symbols
            .solve_java_constraints(&constraints, &mut map, None)
            || !self
                .symbols
                .propagate_java_bounds(&signature, &constraints, &mut map)
        {
            return Vec::new();
        }
        // Before probing, a varargs call of exact array arity is ambiguous;
        // avoid guessing whether its final argument is an array or an element.
        // Once `spread` is known, the tail elements get the component type.
        let fixed = if candidate.is_varargs() {
            candidate.parameters.len().saturating_sub(1)
        } else {
            candidate.parameters.len()
        };
        (0..count)
            .map(|index| {
                let formal = match spread {
                    Some(spread) => signature.argument(index, spread),
                    None if index < fixed => signature.parameters.get(index),
                    None => None,
                };
                formal.and_then(|formal| types.generic_kind(formal, &map))
            })
            .collect()
    }

    /// Argument types for one overload candidate. Nested calls, list
    /// literals and conditionals are probed again with the parameter types
    /// this candidate gives them, so a nested generic factory such as
    /// `emptyList()` can infer its type arguments from the overload being
    /// tried. Only refinements with the original erasure are kept, so
    /// applicability and conversion costs never change. `Ok(None)` when
    /// nothing was refined; an error means the candidate does not fit.
    #[allow(clippy::too_many_arguments)]
    fn candidate_kinds(
        &mut self,
        method: &mut Method<'a>,
        candidate: &JavaMethod,
        args: &'a [Expr<'a>],
        kinds: &[Argument],
        hints: &[Option<Kind>],
        receiver: Option<Kind>,
        expected: Option<Kind>,
        spread: bool,
    ) -> Result<Option<Vec<Argument>>, Error> {
        // Nested overloaded calls re-probe their own arguments; bound the
        // speculation so deeply nested overloads stay linear.
        if self.candidate_depth >= 4 || !args.iter().any(context_sensitive) {
            return Ok(None);
        }
        let refined_hints =
            self.java_probe_hints(candidate, receiver, expected, args.len(), Some(spread));
        let positions: Vec<usize> = (0..args.len())
            .filter(|&position| {
                // Only a parameterized type can tell a nested call more than
                // its erasure, which the first probe already had.
                let hint = refined_hints.get(position).copied().flatten();
                hint.is_some_and(|hint| {
                    matches!(
                        hint.non_null(),
                        Kind::Function(_) | Kind::List(_) | Kind::MutableList(_)
                    ) || !self
                        .symbols
                        .types
                        .type_arguments(hint.non_null())
                        .is_empty()
                }) && hint != hints.get(position).copied().flatten()
                    && context_sensitive(&args[position])
                    && matches!(kinds[position], Argument::Value(kind) if kind != Kind::Null)
            })
            .collect();
        if positions.is_empty() {
            return Ok(None);
        }
        let saved = method.snapshot();
        let saved_result = self.last_generic_result;
        self.candidate_depth += 1;
        let mut refined = kinds.to_vec();
        let mut result = Ok(());
        for (position, arg) in args.iter().enumerate() {
            let Argument::Value(known) = kinds[position] else {
                method.stack.push(Local::Null);
                continue;
            };
            if positions.contains(&position) {
                let at = method.stack.len() as u16;
                match self.expression_as(method, arg, at, refined_hints[position]) {
                    Ok(kind) => {
                        let types = &self.symbols.types;
                        if kind.descriptor(types) == known.descriptor(types) {
                            refined[position] = Argument::Value(kind);
                        }
                    }
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
                method.stack.truncate(at as usize);
            }
            if known != Kind::Null {
                let local = self.local_type(known);
                method.push_stack(local);
            } else {
                method.stack.push(Local::Null);
            }
        }
        self.candidate_depth -= 1;
        self.last_generic_result = saved_result;
        method.restore(saved);
        result.map(|()| Some(refined))
    }

    /// Chooses an overload, then emits the arguments converted to its parameters.
    pub(super) fn java_arguments(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        candidates: &[JavaMethod],
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<JavaMethod, Error> {
        self.java_arguments_expected(method, name, candidates, args, base, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn java_arguments_expected(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        candidates: &[JavaMethod],
        args: &'a [Expr<'a>],
        base: u16,
        result_expected: Option<Kind>,
    ) -> Result<JavaMethod, Error> {
        // Capture this call's receiver before probing nested argument calls.
        // Its result type is installed only after those arguments are emitted.
        let receiver = self.generic_receiver.take();
        self.last_generic_result = None;
        if candidates.is_empty() {
            return Err(at(
                name,
                "This Java class has no public member with that name.",
            ));
        }
        let arity: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.parameters.len() == args.len() || candidate.is_varargs())
            .collect();
        if arity.is_empty() {
            return Err(at(
                name,
                "No version of this Java method takes that many arguments.",
            ));
        }
        // Only a unique arity-compatible signature can supply argument hints
        // before overload selection. The selected call is still fully checked.
        let hints = if arity.len() == 1 {
            self.java_probe_hints(arity[0], receiver, result_expected, args.len(), None)
        } else {
            Vec::new()
        };
        let kinds = self.probe_kinds(method, args, &hints)?;
        let applicable: Vec<_> = candidates
            .iter()
            .filter_map(|candidate| {
                select(
                    std::slice::from_ref(candidate),
                    &kinds,
                    &|argument, target| self.java_argument_cost(argument, target),
                )
                .map(|(_, spread)| (candidate, spread))
            })
            .collect();
        if applicable.is_empty() {
            return Err(at(
                name,
                "No version of this Java method accepts these argument types.",
            ));
        }
        let mut eligible = Vec::new();
        let mut eligible_kinds = Vec::new();
        let mut first_error = None;
        let mut first_body_error = None;
        for (candidate, spread) in &applicable {
            let refined = match self.candidate_kinds(
                method,
                candidate,
                args,
                &kinds,
                &hints,
                receiver,
                result_expected,
                *spread,
            ) {
                Ok(refined) => refined,
                Err(error) if applicable.len() == 1 => return Err(error),
                Err(error) => {
                    first_error.get_or_insert(error);
                    continue;
                }
            };
            let candidate_kinds = refined.as_deref().unwrap_or(&kinds);
            if applicable.len() == 1 {
                eligible.push((*candidate).clone());
                eligible_kinds.push(refined);
                continue;
            }
            match self.infer_java_candidate(
                method,
                name,
                candidate,
                args,
                candidate_kinds,
                receiver,
                result_expected,
                *spread,
                false,
            ) {
                Ok(_) => {
                    if args.iter().any(|source| matches!(source, Expr::Lambda(_))) {
                        match self.probe_java_lambda_candidate(
                            method,
                            name,
                            candidate,
                            args,
                            candidate_kinds,
                            receiver,
                            result_expected,
                            *spread,
                        ) {
                            Ok(()) => {
                                eligible.push((*candidate).clone());
                                eligible_kinds.push(refined);
                            }
                            Err(error) => {
                                first_body_error.get_or_insert(error);
                            }
                        }
                    } else {
                        eligible.push((*candidate).clone());
                        eligible_kinds.push(refined);
                    }
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if eligible.is_empty() {
            return Err(first_body_error.or(first_error).unwrap());
        }
        // Refined kinds keep their erasure, so specificity uses the shared ones.
        let (chosen, spread) = self.select_specific_java_overload(&eligible, &kinds, receiver)
            .ok_or_else(|| at(name, "This Java call is ambiguous. Use explicit argument types to choose an overload."))?;
        let position = eligible
            .iter()
            .position(|candidate| std::ptr::eq(candidate, chosen))
            .unwrap();
        let chosen = chosen.clone();
        let chosen_kinds = eligible_kinds.swap_remove(position);
        let kinds = chosen_kinds.unwrap_or(kinds);
        let inferred = self.infer_java_candidate(
            method,
            name,
            &chosen,
            args,
            &kinds,
            receiver,
            result_expected,
            spread,
            true,
        )?;
        let (map, parsed) = (inferred.map, inferred.signature);
        let types = &self.symbols.types;
        let generic_parameters: Vec<Option<Kind>> = chosen
            .parameters
            .iter()
            .enumerate()
            .map(|(position, _)| {
                parsed
                    .as_ref()
                    .and_then(|signature| signature.parameters.get(position))
                    .and_then(|gtype| types.generic_kind(gtype, &map))
            })
            .collect();
        let expected: Vec<Option<Kind>> = chosen
            .parameters
            .iter()
            .zip(&generic_parameters)
            .map(|(jtype, generic)| generic.or_else(|| types.kind_of(jtype)))
            .collect();
        let generic_result = parsed
            .as_ref()
            .and_then(|signature| types.result_kind(&signature.result, &map, self.site(name)));
        let depth = method.stack.len();
        let fixed = if spread {
            chosen.parameters.len() - 1
        } else {
            chosen.parameters.len()
        };
        for (position, (((arg, target), expected), generic)) in args[..fixed]
            .iter()
            .zip(&chosen.parameters)
            .zip(&expected)
            .zip(&generic_parameters)
            .enumerate()
        {
            let kind = self.java_argument(method, arg, target, *expected)?;
            let formal = parsed
                .as_ref()
                .and_then(|signature| signature.parameters.get(position));
            self.check_emitted_argument(arg, kind, *generic, formal, &map, target)?;
        }
        if spread {
            let JType::Array(component) = chosen.parameters.last().unwrap() else {
                unreachable!("varargs end with an array")
            };
            let formal = parsed
                .as_ref()
                .and_then(|signature| signature.argument(fixed, true));
            self.varargs_array(method, &args[fixed..], component, formal, &map)?;
        }
        method.stack.truncate(depth);
        if let Some(result) = self.symbols.types.kind_of(&chosen.result) {
            method.max_stack = method.max_stack.max(base + result.slots());
        }
        self.last_generic_result = generic_result;
        Ok(chosen)
    }

    fn java_argument(
        &mut self,
        method: &mut Method<'a>,
        arg: &'a Expr<'a>,
        target: &JType,
        expected: Option<Kind>,
    ) -> Result<Kind, Error> {
        let at = method.stack.len() as u16;
        let expected = expected.or_else(|| self.symbols.types.kind_of(target));
        let kind = self.expression_as(method, arg, at, expected)?;
        let given = kind;
        self.convert_to_java(method, kind, target, at);
        let local = match target {
            JType::Long => Local::Long,
            JType::Double => Local::Double,
            JType::Float | JType::Int | JType::Bool | JType::Char | JType::Byte | JType::Short => {
                Local::Int
            }
            _ => {
                let descriptor = target.descriptor();
                let class = match target {
                    JType::Class(name) => self.pool_class(name),
                    _ => self.pool_class(&descriptor),
                };
                Local::Object(class)
            }
        };
        method.push_stack(local);
        Ok(given)
    }

    /// Checks an argument against a Java parameter whose generic type was
    /// worked out from type arguments, such as `E` in `ArrayList<Text>.add`.
    /// Java erases these to `Object`, so without this check `add(5)` would
    /// put an Integer into a list of strings.
    /// Checks an emitted argument against its parameter's generic type. A
    /// wildcard parameter is checked against the original signature, since
    /// its Mammoth view (`List<? super Int>` seen as `List<Int>`) would
    /// wrongly demand an exact type argument.
    fn check_emitted_argument(
        &self,
        arg: &Expr<'_>,
        kind: Kind,
        generic: Option<Kind>,
        formal: Option<&super::generics::GType>,
        map: &HashMap<String, Kind>,
        target: &JType,
    ) -> Result<(), Error> {
        match formal {
            Some(formal) if formal.has_wildcard() => {
                if matches!(arg, Expr::Lambda(_))
                    || self
                        .symbols
                        .java_signature_accepts(formal, kind, map, false)
                {
                    Ok(())
                } else {
                    Err(self.argument_mismatch(arg.offset(), generic))
                }
            }
            _ => match generic {
                Some(generic) => self.check_type_argument(arg, kind, generic, target),
                None => Ok(()),
            },
        }
    }

    /// The error for an argument that does not fit its generic parameter.
    /// Writing into a `? extends` capture gets its own explanation.
    fn argument_mismatch(&self, offset: usize, expected: Option<Kind>) -> Error {
        let types = &self.symbols.types;
        let capture = expected.is_some_and(|kind| {
            matches!(kind.non_null(), Kind::TypeParameter(_))
                && types.has_capture(kind)
                && types.capture_lower(kind).is_none()
        });
        Error {
            offset,
            message: if capture {
                "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it. Copy it first, such as ArrayList<Number>(value)."
            } else {
                "This argument does not match the Java type's type argument, such as a number added to a list of Text."
            },
        }
    }

    fn check_type_argument(
        &self,
        arg: &Expr<'_>,
        kind: Kind,
        expected: Kind,
        target: &JType,
    ) -> Result<(), Error> {
        let types = &self.symbols.types;
        // Only erased reference parameters need the check; a parameter that is
        // already a concrete Java class is checked by overload selection.
        let erased = match target {
            JType::Class(name) => name.as_str(),
            _ => return Ok(()),
        };
        if matches!(arg, Expr::Lambda(_))
            || matches!(expected.non_null(), Kind::Any)
            || types
                .runtime_class_name(expected.non_null())
                .is_some_and(|name| name == erased)
        {
            return Ok(());
        }
        let accepts = kind == Kind::Null
            || types.same_kind(kind.non_null(), expected.non_null())
            || self
                .symbols
                .fits(kind, expected.nullable().unwrap_or(expected))
            || self.symbols.fits(kind.non_null(), expected.non_null());
        if accepts {
            return Ok(());
        }
        Err(self.argument_mismatch(arg.offset(), Some(expected)))
    }

    pub(super) fn pool_class(&mut self, name: &str) -> u16 {
        let kind = self.symbols.types.java_kind(name);
        if kind == Kind::Text {
            return self.refs.string;
        }
        self.object_class(kind)
    }

    fn varargs_array(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
        component: &JType,
        formal: Option<&super::generics::GType>,
        map: &HashMap<String, Kind>,
    ) -> Result<(), Error> {
        let element = formal.and_then(|formal| self.symbols.types.generic_kind(formal, map));
        let at = method.stack.len() as u16;
        self.push_int32(method, args.len() as i32);
        let array_type = JType::Array(Box::new(component.clone()));
        match component {
            JType::Int => method.code.extend([0xbc, 10]),
            JType::Long => method.code.extend([0xbc, 11]),
            JType::Double => method.code.extend([0xbc, 7]),
            JType::Float => method.code.extend([0xbc, 6]),
            JType::Bool => method.code.extend([0xbc, 4]),
            JType::Char => method.code.extend([0xbc, 5]),
            JType::Byte => method.code.extend([0xbc, 8]),
            JType::Short => method.code.extend([0xbc, 9]),
            JType::Class(name) => {
                let class = self.pool_class(name);
                instruction(&mut method.code, 0xbd, class);
            }
            JType::Array(_) => {
                let class = self.pool_class(&component.descriptor());
                instruction(&mut method.code, 0xbd, class);
            }
            JType::Void => unreachable!("arrays never hold void"),
        }
        let array = Local::Object(self.pool_class(&array_type.descriptor()));
        let store = match component {
            JType::Int => 0x4f,
            JType::Long => 0x50,
            JType::Double => 0x52,
            JType::Float => 0x51,
            JType::Bool | JType::Byte => 0x54,
            JType::Char => 0x55,
            JType::Short => 0x56,
            _ => 0x53,
        };
        for (position, arg) in args.iter().enumerate() {
            method.code.push(0x59); // dup
            self.push_int32(method, position as i32);
            method.stack.extend([array, array, Local::Int]);
            let kind = self.java_argument(method, arg, component, element)?;
            self.check_emitted_argument(arg, kind, element, formal, map, component)?;
            method.pop_stack();
            method.stack.truncate(at as usize);
            method.code.push(store);
        }
        method.max_stack = method.max_stack.max(at + 5);
        Ok(())
    }

    /// Converts the Mammoth value on top of the stack to a Java parameter type.
    pub(super) fn convert_to_java(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        target: &JType,
        at: u16,
    ) {
        use JType as J;
        if self.symbols.types.bound(kind).is_some()
            && let J::Class(name) = target
        {
            // A secondary bound may differ from the type parameter's erasure.
            let class = self.pool_class(name);
            instruction(&mut method.code, 0xc0, class);
        }
        match (kind, target) {
            (Kind::Int | Kind::Char, J::Long) => method.code.push(0x85),
            (Kind::Int | Kind::Char, J::Float) => method.code.push(0x86),
            (Kind::Int | Kind::Char, J::Double) => method.code.push(0x87),
            (Kind::Long, J::Float) => method.code.push(0x89),
            (Kind::Long, J::Double) => method.code.push(0x8a),
            (Kind::Double, J::Float) => method.code.push(0x90), // d2f
            (Kind::Int | Kind::Long | Kind::Double | Kind::Bool | Kind::Char, J::Class(_)) => {
                self.box_value(method, kind)
            }
            _ => {}
        }
        let slots = match target {
            J::Long | J::Double => 2,
            _ => 1,
        };
        method.max_stack = method.max_stack.max(at + slots.max(kind.slots()));
    }

    /// Converts a Java result on top of the stack to its Mammoth type.
    pub(super) fn convert_from_java(
        &mut self,
        method: &mut Method<'a>,
        result: &JType,
        base: u16,
        allow_null: bool,
    ) -> Option<Kind> {
        // A boxed result such as `Integer getAge()` is a Mammoth `Int`
        // (nullable where the destination accepts null). Short, Byte and
        // Float boxes stay Java objects: they unbox through other methods.
        let generic = self.last_generic_result.take().or_else(|| match result {
            JType::Class(name) => super::generics::unboxed(name),
            _ => None,
        });
        if *result == JType::Float {
            method.code.push(0x8d); // f2d
        }
        let erased = self.symbols.types.kind_of(result)?;
        let kind = match generic {
            Some(generic) if allow_null && erased.is_reference() && !generic.is_reference() => {
                let nullable = generic.nullable()?;
                self.cast_top(method, erased, nullable, base);
                nullable
            }
            Some(generic) if generic != erased && erased.is_reference() => {
                self.cast_top(method, erased, generic, base);
                generic
            }
            _ => erased,
        };
        method.max_stack = method.max_stack.max(base + kind.slots());
        if kind.is_reference() {
            if allow_null {
                return kind.nullable().or(Some(kind));
            }
            self.guard_non_null(method, kind, base);
        }
        Some(kind)
    }

    /// Emits a public method with a Java interface method's exact name and
    /// descriptor that converts its arguments and calls the Mammoth method.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn java_bridge(
        &mut self,
        output: &mut Vec<u8>,
        class: usize,
        this_class: u16,
        name: &'a str,
        signature: &super::Signature<'_>,
        required: &JavaMethod,
        code_name: u16,
        stack_map_name: u16,
    ) -> Result<(), Error> {
        let mut bridge = Method::new(vec![Local::Object(this_class)]);
        self.mark_bridge(&mut bridge);
        let mut slots = Vec::new();
        for parameter in &required.parameters {
            let local = match parameter {
                JType::Long => Local::Long,
                JType::Double => Local::Double,
                JType::Class(_) | JType::Array(_) => {
                    let class = self.pool_class(&match parameter {
                        JType::Class(name) => name.clone(),
                        _ => parameter.descriptor(),
                    });
                    Local::Object(class)
                }
                _ => Local::Int,
            };
            slots.push(bridge.local(local));
        }
        bridge.access(0x19, 0);
        let mut depth = 1u16;
        for ((parameter, slot), &(_, kind)) in required
            .parameters
            .iter()
            .zip(&slots)
            .zip(&signature.parameters)
        {
            let load = match parameter {
                JType::Long => 0x16,
                JType::Double => 0x18,
                JType::Float => 0x17,
                JType::Class(_) | JType::Array(_) => 0x19,
                _ => 0x15,
            };
            bridge.access(load, *slot);
            match (parameter, kind) {
                (JType::Float, Kind::Double) => bridge.code.push(0x8d), // f2d
                (JType::Class(name), target) => {
                    let wanted = self.symbols.types.runtime_class_name(target);
                    if target.is_reference() {
                        if wanted.as_deref() != Some(name.as_str()) {
                            let class = self.object_class_or_string(target);
                            instruction(&mut bridge.code, 0xc0, class); // checkcast
                        }
                        if kind == kind.non_null() {
                            self.guard_non_null(&mut bridge, kind, depth);
                        }
                    } else {
                        self.unbox_element(&mut bridge, target, depth);
                    }
                }
                _ => {}
            }
            depth += kind.slots();
        }
        bridge.max_stack = bridge.max_stack.max(depth + 2);
        let target = self.member(Some(Kind::Object(class)), name, &signature.descriptor);
        instruction(&mut bridge.code, 0xb6, target); // invokevirtual
        let opcode = match (&required.result, signature.result) {
            (JType::Void, _) => 0xb1,
            (JType::Float, Some(Kind::Double)) => {
                bridge.code.push(0x90); // d2f
                0xae // freturn
            }
            (JType::Class(_) | JType::Array(_), Some(kind)) => {
                if !kind.is_reference() {
                    self.box_value(&mut bridge, kind);
                }
                0xb0 // areturn
            }
            (JType::Long, _) => 0xad,
            (JType::Double, _) => 0xaf,
            _ => 0xac,
        };
        bridge.code.push(opcode);
        let name_index = self.pool.ascii(&required.name);
        let descriptor = self.pool.ascii(&required.descriptor);
        bridge.write(
            output,
            0x0001,
            name_index,
            descriptor,
            code_name,
            stack_map_name,
        )
    }

    fn object_class_or_string(&mut self, kind: Kind) -> u16 {
        self.instance_class(kind.non_null())
    }

    /// `Type.member(args)` on a Java class.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn java_static_call(
        &mut self,
        method: &mut Method<'a>,
        class: &str,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let candidates = self.symbols.types.java_methods(class, name.text, true);
        if candidates.is_empty() {
            return Err(at(
                name,
                "This Java class has no public static method with that name.",
            ));
        }
        self.java_static_invoke(method, &candidates, name, args, base, expected)
    }

    /// A static method that the current Mammoth class inherits from its Java
    /// superclass, called by its bare name as in Java, such as
    /// `createCuboidShape(...)` in a Minecraft block. Protected methods are
    /// included. `Ok(None)` when there is no such method.
    pub(super) fn java_inherited_static_call(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let Some(root) = self
            .current_class
            .and_then(|class| self.symbols.java_root(class))
        else {
            return Ok(None);
        };
        let candidates = self
            .symbols
            .types
            .java_methods_visible(&root, name.text, true, true);
        if candidates.is_empty() {
            return Ok(None);
        }
        self.java_static_invoke(method, &candidates, name, args, base, expected)
            .map(Some)
    }

    fn java_static_invoke(
        &mut self,
        method: &mut Method<'a>,
        candidates: &[JavaMethod],
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let allow_null = std::mem::take(&mut self.allow_null);
        let chosen =
            self.java_arguments_expected(method, name, candidates, args, base, expected)?;
        let class = chosen.owner.clone();
        let interface = self.symbols.types.is_java_interface(&class);
        let owner = self.pool_class(&class);
        let reference = self.pool.member_for(
            if interface { 11 } else { 10 },
            owner,
            &chosen.name,
            &chosen.descriptor,
        );
        instruction(&mut method.code, 0xb8, reference); // invokestatic
        Ok(self.convert_from_java(method, &chosen.result, base, allow_null))
    }

    /// `value.member(args)` where `value` (of type `kind`) is already on the stack.
    pub(super) fn java_instance_call(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
    ) -> Result<Option<Kind>, Error> {
        self.java_instance_call_expected(method, kind, name, args, base, allow_null, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn java_instance_call_expected(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let class = self
            .symbols
            .types
            .runtime_class_name(kind)
            .ok_or_else(|| at(name, "This method needs an object."))?;
        let candidates = self.symbols.types.java_methods(&class, name.text, false);
        if candidates.is_empty() {
            return Err(at(name, "This type has no method with that name."));
        }
        let receiver = self.local_type(kind);
        method.stack.push(receiver);
        self.generic_receiver = Some(kind);
        let chosen =
            self.java_arguments_expected(method, name, &candidates, args, base + 1, expected);
        method.stack.pop();
        let chosen = chosen?;
        let interface = self.symbols.types.is_java_interface(&class);
        let owner = self.pool_class(&class);
        let reference = self.pool.member_for(
            if interface { 11 } else { 10 },
            owner,
            &chosen.name,
            &chosen.descriptor,
        );
        method.max_stack = method.max_stack.max(base + 1);
        if interface {
            let slots: u16 = chosen
                .parameters
                .iter()
                .map(|parameter| match parameter {
                    JType::Long | JType::Double => 2,
                    _ => 1,
                })
                .sum();
            instruction(&mut method.code, 0xb9, reference); // invokeinterface
            method.code.extend([(slots + 1) as u8, 0]);
        } else {
            instruction(&mut method.code, 0xb6, reference); // invokevirtual
        }
        Ok(self.convert_from_java(method, &chosen.result, base, allow_null))
    }

    /// Calls a method a Mammoth class inherits from its Java superclass, on
    /// a receiver (an object of `class`) already on the stack.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn java_inherited_call(
        &mut self,
        method: &mut Method<'a>,
        receiver_kind: Kind,
        class: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let root = self
            .symbols
            .java_root(class)
            .ok_or_else(|| at(name, "This type has no method with that name."))?;
        let own = self.current_class == Some(class);
        let candidates: Vec<JavaMethod> = self
            .symbols
            .types
            .java_inherited_methods(&root, name.text)
            .into_iter()
            .filter(|candidate| own || candidate.access & ACC_PUBLIC != 0)
            .collect();
        if candidates.is_empty() {
            return Err(at(name, "This type has no method with that name."));
        }
        let kind = Kind::Object(class);
        let receiver = self.local_type(kind);
        method.stack.push(receiver);
        self.generic_receiver = self.symbols.java_super_view(receiver_kind);
        let chosen =
            self.java_arguments_expected(method, name, &candidates, args, base + 1, expected);
        method.stack.pop();
        let chosen = chosen?;
        let owner = self.object_class(kind);
        let reference = self
            .pool
            .member_for(10, owner, &chosen.name, &chosen.descriptor);
        method.max_stack = method.max_stack.max(base + 1);
        instruction(&mut method.code, 0xb6, reference); // invokevirtual
        Ok(self.convert_from_java(method, &chosen.result, base, allow_null))
    }

    /// `super.name(args)` when the superclass is a Java class.
    pub(super) fn java_super_call(
        &mut self,
        method: &mut Method<'a>,
        root: &str,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let candidates: Vec<JavaMethod> = self
            .symbols
            .types
            .java_inherited_methods(root, name.text)
            .into_iter()
            .filter(|candidate| candidate.access & ACC_ABSTRACT == 0)
            .collect();
        if candidates.is_empty() {
            return Err(at(name, "The superclass has no method with that name."));
        }
        // Like other Java calls, the result may be null only where the
        // destination accepts null; elsewhere it is checked.
        let allow_null = std::mem::take(&mut self.allow_null);
        method.access(0x19, 0);
        method.stack.push(method.locals[0]);
        self.generic_receiver = self.current_class.and_then(|class| {
            self.symbols
                .java_super_view(self.symbols.types.self_kind(class))
        });
        let chosen =
            self.java_arguments_expected(method, name, &candidates, args, base + 1, expected);
        method.stack.pop();
        let chosen = chosen?;
        let owner = self.pool_class(root);
        let reference = self
            .pool
            .member_for(10, owner, &chosen.name, &chosen.descriptor);
        method.max_stack = method.max_stack.max(base + 1);
        instruction(&mut method.code, 0xb7, reference); // invokespecial
        Ok(self.convert_from_java(method, &chosen.result, base, allow_null))
    }

    /// `Type(args)` for a Java class.
    pub(super) fn java_construct(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Kind, Error> {
        let class = self
            .symbols
            .types
            .runtime_class_name(kind)
            .ok_or_else(|| at(name, "This type cannot be created here."))?;
        let candidates = self.symbols.types.java_constructors(&class);
        if candidates.is_empty() {
            return Err(at(
                name,
                "This Java type has no public constructor. It may be abstract or an interface.",
            ));
        }
        let owner = self.pool_class(&class);
        method.max_stack = method.max_stack.max(base + 2);
        let uninitialized = Local::Uninitialized(method.code.len() as u16);
        instruction(&mut method.code, 0xbb, owner); // new
        method.code.push(0x59); // dup
        method.stack.extend([uninitialized, uninitialized]);
        self.generic_receiver = Some(kind);
        let chosen = self.java_arguments(method, name, &candidates, args, base + 2);
        self.last_generic_result = None;
        method.stack.truncate(base as usize);
        let chosen = chosen?;
        let reference = self
            .pool
            .member_for(10, owner, "<init>", &chosen.descriptor);
        instruction(&mut method.code, 0xb7, reference); // invokespecial
        Ok(kind)
    }

    /// `Type.FIELD` on a Java class.
    pub(super) fn java_static_field(
        &mut self,
        method: &mut Method<'a>,
        class: &str,
        name: Name<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        let allow_null = std::mem::take(&mut self.allow_null);
        let jtype = self
            .symbols
            .types
            .java_field(class, name.text, true)
            .ok_or_else(|| {
                at(
                    name,
                    "This Java class has no public static field with that name.",
                )
            })?;
        let owner = self.pool_class(class);
        let reference = self
            .pool
            .member_for(9, owner, name.text, &jtype.descriptor());
        instruction(&mut method.code, 0xb2, reference); // getstatic
        self.last_generic_result = self.java_field_result(None, class, name);
        self.convert_from_java(method, &jtype, base, allow_null)
            .ok_or_else(|| at(name, "This field has no value."))
    }

    /// A static field that Mammoth class `class` inherits from its Java
    /// superclass or interfaces, such as `MAX_PRIORITY` in a `Thread`
    /// subclass or `FACING` in a Minecraft block. `protected` allows the
    /// superclass's protected fields, for code inside a subclass. `Ok(None)`
    /// when there is no such field.
    pub(super) fn inherited_java_static_field(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        base: u16,
        protected: bool,
    ) -> Result<Option<Kind>, Error> {
        let Some(root) = self.symbols.java_root(class) else {
            return Ok(None);
        };
        let mut owners = vec![root];
        owners.extend(
            self.symbols
                .ancestors(class)
                .flat_map(|ancestor| self.symbols.java_interfaces(ancestor))
                .filter_map(|kind| self.symbols.types.runtime_class_name(kind)),
        );
        let Some((owner, jtype)) = owners.iter().find_map(|binary| {
            self.symbols
                .types
                .java_static_field_owner(binary, name.text, protected)
        }) else {
            return Ok(None);
        };
        let allow_null = std::mem::take(&mut self.allow_null);
        let class_index = self.pool_class(&owner);
        let reference = self
            .pool
            .member_for(9, class_index, name.text, &jtype.descriptor());
        instruction(&mut method.code, 0xb2, reference); // getstatic
        self.last_generic_result = self.java_field_result(None, &owner, name);
        self.convert_from_java(method, &jtype, base, allow_null)
            .ok_or_else(|| at(name, "This field has no value."))
            .map(Some)
    }

    /// `object.field = value` for a public field of a Java object already on
    /// the stack, or `Type.FIELD = value` for a public static field when
    /// `receiver` is `None`. The value is checked against the field's type
    /// arguments, and final fields are rejected.
    pub(super) fn write_java_field(
        &mut self,
        method: &mut Method<'a>,
        receiver: Option<Kind>,
        class: &str,
        name: Name<'a>,
        value: &'a Expr<'a>,
    ) -> Result<(), Error> {
        let statics = receiver.is_none();
        let types = &self.symbols.types;
        let Some((owner, access, jtype)) = types.java_public_field(class, name.text, statics)
        else {
            return Err(at(
                name,
                if statics {
                    "This Java class has no public static field with that name."
                } else {
                    "This Java object has no public field with that name."
                },
            ));
        };
        if access & crate::classfile::ACC_FINAL != 0 {
            return Err(at(
                name,
                "This Java field is final, so it cannot be changed.",
            ));
        }
        let declared = types
            .java_field_generic_type(class, name.text, statics)
            .and_then(|(owner, gtype)| {
                let map = receiver
                    .map(|receiver| types.substitution(receiver, &owner))
                    .unwrap_or_default();
                types.generic_kind(&gtype, &map)
            })
            .or_else(|| types.kind_of(&jtype))
            .ok_or_else(|| at(name, "This field cannot be changed."))?;
        // Java fields of reference type may hold null.
        let expected = if declared.is_reference() {
            declared.nullable().unwrap_or(declared)
        } else {
            declared
        };
        let depth = method.stack.len() as u16;
        if let Some(receiver) = receiver {
            method.push_stack(self.local_type(receiver));
        }
        let at_depth = method.stack.len() as u16;
        let kind = self.expression_as(method, value, at_depth, Some(expected))?;
        self.coerce(method, value, kind, expected, at_depth)?;
        match (&jtype, expected) {
            (JType::Byte, Kind::Int) => method.code.push(0x91), // i2b
            (JType::Short, Kind::Int) => method.code.push(0x93), // i2s
            (JType::Float, Kind::Double) => method.code.push(0x90), // d2f
            _ => self.convert_to_java(method, expected, &jtype, at_depth),
        }
        method.stack.truncate(depth as usize);
        let owner_index = self.pool_class(&owner);
        let reference = self
            .pool
            .member_for(9, owner_index, name.text, &jtype.descriptor());
        let opcode = if statics { 0xb3 } else { 0xb5 }; // putstatic / putfield
        instruction(&mut method.code, opcode, reference);
        Ok(())
    }

    /// `value.field` on a Java object that is already on the stack.
    pub(super) fn java_instance_field(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        let allow_null = std::mem::take(&mut self.allow_null);
        let class = self
            .symbols
            .types
            .runtime_class_name(kind)
            .ok_or_else(|| at(name, "This value has no fields."))?;
        let jtype = self
            .symbols
            .types
            .java_field(&class, name.text, false)
            .ok_or_else(|| at(name, "This Java object has no public field with that name."))?;
        let owner = self.pool_class(&class);
        let reference = self
            .pool
            .member_for(9, owner, name.text, &jtype.descriptor());
        instruction(&mut method.code, 0xb4, reference); // getfield
        self.last_generic_result = self.java_field_result(Some(kind), &class, name);
        self.convert_from_java(method, &jtype, base, allow_null)
            .ok_or_else(|| at(name, "This field has no value."))
    }

    /// The Mammoth type of a generic Java field read through `receiver` (or
    /// statically): `List<Int>` for a `List<Integer>` field, with the
    /// receiver's type arguments substituted and wildcards captured. A raw
    /// receiver leaves the field's type variables raw.
    fn java_field_result(
        &self,
        receiver: Option<Kind>,
        class: &str,
        name: Name<'_>,
    ) -> Option<Kind> {
        let types = &self.symbols.types;
        let (owner, gtype) = types.java_field_generic_type(class, name.text, receiver.is_none())?;
        let map = receiver
            .map(|receiver| types.substitution(receiver, &owner))
            .unwrap_or_default();
        types.result_kind(&gtype, &map, self.site(name))
    }
}

struct JavaInference {
    map: std::collections::HashMap<String, Kind>,
    signature: Option<super::generics::MethodSignature>,
}

impl<'a> Emitter<'a> {
    /// Trial bytecode belongs to cloned emitter/method state. Failed lambda
    /// bodies cannot leave captures, frames, pool entries or helpers behind.
    #[allow(clippy::too_many_arguments)]
    fn probe_java_lambda_candidate(
        &self,
        method: &Method<'a>,
        name: Name<'a>,
        chosen: &JavaMethod,
        args: &'a [Expr<'a>],
        kinds: &[Argument],
        receiver: Option<Kind>,
        expected: Option<Kind>,
        spread: bool,
    ) -> Result<(), Error> {
        let mut trial = self.clone();
        let mut body = method.clone();
        let inferred = trial.infer_java_candidate(
            &mut body, name, chosen, args, kinds, receiver, expected, spread, true,
        )?;
        for (position, source) in args.iter().enumerate() {
            let Expr::Lambda(lambda) = source else {
                continue;
            };
            let formal = inferred
                .signature
                .as_ref()
                .and_then(|signature| signature.argument(position, spread));
            let erased = if spread && position >= chosen.parameters.len() - 1 {
                match chosen.parameters.last() {
                    Some(JType::Array(component)) => Some(component.as_ref()),
                    _ => None,
                }
            } else {
                chosen.parameters.get(position)
            };
            let target = formal
                .and_then(|formal| trial.symbols.types.generic_kind(formal, &inferred.map))
                .or_else(|| erased.and_then(|erased| trial.symbols.types.kind_of(erased)));
            let at = body.stack.len() as u16;
            trial.lambda(&mut body, lambda, target, at)?;
        }
        Ok(())
    }

    fn java_argument_cost(&self, argument: Argument, target: &JType) -> Option<u8> {
        match argument {
            Argument::Value(kind) => self
                .symbols
                .types
                .conversion_cost(kind, target)
                .or_else(|| self.object_cost(kind, target))
                .or_else(|| match target {
                    JType::Class(name)
                        if self.symbols.types.bound(kind).is_some()
                            && self
                                .symbols
                                .fits(kind.non_null(), self.symbols.types.java_kind(name)) =>
                    {
                        Some(2)
                    }
                    _ => None,
                }),
            // An erased `Object` parameter, such as `E` in a list of
            // functions, receives the lambda as its generic type, after
            // every real functional interface.
            Argument::Lambda(arity) => {
                if self.accepts_lambda(target, arity) {
                    Some(1)
                } else {
                    matches!(target, JType::Class(name) if name == "java/lang/Object").then_some(6)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_java_candidate(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        chosen: &JavaMethod,
        args: &'a [Expr<'a>],
        kinds: &[Argument],
        receiver: Option<Kind>,
        result_expected: Option<Kind>,
        spread: bool,
        probe_lambdas: bool,
    ) -> Result<JavaInference, Error> {
        // Work out what the method's type variables stand for, from the
        // receiver's type arguments and from the arguments.
        let types = &self.symbols.types;
        let mut map = receiver
            .map(|receiver| types.substitution(receiver, &chosen.owner))
            .unwrap_or_default();
        let parsed = chosen
            .signature
            .as_deref()
            .and_then(super::generics::parse_method_signature);
        if let Some(signature) = &parsed {
            // A method's <T> shadows a class's T, including at call sites.
            for variable in &signature.variables {
                map.remove(variable);
            }
            let mut constraints = std::collections::HashMap::new();
            for (position, argument) in kinds.iter().enumerate() {
                if let Argument::Value(kind) = argument
                    && let Some(parameter) = signature.argument(position, spread)
                {
                    self.symbols.collect_java_constraints(
                        parameter,
                        *kind,
                        &signature.variables,
                        1,
                        &mut constraints,
                    );
                }
            }
            for (position, source) in args.iter().enumerate() {
                let Expr::Lambda(lambda) = source else {
                    continue;
                };
                let Some(formal) = signature.argument(position, spread) else {
                    continue;
                };
                let Some(sam) = self.symbols.java_lambda_signature(formal) else {
                    continue;
                };
                for (formal, (_, annotation)) in sam.parameters.iter().zip(&lambda.parameters) {
                    if let Some(annotation) = annotation {
                        let declared = super::resolve_type(&self.type_scope, *annotation)?;
                        // The lambda must accept every value of its inferred input.
                        self.symbols.collect_java_constraints(
                            formal,
                            declared,
                            &signature.variables,
                            -1,
                            &mut constraints,
                        );
                    }
                }
            }
            if let Some(expected) = result_expected {
                self.symbols.collect_java_result_constraints(
                    &signature.result,
                    expected.non_null(),
                    &signature.variables,
                    &mut constraints,
                );
            }
            if !self
                .symbols
                .solve_java_constraints(&constraints, &mut map, None)
            {
                return Err(at(
                    name,
                    "This argument does not match the Java type's type argument, such as a number added to a list of Text.",
                ));
            }
            if !self
                .symbols
                .propagate_java_bounds(signature, &constraints, &mut map)
            {
                return Err(at(
                    name,
                    "This inferred Java type argument does not satisfy its declared bounds.",
                ));
            }
            if probe_lambdas {
                let lambda_variables: Vec<_> = signature
                    .variables
                    .iter()
                    .filter(|variable| {
                        !constraints
                            .get(*variable)
                            .is_some_and(|items| items.iter().any(|(_, direction)| *direction == 0))
                    })
                    .cloned()
                    .collect();
                for (position, source) in args.iter().enumerate() {
                    let Expr::Lambda(lambda) = source else {
                        continue;
                    };
                    let Some(formal) = signature.argument(position, spread) else {
                        continue;
                    };
                    let Some(sam) = self.symbols.java_lambda_signature(formal) else {
                        continue;
                    };
                    let result_type = sam.result;
                    if !lambda_variables
                        .iter()
                        .any(|variable| result_type.mentions(variable))
                    {
                        continue;
                    }
                    let Some(target) = types.generic_kind(formal, &map) else {
                        continue;
                    };
                    if let Some(result) = self.probe_java_lambda_result(method, lambda, target)? {
                        self.symbols.collect_java_constraints(
                            &result_type,
                            result,
                            &signature.variables,
                            1,
                            &mut constraints,
                        );
                        if !self
                            .symbols
                            .solve_java_constraints(&constraints, &mut map, None)
                            || !self.symbols.propagate_java_bounds(
                                signature,
                                &constraints,
                                &mut map,
                            )
                        {
                            return Err(at(
                                name,
                                "This inferred Java type argument does not satisfy its declared bounds.",
                            ));
                        }
                    }
                }
            }
        }
        if let Some(signature) = &parsed {
            for (position, (argument, source)) in kinds.iter().zip(args).enumerate() {
                if let Some(formal) = signature.argument(position, spread)
                    && let Argument::Value(kind) = argument
                    && !self
                        .symbols
                        .java_signature_accepts(formal, *kind, &map, false)
                {
                    let expected = self.symbols.types.generic_kind(formal, &map);
                    return Err(self.argument_mismatch(source.offset(), expected));
                }
            }
        }
        if let Some(signature) = &parsed {
            for (variable, bounds) in &signature.bounds {
                if let Some(&actual) = map.get(variable)
                    && bounds.iter().any(|bound| {
                        !self
                            .symbols
                            .java_signature_accepts(bound, actual, &map, false)
                    })
                {
                    return Err(at(
                        name,
                        "This inferred Java type argument does not satisfy its declared bounds.",
                    ));
                }
            }
        }

        for (position, source) in args.iter().enumerate() {
            let Expr::Lambda(lambda) = source else {
                continue;
            };
            let formal = parsed
                .as_ref()
                .and_then(|signature| signature.argument(position, spread));
            let target = if spread && position >= chosen.parameters.len() - 1 {
                match chosen.parameters.last() {
                    Some(JType::Array(component)) => Some(component.as_ref()),
                    _ => None,
                }
            } else {
                chosen.parameters.get(position)
            };
            if let Some(expected) = formal
                .and_then(|formal| types.generic_kind(formal, &map))
                .or_else(|| target.and_then(|target| types.kind_of(target)))
            {
                self.check_java_lambda_parameters(lambda, expected)?;
            }
        }
        Ok(JavaInference {
            map,
            signature: parsed,
        })
    }
}

impl Types<'_> {
    fn java_parameter_subtype(&self, left: &JType, right: &JType) -> bool {
        if left == right {
            return true;
        }
        match (left, right) {
            (JType::Class(left), JType::Class(right)) => self.is_java_subclass(left, right),
            (JType::Array(left), JType::Array(right)) => {
                left.is_reference()
                    && right.is_reference()
                    && self.java_parameter_subtype(left, right)
            }
            (JType::Array(_), JType::Class(right)) => matches!(
                right.as_str(),
                "java/lang/Object" | "java/lang/Cloneable" | "java/io/Serializable"
            ),
            (
                JType::Byte,
                JType::Short | JType::Int | JType::Long | JType::Float | JType::Double,
            )
            | (
                JType::Short | JType::Char,
                JType::Int | JType::Long | JType::Float | JType::Double,
            )
            | (JType::Int, JType::Long | JType::Float | JType::Double)
            | (JType::Long, JType::Float | JType::Double)
            | (JType::Float, JType::Double) => true,
            _ => false,
        }
    }

    fn java_overload_more_specific(
        &self,
        left: &JavaMethod,
        left_spread: bool,
        right: &JavaMethod,
        right_spread: bool,
        count: usize,
    ) -> bool {
        fn parameter(method: &JavaMethod, spread: bool, index: usize) -> Option<&JType> {
            if spread && index >= method.parameters.len() - 1 {
                match method.parameters.last()? {
                    JType::Array(element) => Some(element),
                    _ => None,
                }
            } else {
                method.parameters.get(index)
            }
        }
        let count = count.max(left.parameters.len()).max(right.parameters.len());
        let mut strict = false;
        for index in 0..count {
            let (Some(left), Some(right)) = (
                parameter(left, left_spread, index),
                parameter(right, right_spread, index),
            ) else {
                return false;
            };
            if !self.java_parameter_subtype(left, right) {
                return false;
            }
            strict |= left != right;
        }
        strict
            || (left.owner != right.owner && self.is_java_subclass(&left.owner, &right.owner))
            || (left.result != right.result
                && left.result.is_reference()
                && right.result.is_reference()
                && self.java_parameter_subtype(&left.result, &right.result))
    }
}

impl Emitter<'_> {
    /// Erased specificity says `List` beats `Collection`, but Java also
    /// compares type arguments: `List<String>` is not more specific than
    /// `Collection<Integer>`, so such a call is ambiguous. Parameters that
    /// mention the method's own type variables keep the erased answer.
    fn generic_more_specific(
        &self,
        left: &JavaMethod,
        right: &JavaMethod,
        receiver: Option<Kind>,
    ) -> bool {
        let parse = |method: &JavaMethod| {
            method
                .signature
                .as_deref()
                .and_then(super::generics::parse_method_signature)
        };
        let (Some(left_signature), Some(right_signature)) = (parse(left), parse(right)) else {
            return true;
        };
        if left_signature.parameters.len() != right_signature.parameters.len() {
            return true;
        }
        let types = &self.symbols.types;
        let map_for = |method: &JavaMethod, variables: &[String]| {
            let mut map = receiver
                .map(|receiver| types.substitution(receiver, &method.owner))
                .unwrap_or_default();
            for variable in variables {
                map.remove(variable);
            }
            map
        };
        let left_map = map_for(left, &left_signature.variables);
        let right_map = map_for(right, &right_signature.variables);
        left_signature
            .parameters
            .iter()
            .zip(&right_signature.parameters)
            .all(
                |(left_formal, right_formal)| match types.generic_kind(left_formal, &left_map) {
                    Some(kind) if !left_formal.has_wildcard() => self
                        .symbols
                        .java_signature_accepts(right_formal, kind, &right_map, false),
                    _ => true,
                },
            )
    }

    /// The phase of Java's overload resolution that accepts this candidate:
    /// subtyping and widening, then boxing, then variable arity. A Mammoth
    /// `Double` reaching a Java `float` waits until after real `double`s.
    fn java_phase(&self, candidate: &JavaMethod, kinds: &[Argument], spread: bool) -> u8 {
        if spread {
            return 3;
        }
        kinds
            .iter()
            .zip(&candidate.parameters)
            .map(|(&argument, target)| {
                let narrowing = matches!(
                    (argument, target),
                    (Argument::Value(Kind::Double), JType::Float)
                );
                match self.java_argument_cost(argument, target) {
                    _ if narrowing => 1,
                    Some(4..) => 2,
                    _ => 0,
                }
            })
            .max()
            .unwrap_or(0)
    }

    fn select_specific_java_overload<'m>(
        &self,
        candidates: &'m [JavaMethod],
        kinds: &[Argument],
        receiver: Option<Kind>,
    ) -> Option<(&'m JavaMethod, bool)> {
        let scored: Vec<_> = candidates
            .iter()
            .filter_map(|candidate| {
                overload_score(candidate, kinds, &|argument, target| {
                    self.java_argument_cost(argument, target)
                })
                .map(|(_, spread)| (candidate, spread, self.java_phase(candidate, kinds, spread)))
            })
            .collect();
        let best = scored.iter().map(|(_, _, phase)| *phase).min()?;
        let tied: Vec<_> = scored
            .into_iter()
            .filter(|(_, _, phase)| *phase == best)
            .collect();
        let winners: Vec<_> = tied
            .iter()
            .filter(|(candidate, spread, _)| {
                !tied.iter().any(|(other, other_spread, _)| {
                    self.symbols.types.java_overload_more_specific(
                        other,
                        *other_spread,
                        candidate,
                        *spread,
                        kinds.len(),
                    ) && self.generic_more_specific(other, candidate, receiver)
                })
            })
            .collect();
        if winners.len() != 1 {
            return None;
        }
        Some((winners[0].0, winners[0].1))
    }
}
