//! Checked type parameters for Mammoth functions. JVM descriptors erase a type
//! parameter to Object; callers box arguments and cast or unbox results.
use super::defaults::DefaultHost;
use super::*;

pub(super) fn signature<'a>(
    function: &'a Function<'a>,
    limit: usize,
    types: &Types<'a>,
) -> Result<Signature<'a>, Error> {
    if function.type_parameters.is_empty() {
        return Signature::resolve(
            function.name,
            &function.parameters,
            function.result,
            limit,
            types,
        );
    }
    let mut scope = types.clone();
    let parameters =
        types.declare_parameters(&mut scope, &function.type_parameters, &function.bounds)?;
    let mut signature = Signature::resolve(
        function.name,
        &function.parameters,
        function.result,
        limit,
        &scope,
    )?;
    signature.type_parameters = parameters;
    Ok(signature)
}

pub(super) fn call_name(name: Name<'_>) -> (Name<'_>, Option<Vec<Name<'_>>>) {
    let Some((base, text)) = name
        .text
        .strip_suffix('>')
        .and_then(|text| text.split_once('<'))
    else {
        return (name, None);
    };
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (offset, ch) in text.char_indices() {
        match ch {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&text[start..offset]);
                start = offset + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    let arguments = parts
        .into_iter()
        .map(|part| Name {
            text: part.trim(),
            offset: name.offset,
        })
        .collect();
    (
        Name {
            text: base.trim(),
            offset: name.offset,
        },
        Some(arguments),
    )
}

impl Types<'_> {
    pub(super) fn generic_signature(&self, signature: &Signature<'_>) -> String {
        let mut text = String::new();
        if !signature.type_parameters.is_empty() {
            text.push('<');
            for &(name, kind) in &signature.type_parameters {
                text.push_str(&self.formal_parameter(name.text, kind));
            }
            text.push('>');
        }
        text.push('(');
        for (_, kind) in &signature.parameters {
            text.push_str(&self.signature_type(*kind, false));
        }
        text.push(')');
        text.push_str(
            &signature
                .result
                .map_or_else(|| "V".into(), |kind| self.signature_type(kind, false)),
        );
        text
    }

    pub(super) fn signature_type(&self, kind: Kind, argument: bool) -> String {
        match kind {
            Kind::Applied(index) | Kind::AppliedInterface(index) => {
                let application = self.application(index);
                let args: String = application
                    .arguments
                    .into_iter()
                    .map(|kind| self.signature_type(kind, true))
                    .collect();
                format!("L{}<{args}>;", self.binary_name(kind))
            }
            Kind::TypeParameter(_) if self.is_star(kind) => {
                if argument {
                    "*".to_owned()
                } else {
                    "Ljava/lang/Object;".to_owned()
                }
            }
            Kind::TypeParameter(index) => format!("T{};", self.parameters.borrow()[index]),
            Kind::Nullable(_) if kind.is_nullable_primitive() => kind.descriptor(self).into_owned(),
            Kind::Nullable(_) => self.signature_type(kind.non_null(), argument),
            Kind::List(list) => format!("[{}", self.signature_type(list.element(), false)),
            Kind::MutableList(list) => format!(
                "Ljava/util/ArrayList<{}>;",
                self.signature_type(list.element(), true)
            ),
            Kind::Java(index) => {
                let args = self.type_arguments(kind);
                if args.is_empty() {
                    return kind.descriptor(self).into_owned();
                }
                let args: String = args
                    .into_iter()
                    .map(|kind| self.signature_type(kind, true))
                    .collect();
                format!("L{}<{args}>;", self.java_name(index))
            }
            Kind::Function(index) => {
                let function = self.function_type(index);
                let interface = function.interface().unwrap().0;
                let args: String = function
                    .parameters
                    .into_iter()
                    .chain(function.result)
                    .map(|kind| self.signature_type(kind, true))
                    .collect();
                if args.is_empty() {
                    format!("L{interface};")
                } else {
                    format!("L{interface}<{args}>;")
                }
            }
            kind if argument && !kind.is_reference() => {
                format!("L{};", Types::boxed_class(kind).unwrap())
            }
            kind => kind.descriptor(self).into_owned(),
        }
    }

    /// Whether a type carries information that its erased descriptor loses,
    /// so a JVM `Signature` attribute should record it for Java callers.
    pub(super) fn needs_signature(&self, kind: Kind) -> bool {
        self.signature_type(kind, false) != kind.descriptor(self)
    }

    pub(super) fn contains_parameter(&self, kind: Kind) -> bool {
        match kind {
            Kind::Applied(_) | Kind::AppliedInterface(_) => self
                .type_arguments(kind)
                .into_iter()
                .any(|kind| self.contains_parameter(kind)),
            Kind::TypeParameter(_) => true,
            Kind::Nullable(_) => self.contains_parameter(kind.non_null()),
            Kind::List(list) | Kind::MutableList(list) => self.contains_parameter(list.element()),
            Kind::Java(_) => self
                .type_arguments(kind)
                .into_iter()
                .any(|kind| self.contains_parameter(kind)),
            Kind::Function(index) => {
                let function = self.function_type(index);
                function
                    .parameters
                    .into_iter()
                    .chain(function.result)
                    .any(|kind| self.contains_parameter(kind))
            }
            _ => false,
        }
    }

    /// Whether substituting `map` into `kind` would turn a fixed-size list of
    /// a type parameter, which is an `Object[]` (or bound) array, into a list
    /// of primitives, which is a primitive array such as `int[]`. The two
    /// cannot share storage, and Mammoth never copies lists implicitly.
    pub(super) fn primitive_list_argument(&self, kind: Kind, map: &HashMap<Kind, Kind>) -> bool {
        if !self.contains_parameter(kind) {
            return false;
        }
        match kind {
            Kind::List(list) => {
                let element = list.element();
                if let Kind::TypeParameter(_) = element {
                    let actual = map.get(&element).copied().unwrap_or(element);
                    if !actual.is_reference() {
                        return true;
                    }
                }
                self.primitive_list_argument(element, map)
            }
            Kind::MutableList(list) => self.primitive_list_argument(list.element(), map),
            Kind::Nullable(_) => self.primitive_list_argument(kind.non_null(), map),
            Kind::Applied(_) | Kind::AppliedInterface(_) | Kind::Java(_) => self
                .type_arguments(kind)
                .into_iter()
                .any(|kind| self.primitive_list_argument(kind, map)),
            Kind::Function(index) => {
                let function = self.function_type(index);
                function
                    .parameters
                    .into_iter()
                    .chain(function.result)
                    .any(|kind| self.primitive_list_argument(kind, map))
            }
            _ => false,
        }
    }

    pub(super) fn replace_parameters(&self, kind: Kind, map: &HashMap<Kind, Kind>) -> Kind {
        if !self.contains_parameter(kind) {
            return kind;
        }
        match kind {
            Kind::Applied(index) | Kind::AppliedInterface(index) => {
                let application = self.application(index);
                let arguments = application
                    .arguments
                    .into_iter()
                    .map(|kind| self.replace_parameters(kind, map))
                    .collect();
                if application.interface {
                    self.apply_interface(application.class, arguments)
                } else {
                    self.apply_class(application.class, arguments)
                }
            }
            Kind::TypeParameter(_) => map.get(&kind).copied().unwrap_or(kind),
            Kind::Nullable(_) => {
                let inner = self.replace_parameters(kind.non_null(), map);
                inner.nullable().unwrap_or(inner)
            }
            Kind::List(list) => Kind::List(
                ListKind::for_element(self.replace_parameters(list.element(), map)).unwrap(),
            ),
            Kind::MutableList(list) => Kind::MutableList(
                ListKind::for_element(self.replace_parameters(list.element(), map)).unwrap(),
            ),
            Kind::Java(index) => {
                let arguments = self
                    .type_arguments(kind)
                    .into_iter()
                    .map(|kind| self.replace_parameters(kind, map))
                    .collect();
                Kind::Java(self.java_generic(&self.java_name(index), arguments))
            }
            Kind::Function(index) => {
                let function = self.function_type(index);
                let parameters = function
                    .parameters
                    .into_iter()
                    .map(|kind| self.replace_parameters(kind, map))
                    .collect();
                let result = function
                    .result
                    .map(|kind| self.replace_parameters(kind, map));
                self.function_kind(parameters, result)
            }
            other => other,
        }
    }

    pub(super) fn infer_parameters(
        &self,
        pattern: Kind,
        actual: Kind,
        map: &mut HashMap<Kind, Kind>,
    ) {
        match (pattern, actual) {
            (Kind::Applied(left), Kind::Applied(right))
            | (Kind::AppliedInterface(left), Kind::AppliedInterface(right)) => {
                let left = self.application(left);
                let right = self.application(right);
                if left.class == right.class {
                    for (left, right) in left.arguments.into_iter().zip(right.arguments) {
                        self.infer_parameters(left, right, map);
                    }
                }
            }
            (Kind::TypeParameter(_), Kind::Null | Kind::Nullable(_)) => {}
            (Kind::TypeParameter(_), actual) => {
                map.entry(pattern).or_insert(actual);
            }
            (Kind::Nullable(_), actual) if actual != Kind::Null => {
                self.infer_parameters(pattern.non_null(), actual.non_null(), map)
            }
            (Kind::MutableList(left), Kind::MutableList(right))
            | (Kind::List(left), Kind::List(right)) => {
                self.infer_parameters(left.element(), right.element(), map)
            }
            (Kind::Java(_), Kind::Java(_) | Kind::MutableList(_)) => {
                let owner = self.runtime_class_name(pattern).unwrap();
                let actual = self.substitution(actual, &owner);
                for (left, name) in self
                    .type_arguments(pattern)
                    .into_iter()
                    .zip(self.class_type_parameters(&owner))
                {
                    if let Some(&right) = actual.get(&name) {
                        self.infer_parameters(left, right, map);
                    }
                }
            }
            (Kind::Function(left), Kind::Function(right)) => {
                let left = self.function_type(left);
                let right = self.function_type(right);
                for (left, right) in left.parameters.into_iter().zip(right.parameters) {
                    self.infer_parameters(left, right, map);
                }
                if let (Some(left), Some(right)) = (left.result, right.result) {
                    self.infer_parameters(left, right, map);
                }
            }
            _ => {}
        }
    }
}

impl Symbols<'_> {
    pub(super) fn generic_result(
        &self,
        name: Name<'_>,
        args: &[Expr<'_>],
        scope: &dyn Fn(&str) -> Option<Kind>,
    ) -> Option<Kind> {
        let (name, explicit) = call_name(name);
        let signature = self.functions.get(name.text)?;
        if signature.type_parameters.is_empty() {
            return signature.result;
        }
        let mut map = HashMap::new();
        if let Some(explicit) = explicit {
            if explicit.len() != signature.type_parameters.len() {
                return None;
            }
            for ((_, parameter), annotation) in signature.type_parameters.iter().zip(explicit) {
                map.insert(*parameter, resolve_type(&self.types, annotation).ok()?);
            }
        }
        for ((_, pattern), arg) in signature.parameters.iter().zip(args) {
            if let Some(kind) = self.static_kind(arg, scope) {
                self.infer_from(*pattern, kind, &mut map);
            }
        }
        let kind = self.types.replace_parameters(signature.result?, &map);
        signature
            .type_parameters
            .iter()
            .all(|(_, kind)| map.contains_key(kind))
            .then_some(kind)
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn generic_call(
        &mut self,
        method: &mut Method<'a>,
        full_name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let (name, explicit) = call_name(full_name);
        let Some(signature) = self.symbols.functions.get(name.text) else {
            return Ok(None);
        };
        if method.binding(name).is_some()
            || self.fields.contains_key(name.text)
            || self.symbols.global(name.text).is_some()
        {
            return Ok(None);
        }
        if self
            .current_class
            .is_some_and(|class| self.symbols.find_method(class, name.text).is_some())
        {
            return Ok(None);
        }
        if signature.type_parameters.is_empty() {
            if explicit.is_some() {
                return Err(at(full_name, "This function does not take type arguments."));
            }
            return Ok(None);
        }
        let args = self.complete_args(method, full_name, signature, args, DefaultHost::Function)?;
        let map = self.infer_call(method, full_name, args, base, signature, expected)?;
        self.adapted_arguments(method, args, base, signature, &map)?;
        let reference = if let Some(target) = self.symbols.imports.get(name.text) {
            let class = self.pool.class(&target.class);
            self.pool
                .member_for(10, class, target.member, &signature.descriptor)
        } else {
            self.member(None, name.text, &signature.descriptor)
        };
        instruction(&mut method.code, 0xb8, reference);
        let result = signature.result.map(|erased| {
            let target = self.symbols.types.replace_parameters(erased, &map);
            if target != erased {
                self.cast_top(method, erased, target, base);
            }
            target
        });
        Ok(Some(result))
    }
    pub(super) fn infer_call(
        &mut self,
        method: &mut Method<'a>,
        full_name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        signature: &Signature<'a>,
        expected: Option<Kind>,
    ) -> Result<HashMap<Kind, Kind>, Error> {
        self.infer_call_in(
            method,
            full_name,
            args,
            base,
            signature,
            expected,
            &HashMap::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_call_in(
        &mut self,
        method: &mut Method<'a>,
        full_name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        signature: &Signature<'a>,
        expected: Option<Kind>,
        outer: &HashMap<Kind, Kind>,
    ) -> Result<HashMap<Kind, Kind>, Error> {
        let (name, explicit) = call_name(full_name);
        if args.len() != signature.parameters.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let mut map = outer.clone();
        if let Some(explicit) = explicit {
            if explicit.len() != signature.type_parameters.len() {
                return Err(at(
                    full_name,
                    "Give one type argument for each type parameter.",
                ));
            }
            for ((_, parameter), annotation) in signature.type_parameters.iter().zip(explicit) {
                let kind = resolve_type(&self.type_scope, annotation)?;
                if matches!(kind, Kind::Nullable(_)) {
                    return Err(at(
                        annotation,
                        "Type arguments must be non-null. Use T? in the function's parameter or result type for a missing value.",
                    ));
                }
                map.insert(*parameter, kind);
            }
        }
        if let (Some(pattern), Some(actual)) = (signature.result, expected) {
            // `Bad<T>` for an `Outcome<Int>` destination is seen as `Outcome<T>`.
            let pattern = self.symbols.supertype_view(actual, pattern);
            self.symbols
                .types
                .infer_parameters(pattern, actual, &mut map);
            // A star in the expected type says nothing about the argument.
            map.retain(|_, kind| !self.symbols.types.is_star(*kind));
        }
        // Probe without executing arguments. Snapshots also restore local slots
        // and stack-map state; arguments are emitted exactly once below.
        let saved = method.snapshot();
        let saved_null = self.allow_null;
        let inferred = (|| {
            for (arg, &(_, pattern)) in args.iter().zip(&signature.parameters) {
                if matches!(arg, Expr::Lambda(_)) {
                    continue;
                }
                let target = self.symbols.types.replace_parameters(pattern, &map);
                let kind = if self.symbols.types.contains_parameter(target) {
                    self.allow_null = matches!(target, Kind::Nullable(_));
                    self.expression(method, arg, base)?
                } else {
                    self.expression_as(method, arg, base, Some(target))?
                };
                self.symbols.infer_from(pattern, kind, &mut map);
                method.code.push(kind.pop_opcode());
            }
            for (arg, &(_, pattern)) in args.iter().zip(&signature.parameters) {
                let Expr::Lambda(lambda) = arg else { continue };
                if matches!(pattern.non_null(), Kind::TypeParameter(_)) {
                    let target = self
                        .symbols
                        .types
                        .replace_parameters(pattern, &map)
                        .non_null();
                    let context = matches!(target, Kind::Function(_)).then_some(target);
                    let kind = self.lambda(method, lambda, context, base)?;
                    self.symbols.infer_from(pattern, kind, &mut map);
                    method.code.push(0x57);
                    continue;
                }
                let Kind::Function(index) = pattern.non_null() else {
                    // A Java functional interface whose types are already known.
                    let target = self.symbols.types.replace_parameters(pattern, &map);
                    if matches!(target.non_null(), Kind::Java(_))
                        && !self.symbols.types.contains_parameter(target)
                    {
                        continue;
                    }
                    return Err(at(name, "A lambda needs a function parameter type here."));
                };
                let function = self.symbols.types.function_type(index);
                for ((_, annotation), pattern) in lambda.parameters.iter().zip(&function.parameters)
                {
                    if let Some(annotation) = annotation {
                        let kind = resolve_type(&self.type_scope, *annotation)?;
                        self.symbols
                            .types
                            .infer_parameters(*pattern, kind, &mut map);
                    }
                }
                let target = self.symbols.types.replace_parameters(pattern, &map);
                let Kind::Function(index) = target.non_null() else {
                    unreachable!()
                };
                let function = self.symbols.types.function_type(index);
                let missing: HashMap<_, _> = signature
                    .type_parameters
                    .iter()
                    .filter(|(_, kind)| !map.contains_key(kind))
                    .map(|(_, kind)| (*kind, Kind::Any))
                    .collect();
                if function
                    .parameters
                    .iter()
                    .any(|kind| self.symbols.types.replace_parameters(*kind, &missing) != *kind)
                {
                    return Err(at(
                        name,
                        "Give the lambda parameters types or provide explicit type arguments.",
                    ));
                }
                self.infer_lambda_result = function.result.is_some_and(|kind| {
                    self.symbols.types.replace_parameters(kind, &missing) != kind
                });
                let result = self.lambda(method, lambda, Some(target), base);
                self.infer_lambda_result = false;
                let kind = result?;
                self.symbols.infer_from(pattern, kind, &mut map);
                method.code.push(0x57);
            }
            Ok(())
        })();
        method.restore(saved);
        self.allow_null = saved_null;
        inferred?;
        if signature
            .type_parameters
            .iter()
            .any(|(_, kind)| !map.contains_key(kind))
        {
            return Err(at(
                full_name,
                "Cannot infer every type parameter. Give type arguments, such as identity<Int>(value), or a result type.",
            ));
        }
        self.symbols
            .check_bounds(&signature.type_parameters, &map, full_name)?;
        Ok(map)
    }
    pub(super) fn adapted_arguments(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        signature: &Signature<'a>,
        map: &HashMap<Kind, Kind>,
    ) -> Result<(), Error> {
        self.adapted_arguments_to(method, args, base, signature, map, signature)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn adapted_arguments_to(
        &mut self,
        method: &mut Method<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        signature: &Signature<'a>,
        map: &HashMap<Kind, Kind>,
        abi: &Signature<'a>,
    ) -> Result<(), Error> {
        for ((arg, &(_, erased)), &(_, abi_kind)) in
            args.iter().zip(&signature.parameters).zip(&abi.parameters)
        {
            if self.symbols.types.writes_star(erased, map) {
                return Err(Error {
                    offset: arg.offset(),
                    message: "This parameter has a star-projected type (*), so no value can be passed to it.",
                });
            }
            if self.symbols.types.primitive_list_argument(erased, map) {
                return Err(Error {
                    offset: arg.offset(),
                    message: "A generic List<T> holds objects, but this list holds primitive values. Use a list of objects or MutableList.",
                });
            }
            let target = self.symbols.types.replace_parameters(erased, map);
            let at = method.stack.len() as u16;
            let actual = self.expression_as(method, arg, at, Some(target))?;
            if matches!(target.non_null(), Kind::Java(_))
                && actual != Kind::Null
                && !self.symbols.java_type_arguments_fit(actual, target)
            {
                return Err(Error {
                    offset: arg.offset(),
                    message: "This collection's type arguments do not match the generic function parameter.",
                });
            }
            self.coerce(method, arg, actual, target, at)?;
            if matches!(erased, Kind::TypeParameter(_)) {
                self.box_value(method, target);
            }
            if erased != abi_kind {
                self.cast_top(method, erased, abi_kind, at);
            }
            method.push_stack(self.local_type(abi_kind));
        }
        method.stack.truncate(base as usize);
        Ok(())
    }
}
