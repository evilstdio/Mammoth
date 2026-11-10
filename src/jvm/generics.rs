//! Java generics: reading generic signatures from class files and working out
//! what a type variable such as `E` in `ArrayList<E>` stands for.
//!
//! Generic types are erased on the JVM, so this only affects the types the
//! compiler gives to values. A method declared to return `E` returns
//! `Object` at runtime; Mammoth then casts the result to the known type.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{Kind, ListKind, Types, interop::JType};

#[derive(Clone, Debug)]
pub(super) enum GType {
    Base(JType),
    Class(String, Vec<GType>),
    Var(String),
    Array(Box<GType>),
    /// Unbounded or upper-bounded wildcard.
    Wildcard(Option<Box<GType>>),
    /// Lower bound, retained separately for argument checking.
    Super(Box<GType>),
}

impl GType {
    pub(super) fn substitute(&self, map: &HashMap<String, GType>) -> GType {
        match self {
            Self::Var(name) => map.get(name).cloned().unwrap_or_else(|| self.clone()),
            Self::Class(name, args) => Self::Class(
                name.clone(),
                args.iter().map(|arg| arg.substitute(map)).collect(),
            ),
            Self::Array(inner) => Self::Array(Box::new(inner.substitute(map))),
            Self::Super(inner) => Self::Super(Box::new(inner.substitute(map))),
            Self::Wildcard(bound) => {
                Self::Wildcard(bound.as_ref().map(|bound| Box::new(bound.substitute(map))))
            }
            _ => self.clone(),
        }
    }

    pub(super) fn mentions(&self, variable: &str) -> bool {
        match self {
            Self::Var(name) => name == variable,
            Self::Class(_, arguments) => arguments.iter().any(|arg| arg.mentions(variable)),
            Self::Array(bound) | Self::Super(bound) | Self::Wildcard(Some(bound)) => {
                bound.mentions(variable)
            }
            _ => false,
        }
    }

    pub(super) fn has_wildcard(&self) -> bool {
        match self {
            Self::Wildcard(_) | Self::Super(_) => true,
            Self::Class(_, arguments) => arguments.iter().any(Self::has_wildcard),
            Self::Array(inner) => inner.has_wildcard(),
            _ => false,
        }
    }
}

struct Reader<'s> {
    text: &'s [u8],
    position: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn identifier(&mut self, stops: &[u8]) -> Option<String> {
        let start = self.position;
        while let Some(byte) = self.peek() {
            if stops.contains(&byte) {
                break;
            }
            self.position += 1;
        }
        std::str::from_utf8(&self.text[start..self.position])
            .ok()
            .map(str::to_owned)
    }

    fn type_parameters(&mut self) -> Option<Vec<String>> {
        Some(
            self.parameter_bounds()?
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
        )
    }

    fn parameter_bounds(&mut self) -> Option<Vec<(String, Vec<GType>)>> {
        let mut parameters = Vec::new();
        if self.peek() != Some(b'<') {
            return Some(parameters);
        }
        self.next();
        while self.peek()? != b'>' {
            let name = self.identifier(b":")?;
            if name.is_empty() || self.peek() != Some(b':') {
                return None;
            }
            let mut bounds = Vec::new();
            while self.peek() == Some(b':') {
                self.next();
                if matches!(self.peek(), Some(b'L' | b'T' | b'[')) {
                    bounds.push(self.reference()?);
                }
            }
            parameters.push((name, bounds));
        }
        self.next();
        Some(parameters)
    }

    fn java_type(&mut self) -> Option<GType> {
        Some(match self.peek()? {
            b'L' | b'T' | b'[' => self.reference()?,
            byte => {
                self.next();
                GType::Base(match byte {
                    b'I' => JType::Int,
                    b'J' => JType::Long,
                    b'D' => JType::Double,
                    b'F' => JType::Float,
                    b'Z' => JType::Bool,
                    b'C' => JType::Char,
                    b'B' => JType::Byte,
                    b'S' => JType::Short,
                    b'V' => JType::Void,
                    _ => return None,
                })
            }
        })
    }

    fn reference(&mut self) -> Option<GType> {
        match self.next()? {
            b'T' => {
                let name = self.identifier(b";")?;
                self.next();
                Some(GType::Var(name))
            }
            b'[' => Some(GType::Array(Box::new(self.java_type()?))),
            b'L' => {
                let mut name = self.identifier(b"<;.")?;
                let mut arguments = Vec::new();
                loop {
                    match self.peek()? {
                        b'<' => {
                            self.next();
                            arguments.clear();
                            while self.peek()? != b'>' {
                                arguments.push(match self.peek()? {
                                    b'*' => {
                                        self.next();
                                        GType::Wildcard(None)
                                    }
                                    b'+' | b'-' => {
                                        let extends = self.next()? == b'+';
                                        let bound = self.reference()?;
                                        if extends {
                                            GType::Wildcard(Some(Box::new(bound)))
                                        } else {
                                            // `? super T` accepts T, so treat it as T.
                                            GType::Super(Box::new(bound))
                                        }
                                    }
                                    _ => self.reference()?,
                                });
                            }
                            self.next();
                        }
                        b'.' => {
                            // A nested class of a generic class: Outer<T>.Inner
                            self.next();
                            name.push('$');
                            name.push_str(&self.identifier(b"<;.")?);
                        }
                        b';' => {
                            self.next();
                            return Some(GType::Class(name, arguments));
                        }
                        _ => return None,
                    }
                }
            }
            _ => None,
        }
    }
}

/// The type in a field's generic signature, such as `TT;` or
/// `Ljava/util/List<TT;>;`.
pub(super) fn parse_field_signature(signature: &str) -> Option<GType> {
    let mut reader = Reader {
        text: signature.as_bytes(),
        position: 0,
    };
    reader.java_type()
}

/// Type parameter names, superclass, and interfaces from a class signature.
pub(super) fn parse_class_signature(signature: &str) -> Option<(Vec<String>, Vec<GType>)> {
    let mut reader = Reader {
        text: signature.as_bytes(),
        position: 0,
    };
    let parameters = reader.type_parameters()?;
    let mut supers = Vec::new();
    while reader.peek().is_some() {
        supers.push(reader.reference()?);
    }
    Some((parameters, supers))
}

/// Formal class/interface bounds, retained independently of inheritance edges.
pub(super) fn parse_class_bounds(signature: &str) -> Option<Vec<(String, Vec<GType>)>> {
    Reader {
        text: signature.as_bytes(),
        position: 0,
    }
    .parameter_bounds()
}

/// A method's own type parameters, parameter types, and result type.
pub(super) struct MethodSignature {
    pub variables: Vec<String>,
    pub bounds: Vec<(String, Vec<GType>)>,
    pub parameters: Vec<GType>,
    pub result: GType,
}

impl MethodSignature {
    /// A spread call checks every trailing value against the array element.
    pub(super) fn argument(&self, position: usize, spread: bool) -> Option<&GType> {
        if spread && position >= self.parameters.len().checked_sub(1)? {
            match self.parameters.last()? {
                GType::Array(element) => Some(element),
                _ => None,
            }
        } else {
            self.parameters.get(position)
        }
    }
}

/// The Mammoth number type for a Java box class whose values Mammoth
/// stores as that box. Short, Byte and Float have no Mammoth type.
pub(super) fn unboxed(binary: &str) -> Option<Kind> {
    Some(match binary {
        "java/lang/Integer" => Kind::Int,
        "java/lang/Long" => Kind::Long,
        "java/lang/Double" => Kind::Double,
        "java/lang/Boolean" => Kind::Bool,
        "java/lang/Character" => Kind::Char,
        _ => return None,
    })
}

pub(super) fn parse_method_signature(signature: &str) -> Option<MethodSignature> {
    let mut reader = Reader {
        text: signature.as_bytes(),
        position: 0,
    };
    let bounds = reader.parameter_bounds()?;
    let variables = bounds.iter().map(|(name, _)| name.clone()).collect();
    if reader.next()? != b'(' {
        return None;
    }
    let mut parameters = Vec::new();
    while reader.peek()? != b')' {
        parameters.push(reader.java_type()?);
    }
    reader.next();
    let result = reader.java_type()?;
    Some(MethodSignature {
        variables,
        bounds,
        parameters,
        result,
    })
}

impl Types<'_> {
    /// Substitute symbolic arguments through Java superclass/interface edges.
    fn generic_view(&self, formal: &GType, target: &str) -> Option<GType> {
        let mut queue = VecDeque::from([formal.clone()]);
        let mut seen = HashSet::new();
        while let Some(current) = queue.pop_front() {
            let GType::Class(name, arguments) = &current else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            if name == target {
                return Some(current);
            }
            let Some(info) = self.java_class(name) else {
                continue;
            };
            if let Some((parameters, parents)) =
                info.signature.as_deref().and_then(parse_class_signature)
            {
                let map = parameters
                    .into_iter()
                    .zip(arguments.iter().cloned())
                    .collect();
                queue.extend(parents.into_iter().map(|parent| parent.substitute(&map)));
            } else {
                queue.extend(
                    info.super_name
                        .iter()
                        .chain(&info.interfaces)
                        .map(|parent| GType::Class(parent.clone(), Vec::new())),
                );
            }
        }
        None
    }
}

impl Types<'_> {
    pub(super) fn class_type_parameters(&self, binary: &str) -> Vec<String> {
        self.java_class(binary)
            .and_then(|info| info.signature.clone())
            .and_then(|signature| parse_class_signature(&signature))
            .map(|(parameters, _)| parameters)
            .unwrap_or_default()
    }

    /// What each type variable of `owner` stands for, seen from a value of
    /// type `receiver` (a subclass of `owner`).
    pub(super) fn substitution(&self, receiver: Kind, owner: &str) -> HashMap<String, Kind> {
        let Some(binary) = self.runtime_class_name(receiver) else {
            return HashMap::new();
        };
        let arguments = self.type_arguments(receiver);
        let start: HashMap<String, Kind> = self
            .class_type_parameters(&binary)
            .into_iter()
            .zip(arguments)
            .collect();
        let mut queue = VecDeque::from([(binary, start)]);
        let mut seen = HashSet::new();
        while let Some((name, map)) = queue.pop_front() {
            if name == owner {
                return map;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(info) = self.java_class(&name) else {
                continue;
            };
            match info.signature.as_deref().and_then(parse_class_signature) {
                Some((_, supers)) => {
                    for parent in supers {
                        let GType::Class(parent_name, parent_arguments) = parent else {
                            continue;
                        };
                        let parameters = self.class_type_parameters(&parent_name);
                        let parent_map = parameters
                            .into_iter()
                            .zip(&parent_arguments)
                            .filter_map(|(parameter, argument)| {
                                Some((parameter, self.generic_kind(argument, &map)?))
                            })
                            .collect();
                        queue.push_back((parent_name, parent_map));
                    }
                }
                None => {
                    if let Some(parent) = &info.super_name {
                        queue.push_back((parent.clone(), HashMap::new()));
                    }
                    for interface in &info.interfaces {
                        queue.push_back((interface.clone(), HashMap::new()));
                    }
                }
            }
        }
        HashMap::new()
    }

    /// The Mammoth type of a value that Java hands to Mammoth: a method
    /// result or a field. Each wildcard type argument becomes a fresh hidden
    /// type parameter (a capture) bounded by the wildcard's upper bound, so
    /// the value reads as that bound but nothing can be written into it. A
    /// `List<? extends Number>` therefore accepts no `add` and is not a
    /// `List<Number>`. A `? super` capture reads as `Any`; writes are
    /// conservatively rejected too. Types without wildcards are unchanged.
    ///
    /// `site` is the source file and offset of the call or field read.
    /// Captures belong to their site, so probing and emitting one call see
    /// the same types, while values from different sites never mix.
    pub(super) fn result_kind(
        &self,
        gtype: &GType,
        map: &HashMap<String, Kind>,
        site: (usize, usize),
    ) -> Option<Kind> {
        self.capture_kind(gtype, map, site, &mut 0)
    }

    fn capture_kind(
        &self,
        gtype: &GType,
        map: &HashMap<String, Kind>,
        site: (usize, usize),
        ordinal: &mut usize,
    ) -> Option<Kind> {
        if !gtype.has_wildcard() {
            return self.generic_kind(gtype, map);
        }
        match gtype {
            GType::Wildcard(bound) => {
                let bound = bound
                    .as_ref()
                    .and_then(|bound| self.capture_kind(bound, map, site, ordinal));
                Some(self.wildcard_capture(bound, false, site, ordinal))
            }
            GType::Super(bound) => {
                let bound = self.capture_kind(bound, map, site, ordinal);
                Some(self.wildcard_capture(bound, true, site, ordinal))
            }
            GType::Array(inner) => {
                if let GType::Base(element) = inner.as_ref() {
                    return self.kind_of(&JType::Array(Box::new(element.clone())));
                }
                let element = self.capture_kind(inner, map, site, ordinal)?;
                Some(Kind::List(ListKind::for_element(element)?))
            }
            GType::Class(name, arguments) => {
                let base = self.java_kind(name);
                if !matches!(base, Kind::Java(_)) {
                    return Some(base);
                }
                let mapped: Vec<Kind> = arguments
                    .iter()
                    .map(|argument| {
                        self.capture_kind(argument, map, site, ordinal)
                            .unwrap_or(Kind::Any)
                    })
                    .collect();
                Some(Kind::Java(self.java_generic(name, mapped)))
            }
            GType::Base(_) | GType::Var(_) => self.generic_kind(gtype, map),
        }
    }

    /// A capture of `? extends bound`, or of `? super bound` when `lower`.
    /// A lower-bounded capture reads as `Any` and accepts its bound.
    fn wildcard_capture(
        &self,
        bound: Option<Kind>,
        lower: bool,
        site: (usize, usize),
        ordinal: &mut usize,
    ) -> Kind {
        let bound = bound.filter(|&bound| bound != Kind::Any);
        let key = (site.0, site.1, *ordinal, bound, lower);
        *ordinal += 1;
        if let Some(&kind) = self.captures.borrow().get(&key) {
            return kind;
        }
        let index = self.parameters.borrow().len();
        self.parameters
            .borrow_mut()
            .push(if lower { "? super" } else { "?" });
        let upper = if lower { None } else { bound };
        self.bounds.borrow_mut().push(upper.into_iter().collect());
        self.variances
            .borrow_mut()
            .push(crate::syntax::Variance::Invariant);
        let kind = Kind::TypeParameter(index);
        self.captures.borrow_mut().insert(key, kind);
        kind
    }

    /// The lower bound of a `? super` capture: values of that type can be
    /// written where the capture is expected.
    pub(super) fn capture_lower(&self, kind: Kind) -> Option<Kind> {
        let Kind::TypeParameter(index) = kind.non_null() else {
            return None;
        };
        if self.parameters.borrow()[index] != "? super" {
            return None;
        }
        self.captures
            .borrow()
            .iter()
            .find(|(key, value)| key.4 && **value == kind.non_null())
            .and_then(|(key, _)| key.3)
    }

    /// Hidden type parameters that stand in for type arguments while a cast
    /// works out which ones a value's type determines. They never escape.
    pub(super) fn cast_markers(&self, count: usize) -> Vec<Kind> {
        let mut ordinal = 0;
        (0..count)
            .map(|_| self.wildcard_capture(None, false, (usize::MAX, usize::MAX), &mut ordinal))
            .collect()
    }

    /// Whether `kind` is `parameter` or has it among its type arguments.
    pub(super) fn mentions(&self, kind: Kind, parameter: Kind) -> bool {
        kind == parameter
            || match kind {
                Kind::Nullable(_) => self.mentions(kind.non_null(), parameter),
                Kind::List(list) | Kind::MutableList(list) => {
                    self.mentions(list.element(), parameter)
                }
                Kind::Applied(_) | Kind::AppliedInterface(_) | Kind::Java(_) => self
                    .type_arguments(kind)
                    .into_iter()
                    .any(|kind| self.mentions(kind, parameter)),
                _ => false,
            }
    }

    /// Whether `kind` mentions a Java wildcard capture.
    pub(super) fn has_capture(&self, kind: Kind) -> bool {
        match kind {
            Kind::TypeParameter(index) => self.parameters.borrow()[index].starts_with('?'),
            Kind::Nullable(_) => self.has_capture(kind.non_null()),
            Kind::List(list) | Kind::MutableList(list) => self.has_capture(list.element()),
            Kind::Applied(_) | Kind::AppliedInterface(_) | Kind::Java(_) => self
                .type_arguments(kind)
                .into_iter()
                .any(|kind| self.has_capture(kind)),
            Kind::Function(index) => {
                let function = self.function_type(index);
                function
                    .parameters
                    .into_iter()
                    .chain(function.result)
                    .any(|kind| self.has_capture(kind))
            }
            _ => false,
        }
    }

    /// The Mammoth type for a generic Java type, given what its type
    /// variables stand for. `None` when a type variable is unknown.
    pub(super) fn generic_kind(&self, gtype: &GType, map: &HashMap<String, Kind>) -> Option<Kind> {
        match gtype {
            GType::Base(jtype) => self.kind_of(jtype),
            GType::Var(name) => map.get(name).copied(),
            GType::Wildcard(bound) => bound
                .as_ref()
                .and_then(|bound| self.generic_kind(bound, map)),
            GType::Super(bound) => self.generic_kind(bound, map),
            GType::Array(inner) => {
                // `byte[]` is its own list type, though a byte reads as `Int`.
                if let GType::Base(element) = inner.as_ref() {
                    return self.kind_of(&JType::Array(Box::new(element.clone())));
                }
                let element = self.generic_kind(inner, map)?;
                Some(Kind::List(ListKind::for_element(element)?))
            }
            GType::Class(name, arguments) => {
                // Java's boxes are Mammoth's numbers, so a `List<Integer>`
                // is a `List<Int>`, the same type Mammoth code writes.
                if let Some(primitive) = unboxed(name) {
                    return Some(primitive);
                }
                let base = self.java_kind(name);
                if arguments.is_empty() || !matches!(base, Kind::Java(_)) {
                    return Some(base);
                }
                let mapped: Vec<Kind> = arguments
                    .iter()
                    .map(|argument| self.generic_kind(argument, map).unwrap_or(Kind::Any))
                    .collect();
                if mapped.iter().all(|kind| *kind == Kind::Any)
                    && arguments
                        .iter()
                        .all(|argument| self.generic_kind(argument, map).is_none())
                {
                    return Some(base);
                }
                Some(Kind::Java(self.java_generic(name, mapped)))
            }
        }
    }
}

impl super::Symbols<'_> {
    pub(super) fn java_lambda_signature(&self, formal: &GType) -> Option<MethodSignature> {
        let GType::Class(owner, _) = formal else {
            return None;
        };
        let methods = self.types.java_abstract_methods(owner);
        if methods.len() != 1 {
            return None;
        }
        let sam = &methods[0];
        let GType::Class(_, arguments) = self.types.generic_view(formal, &sam.owner)? else {
            return None;
        };
        let map = self
            .types
            .class_type_parameters(&sam.owner)
            .into_iter()
            .zip(arguments)
            .collect();
        let mut signature = sam
            .signature
            .as_deref()
            .and_then(parse_method_signature)
            .unwrap_or_else(|| MethodSignature {
                variables: Vec::new(),
                bounds: Vec::new(),
                parameters: sam.parameters.iter().cloned().map(GType::Base).collect(),
                result: GType::Base(sam.result.clone()),
            });
        if !signature.variables.is_empty() {
            return None;
        }
        signature.parameters = signature
            .parameters
            .iter()
            .map(|parameter| parameter.substitute(&map))
            .collect();
        signature.result = signature.result.substitute(&map);
        Some(signature)
    }

    /// View a generic result through the destination's Java ancestor before
    /// collecting constraints. This preserves symbolic method variables.
    pub(super) fn collect_java_result_constraints(
        &self,
        result: &GType,
        expected: Kind,
        variables: &[String],
        constraints: &mut HashMap<String, Vec<(Kind, i8)>>,
    ) {
        if let (GType::Class(_, _), Some(target)) =
            (result, self.types.runtime_class_name(expected))
        {
            let mut queue = VecDeque::from([result.clone()]);
            let mut seen = HashSet::new();
            while let Some(current) = queue.pop_front() {
                let GType::Class(name, arguments) = &current else {
                    continue;
                };
                if !seen.insert(name.clone()) {
                    continue;
                }
                if *name == target {
                    self.collect_java_constraints(&current, expected, variables, -1, constraints);
                    return;
                }
                let Some(info) = self.types.java_class(name) else {
                    continue;
                };
                if let Some((parameters, parents)) =
                    info.signature.as_deref().and_then(parse_class_signature)
                {
                    let map = parameters
                        .into_iter()
                        .zip(arguments.iter().cloned())
                        .collect();
                    queue.extend(parents.into_iter().map(|parent| parent.substitute(&map)));
                } else {
                    queue.extend(
                        info.super_name
                            .iter()
                            .chain(&info.interfaces)
                            .map(|parent| GType::Class(parent.clone(), Vec::new())),
                    );
                }
            }
        } else {
            self.collect_java_constraints(result, expected, variables, -1, constraints);
        }
    }

    /// Collect lower (1), equality (0), and upper (-1) constraints separately.
    pub(super) fn collect_java_constraints(
        &self,
        formal: &GType,
        actual: Kind,
        variables: &[String],
        direction: i8,
        constraints: &mut HashMap<String, Vec<(Kind, i8)>>,
    ) {
        match formal {
            GType::Var(name) if actual != Kind::Null && variables.contains(name) => {
                constraints
                    .entry(name.clone())
                    .or_default()
                    .push((actual.non_null(), direction));
            }
            GType::Wildcard(Some(bound)) => {
                self.collect_java_constraints(bound, actual, variables, 1, constraints)
            }
            GType::Super(bound) => {
                self.collect_java_constraints(bound, actual, variables, -1, constraints)
            }
            GType::Array(inner) => {
                if let Kind::List(list) = actual.non_null() {
                    self.collect_java_constraints(
                        inner,
                        list.element(),
                        variables,
                        direction,
                        constraints,
                    );
                }
            }
            GType::Class(owner, arguments) => {
                for view in self.java_generic_views(actual) {
                    let actual_map = self.types.substitution(view, owner);
                    for (name, formal) in self
                        .types
                        .class_type_parameters(owner)
                        .iter()
                        .zip(arguments)
                    {
                        if let Some(&actual) = actual_map.get(name) {
                            self.collect_java_constraints(
                                formal,
                                actual,
                                variables,
                                0,
                                constraints,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn inference_candidates(&self, actual: Kind) -> Vec<Kind> {
        let mut found = vec![actual];
        if let Some(class) = self.types.native_class(actual) {
            for ancestor in self.ancestors(class) {
                if let Some(view) = self.class_view(actual, ancestor) {
                    found.push(view);
                }
            }
        }
        if self.types.interface_root(actual).is_some() {
            found.extend(self.interface_ancestors(actual));
        }
        let mut queue = VecDeque::from(self.java_generic_views(actual));
        let mut seen = HashSet::new();
        while let Some(view) = queue.pop_front() {
            let Some(binary) = self.types.runtime_class_name(view) else {
                continue;
            };
            if !seen.insert(binary.clone()) {
                continue;
            }
            let Some(info) = self.types.java_class(&binary) else {
                continue;
            };
            if info.is_public() {
                found.push(view);
            }
            for parent in info.super_name.iter().chain(&info.interfaces) {
                let parameters = self.types.class_type_parameters(parent);
                let map = self.types.substitution(view, parent);
                let arguments: Option<Vec<_>> = parameters
                    .iter()
                    .map(|name| map.get(name).copied())
                    .collect();
                if let Some(arguments) = arguments {
                    queue.push_back(if arguments.is_empty() {
                        self.types.java_kind(parent)
                    } else {
                        Kind::Java(self.types.java_generic(parent, arguments))
                    });
                }
            }
        }
        found.push(Kind::Any);
        found
    }

    pub(super) fn solve_java_constraints(
        &self,
        constraints: &HashMap<String, Vec<(Kind, i8)>>,
        map: &mut HashMap<String, Kind>,
        signature: Option<&MethodSignature>,
    ) -> bool {
        for (name, constraints) in constraints {
            let seeds = if let Some((exact, _)) =
                constraints.iter().find(|(_, direction)| *direction == 0)
            {
                vec![*exact]
            } else if constraints
                .iter()
                .all(|(actual, _)| self.types.same_kind(*actual, constraints[0].0))
            {
                vec![constraints[0].0]
            } else {
                constraints
                    .iter()
                    .flat_map(|(actual, _)| self.inference_candidates(*actual))
                    .collect()
            };
            let mut candidates: Vec<_> = seeds
                .into_iter()
                .filter(|candidate| {
                    constraints
                        .iter()
                        .all(|(actual, direction)| match direction {
                            1 => self.fits(*actual, *candidate),
                            -1 => self.fits(*candidate, *actual),
                            _ => self.types.same_kind(*actual, *candidate),
                        })
                })
                .filter(|candidate| {
                    signature.is_none_or(|signature| {
                        let mut tentative = map.clone();
                        tentative.insert(name.clone(), *candidate);
                        signature
                            .bounds
                            .iter()
                            .filter(|(variable, _)| variable == name)
                            .flat_map(|(_, bounds)| bounds)
                            .all(|bound| {
                                // Relations to other method variables propagate in
                                // rounds; do not filter using stale substitutions.
                                signature
                                    .variables
                                    .iter()
                                    .any(|other| other != name && bound.mentions(other))
                                    || self.java_signature_accepts(
                                        bound, *candidate, &tentative, false,
                                    )
                            })
                    })
                })
                .collect();
            // Pick a minimal satisfying type; stable signatures break ties
            // between incomparable views without depending on argument order.
            let all = candidates.clone();
            candidates.retain(|candidate| {
                !all.iter()
                    .any(|other| self.fits(*other, *candidate) && !self.fits(*candidate, *other))
            });
            let Some(chosen) = candidates
                .into_iter()
                .min_by_key(|kind| self.types.signature_type(*kind, false))
            else {
                return false;
            };
            map.insert(name.clone(), chosen);
        }
        true
    }

    /// Feed dependent bounds back into inference until substitutions stabilize.
    /// Self-recursive bounds remain checked after solving, not expanded here.
    pub(super) fn propagate_java_bounds(
        &self,
        signature: &MethodSignature,
        original: &HashMap<String, Vec<(Kind, i8)>>,
        map: &mut HashMap<String, Kind>,
    ) -> bool {
        for _ in 0..signature
            .variables
            .len()
            .saturating_mul(4)
            .saturating_add(8)
        {
            let previous = map.clone();
            let mut constraints = original.clone();
            for (variable, bounds) in &signature.bounds {
                if let Some(&actual) = previous.get(variable) {
                    for bound in bounds {
                        if !bound.mentions(variable) {
                            self.collect_java_constraints(
                                bound,
                                actual,
                                &signature.variables,
                                1,
                                &mut constraints,
                            );
                        }
                    }
                }
            }
            if !self.solve_java_constraints(&constraints, map, Some(signature)) {
                return false;
            }
            if signature
                .variables
                .iter()
                .all(|name| match (previous.get(name), map.get(name)) {
                    (Some(left), Some(right)) => self.types.same_kind(*left, *right),
                    (None, None) => true,
                    _ => false,
                })
            {
                return true;
            }
        }
        false // Reject nonconverging inference rather than looping indefinitely.
    }

    /// Java generic classes are invariant. Compare arguments in the declared
    /// ancestor's view, rather than comparing the subclass's argument positions.
    pub(super) fn java_type_arguments_fit(&self, actual: Kind, expected: Kind) -> bool {
        let expected = expected.non_null();
        if !matches!(expected, Kind::Java(_)) {
            return true;
        }
        let arguments = self.types.type_arguments(expected);
        if arguments.is_empty() {
            return true; // An explicitly raw Java type retains its erased API.
        }
        let owner = self.types.runtime_class_name(expected).unwrap();
        self.java_generic_views(actual).into_iter().any(|view| {
            let map = self.types.substitution(view, &owner);
            self.types
                .class_type_parameters(&owner)
                .iter()
                .zip(&arguments)
                .all(|(name, expected)| {
                    self.types.is_star(*expected)
                        || map
                            .get(name)
                            .is_some_and(|actual| self.types.same_kind(*actual, *expected))
                })
        })
    }
    pub(super) fn java_generic_views(&self, actual: Kind) -> Vec<Kind> {
        let boxed = Types::boxed_class(actual.non_null()).map(|name| self.types.java_kind(name));
        let mut views = vec![
            self.java_super_view(actual)
                .or(boxed)
                .unwrap_or(actual.non_null()),
        ];
        if self.types.interface_root(actual.non_null()).is_some() {
            views.extend(self.java_parents(actual.non_null()));
        }
        if let Some(class) = self.types.native_class(actual.non_null()) {
            for ancestor in self.ancestors(class) {
                let view = self.class_view(actual.non_null(), ancestor).unwrap();
                let map = self.types.class_map(view);
                views.extend(
                    self.java_interfaces(ancestor)
                        .into_iter()
                        .map(|kind| self.types.replace_parameters(kind, &map)),
                );
            }
        }
        views
    }

    /// Whether a Mammoth primitive and its Java box name the same type
    /// argument, as `Int` and `java.lang.Integer` do.
    pub(super) fn boxed_same(&self, left: Kind, right: Kind) -> bool {
        Types::boxed_class(right)
            .is_some_and(|boxed| self.types.runtime_class_name(left).as_deref() == Some(boxed))
            || Types::boxed_class(left)
                .is_some_and(|boxed| self.types.runtime_class_name(right).as_deref() == Some(boxed))
    }

    /// Check the original signature before erasure. Nested concrete arguments
    /// are invariant; wildcards change direction only at their own position.
    pub(super) fn java_signature_accepts(
        &self,
        formal: &GType,
        actual: Kind,
        map: &HashMap<String, Kind>,
        invariant: bool,
    ) -> bool {
        if actual == Kind::Null && !invariant {
            return true; // Java reference parameters permit null.
        }
        if !invariant
            && matches!(formal, GType::Var(_))
            && self
                .types
                .generic_kind(formal, map)
                .is_some_and(|expected| {
                    self.types.same_kind(actual.non_null(), expected.non_null())
                })
        {
            return true;
        }
        if !invariant && self.types.bound(actual).is_some() {
            return self
                .types
                .all_bounds(actual)
                .into_iter()
                .any(|bound| self.java_signature_accepts(formal, bound, map, false));
        }
        match formal {
            GType::Base(_) if !invariant => true, // Overload selection checks primitive conversions.
            GType::Wildcard(None) => true,
            GType::Wildcard(Some(bound)) => self.java_signature_accepts(bound, actual, map, false),
            GType::Super(bound) => self.types.generic_kind(bound, map).is_none_or(|bound| {
                self.fits(bound.non_null(), actual.non_null())
                    || self.boxed_same(bound.non_null(), actual.non_null())
            }),
            GType::Class(owner, arguments) if !arguments.is_empty() => {
                // List<List<? extends A>> cannot accept List<List<B>>: the
                // callee may insert a different inner list. Wildcard captures
                // cannot be equal to concrete types at an invariant position.
                if invariant && formal.has_wildcard() {
                    return false;
                }
                self.java_generic_views(actual).into_iter().any(|view| {
                    let Some(binary) = self.types.runtime_class_name(view) else {
                        return false;
                    };
                    if (invariant && binary != *owner)
                        || !self.types.is_java_subclass(&binary, owner)
                    {
                        return false;
                    }
                    let actual_map = self.types.substitution(view, owner);
                    let parameters = self.types.class_type_parameters(owner);
                    parameters.len() == arguments.len()
                        && parameters.iter().zip(arguments).all(|(name, argument)| {
                            actual_map.get(name).map_or_else(
                                || self.types.generic_kind(argument, map).is_none(),
                                |actual| self.java_signature_accepts(argument, *actual, map, true),
                            )
                        })
                })
            }
            GType::Array(inner) => match actual.non_null() {
                Kind::List(list) => {
                    self.java_signature_accepts(inner, list.element(), map, invariant)
                }
                _ => false,
            },
            _ => self.types.generic_kind(formal, map).is_none_or(|expected| {
                if invariant {
                    self.types.same_kind(actual, expected) || self.boxed_same(actual, expected)
                } else {
                    self.fits(actual.non_null(), expected.non_null())
                }
            }),
        }
    }
}
