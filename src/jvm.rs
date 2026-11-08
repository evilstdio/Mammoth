use std::{
    borrow::Cow,
    cell::{OnceCell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::classpath::ClassPath;
use defaults::DefaultHost;

use crate::syntax::{
    Arithmetic, Catch, Class, Comparison, Condition, Error, Expr, Function, Initializer, Interface,
    JavaClass, Kind, Lambda, ListKind, Logic, Name, Part, Program, Reference, Statement,
    WhenBranch, WhenCondition,
};

mod annotations;
mod bounds;
mod branching;
mod bridges;
mod builtins;
mod casts;
mod classes;
mod collections;
mod defaults;
mod exceptions;
mod extensions;
mod function_types;
mod generic_methods;
mod generics;
mod globals;
mod interop;
mod java_fields;
mod lambdas;
mod native_classes;
mod native_generics;
mod overloads;
mod project;
mod special;
mod statics;
mod variance;

const ARRAY_LIST: &str = "java/util/ArrayList";

type TypeParameters<'a> = Vec<(Name<'a>, Kind)>;

pub struct ClassFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn emit<'a>(program: &'a Program<'a>) -> Result<Vec<ClassFile>, Error> {
    let symbols = resolve_symbols(program)?;
    let mut classes = emit_classes(program, &symbols)?;
    classes.extend(function_types::interfaces(&symbols.types));
    Ok(classes)
}

/// Resolves declarations once for the entire project, then emits each file in
/// its own scope. The index on an error identifies its source file.
pub fn emit_project<'a>(programs: &'a [Program<'a>]) -> Result<Vec<ClassFile>, (usize, Error)> {
    let programs: Vec<_> = programs.iter().collect();
    let symbols = project::resolve(&programs)?;
    let mut classes = Vec::new();
    for (index, (program, symbols)) in programs.iter().zip(&symbols).enumerate() {
        classes.extend(emit_classes(program, symbols).map_err(|error| (index, error))?);
    }
    if let Some(first) = symbols.first() {
        classes.extend(function_types::interfaces(&first.types));
    }
    Ok(classes)
}

fn resolve_symbols<'a>(program: &'a Program<'a>) -> Result<Symbols<'a>, Error> {
    project::resolve(&[program])
        .map(|mut symbols| symbols.remove(0))
        .map_err(|(_, error)| error)
}

fn emit_classes<'a>(
    program: &'a Program<'a>,
    symbols: &'a Symbols<'a>,
) -> Result<Vec<ClassFile>, Error> {
    symbols.check_pending_bounds(symbols.types.module)?;
    let mut files = Vec::new();
    for &index in &symbols.own_interfaces {
        files.push(emit_interface(program, symbols, index)?);
    }
    for &index in &symbols.own_classes {
        files.push(emit_class(program, symbols, Some(index))?);
    }
    files.push(emit_class(program, symbols, None)?);
    symbols.check_pending_bounds(symbols.types.module)?;
    Ok(files)
}

pub struct JavaCheck {
    pub operation: &'static str,
    pub class: String,
    pub member: String,
    pub descriptor: String,
    pub offset: usize,
}

pub fn java_checks(program: &Program<'_>) -> Result<Vec<JavaCheck>, Error> {
    let symbols = resolve_symbols(program)?;
    emit_classes(program, &symbols)?;
    let mut checks = Vec::new();
    for class in &program.java_classes {
        let Kind::Java(index) = symbols.types.names[class.name.text] else {
            unreachable!()
        };
        let binary = &symbols.types.java_name(index);
        let resolved = &symbols.java_classes[&index];
        checks.push(JavaCheck {
            operation: if class.interface {
                "interface"
            } else {
                "class"
            },
            class: binary.clone(),
            member: String::new(),
            descriptor: String::new(),
            offset: class.target.offset,
        });
        if let (Some(constructor), Some(signature)) = (&class.constructor, &resolved.constructor) {
            checks.push(JavaCheck {
                operation: "constructor",
                class: binary.clone(),
                member: "<init>".into(),
                descriptor: signature.descriptor.clone(),
                offset: constructor.name.offset,
            });
        }
        for method in &class.methods {
            let name = method.foreign_name.unwrap_or(method.name);
            checks.push(JavaCheck {
                operation: "instance",
                class: binary.clone(),
                member: name.text.into(),
                descriptor: resolved.methods[method.name.text].descriptor.clone(),
                offset: name.offset,
            });
        }
    }
    for binding in &program.java {
        let name = binding.function.name.text;
        let target = &symbols.java[name];
        checks.push(JavaCheck {
            operation: if symbols.java_owner_is_interface(&target.class) {
                "static-interface"
            } else {
                "static"
            },
            class: target.class.clone(),
            member: target.member.into(),
            descriptor: symbols.functions[name].descriptor.clone(),
            offset: binding.target.offset,
        });
    }
    for field in &program.java_fields {
        let (kind, target) = &symbols.java_fields[field.name.text];
        checks.push(JavaCheck {
            operation: "field",
            class: target.class.clone(),
            member: target.member.into(),
            descriptor: kind.descriptor(&symbols.types).into_owned(),
            offset: field.target.offset,
        });
    }
    checks.sort_unstable_by_key(|check| check.offset);
    Ok(checks)
}

#[derive(Clone)]
struct Types<'a> {
    module: Option<&'a str>,
    native_names: Rc<project::NativeNames>,
    names: HashMap<&'a str, Kind>,
    /// Internal names of Java classes. `Kind::Java(index)` refers to an entry.
    /// Declared `java class` bindings come first; classes found on the class
    /// path are added while compiling.
    java: Rc<RefCell<Vec<JavaEntry>>>,
    classpath: Rc<OnceCell<ClassPath>>,
    /// Packages imported with `import some.package.*`, as internal names.
    packages: Vec<String>,
    /// Signatures of function types. `Kind::Function(index)` refers to an entry.
    functions: Rc<RefCell<Vec<FunctionType>>>,
    parameters: Rc<RefCell<Vec<&'a str>>>,
    class_parameters: Rc<RefCell<Vec<TypeParameters<'a>>>>,
    interface_parameters: Rc<RefCell<Vec<TypeParameters<'a>>>>,
    /// The bound of each type parameter, by `Kind::TypeParameter` index.
    bounds: Rc<RefCell<Vec<Vec<Kind>>>>,
    /// The declared variance of each type parameter, by index. Only
    /// interface parameters can be `out` or `in`.
    variances: Rc<RefCell<Vec<crate::syntax::Variance>>>,
    /// Star projections, one hidden type parameter per declaration and
    /// position: (is interface, declaration, position, parameter).
    #[allow(clippy::type_complexity)]
    stars: Rc<RefCell<Vec<(bool, usize, usize, Kind)>>>,
    /// Java wildcard captures, one hidden type parameter per source site:
    /// (source file, offset, wildcard ordinal, bound, is lower bound) to
    /// parameter.
    #[allow(clippy::type_complexity)]
    captures: Rc<RefCell<HashMap<(usize, usize, usize, Option<Kind>, bool), Kind>>>,
    /// Parameterized types written in the source whose arguments are checked
    /// against bounds after declarations are resolved: the type, the source
    /// offset, and the module that wrote it.
    #[allow(clippy::type_complexity)]
    pending_bounds: Rc<RefCell<Vec<(Kind, usize, Option<&'a str>)>>>,
    applications: Rc<RefCell<Vec<native_classes::Application>>>,
}

struct JavaEntry {
    binary: String,
    arguments: Vec<Kind>,
    declared: bool,
}

#[derive(Clone, PartialEq)]
struct FunctionType {
    parameters: Vec<Kind>,
    result: Option<Kind>,
}

impl FunctionType {
    /// The Java functional interface, its method, and its erased descriptor.
    fn interface(&self) -> Option<(&'static str, &'static str, &'static str)> {
        Some(match (self.parameters.len(), self.result.is_some()) {
            (0, false) => ("java/lang/Runnable", "run", "()V"),
            (0, true) => ("java/util/function/Supplier", "get", "()Ljava/lang/Object;"),
            (1, false) => (
                "java/util/function/Consumer",
                "accept",
                "(Ljava/lang/Object;)V",
            ),
            (1, true) => (
                "java/util/function/Function",
                "apply",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            ),
            (2, false) => (
                "java/util/function/BiConsumer",
                "accept",
                "(Ljava/lang/Object;Ljava/lang/Object;)V",
            ),
            (2, true) => (
                "java/util/function/BiFunction",
                "apply",
                "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            ),
            (count, result) if count <= function_types::MAX_PARAMETERS => {
                let (class, descriptor) = function_types::names(count, result);
                (class, "invoke", descriptor)
            }
            _ => return None,
        })
    }
}

impl Types<'_> {
    fn java_name(&self, index: usize) -> String {
        self.java.borrow()[index].binary.clone()
    }

    fn function_type(&self, index: usize) -> FunctionType {
        self.functions.borrow()[index].clone()
    }

    /// The `Kind::Function` for a signature, adding it if needed.
    fn function_kind(&self, parameters: Vec<Kind>, result: Option<Kind>) -> Kind {
        let signature = FunctionType { parameters, result };
        let mut functions = self.functions.borrow_mut();
        let index = functions
            .iter()
            .position(|existing| *existing == signature)
            .unwrap_or_else(|| {
                functions.push(signature);
                functions.len() - 1
            });
        Kind::Function(index)
    }

    fn classpath(&self) -> &ClassPath {
        self.classpath.get_or_init(ClassPath::from_environment)
    }

    /// The `Kind::Java` index for a class internal name, adding it if needed.
    fn java_index(&self, binary: &str) -> usize {
        self.java_generic(binary, Vec::new())
    }

    /// The `Kind::Java` index for a class with type arguments, such as
    /// `ArrayList<Text>`, adding it if needed.
    fn java_generic(&self, binary: &str, arguments: Vec<Kind>) -> usize {
        let mut java = self.java.borrow_mut();
        if let Some(index) = java.iter().position(|entry| {
            !entry.declared && entry.binary == binary && entry.arguments == arguments
        }) {
            return index;
        }
        java.push(JavaEntry {
            binary: binary.to_owned(),
            arguments,
            declared: false,
        });
        java.len() - 1
    }

    /// The type arguments of a Java type, such as `[Text]` for `ArrayList<Text>`.
    fn type_arguments(&self, kind: Kind) -> Vec<Kind> {
        match kind.non_null() {
            Kind::Java(index) => self.java.borrow()[index].arguments.clone(),
            Kind::Applied(index) | Kind::AppliedInterface(index) => {
                self.applications.borrow()[index].arguments.clone()
            }
            Kind::MutableList(list) => vec![list.element()],
            _ => Vec::new(),
        }
    }

    /// Finds a public Java class by simple name in `java.lang` or an imported package.
    fn implicit_java_class(&self, simple: &str) -> Option<Kind> {
        if !simple.starts_with(|ch: char| ch.is_ascii_uppercase())
            || !simple
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        std::iter::once("java/lang")
            .chain(self.packages.iter().map(String::as_str))
            .find_map(|package| {
                let binary = format!("{package}/{simple}");
                let info = self.classpath().load(&binary)?;
                info.is_public().then(|| self.java_kind(&binary))
            })
    }

    /// The Mammoth type for a Java class internal name.
    fn java_kind(&self, binary: &str) -> Kind {
        match binary {
            "java/lang/String" => Kind::Text,
            "java/lang/Object" => Kind::Any,
            _ => Kind::Java(self.java_index(binary)),
        }
    }
}

fn declare_type<'a>(types: &mut Types<'a>, name: Name<'a>, kind: Kind) -> Result<(), Error> {
    if matches!(
        name.text,
        "print" | "Text" | "Int" | "Double" | "Bool" | "Long" | "Char" | "Any"
    ) || types.names.insert(name.text, kind).is_some()
    {
        return Err(at(name, "This type name is already in use or reserved."));
    }
    Ok(())
}

/// Splits `(A, B) -> R` into its parameter types and result type.
fn split_function_type(text: &str) -> Option<(Vec<&str>, &str)> {
    let inner = text.strip_prefix('(')?;
    let mut depth = 0usize;
    let mut close = None;
    for (index, ch) in inner.char_indices() {
        match ch {
            '(' | '<' => depth += 1,
            ')' if depth == 0 => {
                close = Some(index);
                break;
            }
            ')' | '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let close = close?;
    let parameters_text = &inner[..close];
    let result = inner[close + 1..].trim_start().strip_prefix("->")?.trim();
    let mut parameters = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, ch) in parameters_text.char_indices() {
        match ch {
            '(' | '<' => depth += 1,
            ')' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parameters.push(parameters_text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if !parameters_text.trim().is_empty() {
        parameters.push(parameters_text[start..].trim());
    }
    Some((parameters, result))
}

/// The type inside parentheses that wrap all of `text`, as in
/// `((Int) -> Text)`. `None` for a function type such as `(Int) -> Text`.
fn grouped_type(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('(')?.strip_suffix(')')?;
    let mut depth = 0usize;
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return None,
            ')' => depth -= 1,
            _ => {}
        }
    }
    (depth == 0).then_some(inner)
}

fn resolve_type(types: &Types<'_>, name: Name<'_>) -> Result<Kind, Error> {
    if let Some(inner) = name.text.strip_suffix('?')
        && grouped_type(inner).is_some()
    {
        return resolve_type(
            types,
            Name {
                text: inner,
                offset: name.offset,
            },
        )?
        .nullable()
        .ok_or_else(|| at(name, "This type cannot be nullable."));
    }
    if let Some(inner) = grouped_type(name.text) {
        return resolve_type(
            types,
            Name {
                text: inner,
                offset: name.offset + 1,
            },
        );
    }
    if name.text.starts_with('(') {
        let (parameters, result) = split_function_type(name.text)
            .ok_or_else(|| at(name, "Write a function type such as (Int, Int) -> Int."))?;
        let resolve_part = |text: &str| {
            let offset = name.offset + (text.as_ptr() as usize - name.text.as_ptr() as usize);
            resolve_type(types, Name { text, offset })
        };
        let parameters = parameters
            .into_iter()
            .map(resolve_part)
            .collect::<Result<Vec<_>, _>>()?;
        let result = if result == "Unit" {
            None
        } else {
            Some(resolve_part(result)?)
        };
        let signature = FunctionType {
            parameters: parameters.clone(),
            result,
        };
        if signature.interface().is_none() {
            return Err(at(
                name,
                "This function type has too many parameters for the JVM.",
            ));
        }
        return Ok(types.function_kind(parameters, result));
    }
    if let Some(text) = name.text.strip_suffix('?') {
        return resolve_type(
            types,
            Name {
                text,
                offset: name.offset,
            },
        )?
        .nullable()
        .ok_or_else(|| at(name, "This type cannot be nullable."));
    }
    if let Some((container, element)) = name
        .text
        .strip_suffix('>')
        .and_then(|text| text.split_once('<'))
        && matches!(container.trim(), "List" | "MutableList")
    {
        let trimmed = element.trim();
        if container.trim() == "MutableList" && trimmed == "*" {
            let star = types.java_star();
            return Ok(Kind::Java(types.java_generic(ARRAY_LIST, vec![star])));
        }
        let offset = name.offset + container.len() + 1 + element.len() - element.trim_start().len();
        let element = resolve_type(
            types,
            Name {
                text: trimmed,
                offset,
            },
        )?;
        let list = ListKind::for_element(element)
            .ok_or_else(|| at(name, "List elements need a non-null value or object type."))?;
        return Ok(if container.trim() == "List" {
            Kind::List(list)
        } else {
            Kind::MutableList(list)
        });
    }
    if let Some((container, arguments)) = name
        .text
        .strip_suffix('>')
        .and_then(|text| text.split_once('<'))
    {
        let base = types
            .names
            .get(container.trim())
            .copied()
            .map(Ok)
            .unwrap_or_else(|| {
                resolve_type(
                    types,
                    Name {
                        text: container.trim(),
                        offset: name.offset,
                    },
                )
            })?;
        if let Kind::Object(class) = base {
            return types.resolve_application(class, name);
        }
        if let Kind::Interface(interface) = base {
            return types.resolve_interface_application(interface, name);
        }
        let Kind::Java(index) = base else {
            return Err(at(
                name,
                "Only Java classes such as ArrayList or HashMap take type arguments here.",
            ));
        };
        let mut depth = 0usize;
        let mut start = 0;
        let mut parts = Vec::new();
        for (position, ch) in arguments.char_indices() {
            match ch {
                '<' | '(' => depth += 1,
                '>' | ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(&arguments[start..position]);
                    start = position + 1;
                }
                _ => {}
            }
        }
        parts.push(&arguments[start..]);
        let mut resolved = Vec::with_capacity(parts.len());
        for part in parts {
            let text = part.trim();
            if text == "*" {
                resolved.push(types.java_star());
                continue;
            }
            let offset = name.offset + (text.as_ptr() as usize - name.text.as_ptr() as usize);
            let kind = resolve_type(types, Name { text, offset })?;
            if matches!(kind, Kind::Nullable(_)) {
                return Err(at(
                    Name { text, offset },
                    "Type arguments cannot be nullable here.",
                ));
            }
            resolved.push(kind);
        }
        let binary = types.java_name(index);
        if types.class_type_parameters(&binary).len() != resolved.len() {
            return Err(at(
                name,
                "This Java type needs a different number of type arguments.",
            ));
        }
        let kind = Kind::Java(types.java_generic(&binary, resolved));
        types.defer_bound_check(kind, name.offset);
        return Ok(kind);
    }
    if let Some(&Kind::Object(class)) = types.names.get(name.text)
        && !types.class_parameters.borrow()[class].is_empty()
    {
        return Err(at(
            name,
            "Give this generic class type arguments, such as Box<Int>.",
        ));
    }
    if let Some(&Kind::Interface(interface)) = types.names.get(name.text)
        && !types.interface_parameters.borrow()[interface].is_empty()
    {
        return Err(at(
            name,
            "Give this generic interface type arguments, such as Producer<Int>.",
        ));
    }
    match name.text {
        "Text" => Ok(Kind::Text),
        "Int" => Ok(Kind::Int),
        "Double" => Ok(Kind::Double),
        "Long" => Ok(Kind::Long),
        "Char" => Ok(Kind::Char),
        "Bool" => Ok(Kind::Bool),
        "Any" => Ok(Kind::Any),
        _ => types
            .names
            .get(name.text)
            .copied()
            .or_else(|| match name.text {
                "String" => Some(Kind::Text),
                "Object" => Some(Kind::Any),
                _ => types.implicit_java_class(name.text),
            })
            .ok_or_else(|| {
                let mut known: Vec<&str> = types.names.keys().copied().collect();
                known.extend([
                    "Text",
                    "Int",
                    "Long",
                    "Double",
                    "Bool",
                    "Char",
                    "Any",
                    "List",
                    "MutableList",
                ]);
                at(
                    name,
                    suggest("This type has not been defined.", name.text, &known),
                )
            }),
    }
}

fn resolve_functions<'a>(
    functions: impl IntoIterator<Item = &'a Function<'a>>,
    max_parameters: usize,
    types: &Types<'a>,
    external: bool,
) -> Result<HashMap<&'a str, Signature<'a>>, Error> {
    let mut names = HashMap::new();
    for function in functions {
        let name = function.name;
        if (!external
            && matches!(
                name.text,
                "print"
                    | "for"
                    | "fun"
                    | "let"
                    | "var"
                    | "if"
                    | "while"
                    | "return"
                    | "true"
                    | "false"
                    | "null"
                    | "and"
                    | "or"
                    | "not"
                    | "class"
                    | "interface"
            ))
            || names.contains_key(name.text)
        {
            return Err(at(
                name,
                "This function name is already in use or reserved.",
            ));
        }
        if name.text.len() >= u16::MAX as usize {
            return Err(at(name, "This function name is too long for the JVM."));
        }
        let signature = native_generics::signature(function, max_parameters, types)?;
        if external {
            signature.check_java(name)?;
        }
        let mut signature = signature;
        signature.vararg = function.vararg;
        names.insert(name.text, signature);
    }
    Ok(names)
}

#[derive(Clone)]
struct JavaTarget<'a> {
    class: String,
    member: &'a str,
}

impl<'a> JavaTarget<'a> {
    fn parse(target: Name<'a>, message: &'static str) -> Result<Self, Error> {
        let invalid = || at(target, message);
        let (class, member) = target.text.rsplit_once('.').ok_or_else(invalid)?;
        if member.len() > u16::MAX as usize || !java_identifier(member) {
            return Err(invalid());
        }
        Ok(Self {
            class: java_class_name(class).ok_or_else(invalid)?,
            member,
        })
    }
}

fn java_class_name(name: &str) -> Option<String> {
    (name.contains('.') && name.len() <= u16::MAX as usize && name.split('.').all(java_identifier))
        .then(|| name.replace('.', "/"))
}

fn java_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    let start = |byte: u8| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$');
    bytes.next().is_some_and(start) && bytes.all(|byte| start(byte) || byte.is_ascii_digit())
}

#[derive(Clone)]
struct Signature<'a> {
    type_parameters: Vec<(Name<'a>, Kind)>,
    parameters: Vec<(Name<'a>, Kind)>,
    result: Option<Kind>,
    descriptor: String,
    slots: u16,
    /// Whether the last parameter was declared `vararg`.
    vararg: bool,
}

impl<'a> Signature<'a> {
    fn resolve(
        name: Name<'a>,
        parameters: &[(Name<'a>, Name<'a>)],
        result: Option<Name<'a>>,
        max_parameters: usize,
        types: &Types<'_>,
    ) -> Result<Self, Error> {
        if parameters.len() > max_parameters {
            return Err(at(name, "This parameter list is too long for the JVM."));
        }
        let kind = |name| resolve_type(types, name);
        let mut names = HashSet::with_capacity(parameters.len());
        let mut resolved = Vec::with_capacity(parameters.len());
        let mut descriptor = String::from("(");
        let mut slots = 0;
        for &(parameter, annotation) in parameters {
            if !names.insert(parameter.text) {
                return Err(at(parameter, "This parameter name is already in use."));
            }
            let kind = kind(annotation)?;
            slots += kind.slots();
            descriptor.push_str(&kind.descriptor(types));
            resolved.push((parameter, kind));
        }
        if slots as usize > max_parameters {
            return Err(at(name, "This parameter list uses too many JVM slots."));
        }
        descriptor.push(')');
        let result = result.map(kind).transpose()?;
        descriptor.push_str(&result.map_or(Cow::Borrowed("V"), |kind| kind.descriptor(types)));
        if descriptor.len() > u16::MAX as usize {
            return Err(at(name, "This signature is too long for the JVM."));
        }
        Ok(Self {
            type_parameters: Vec::new(),
            parameters: resolved,
            result,
            descriptor,
            slots,
            vararg: false,
        })
    }

    fn parameter_slots(&self, first: u16) -> impl Iterator<Item = (u16, Kind)> + '_ {
        self.parameters.iter().scan(first, |slot, &(_, kind)| {
            let current = *slot;
            *slot += kind.slots();
            Some((current, kind))
        })
    }

    fn check_java(&self, name: Name<'_>) -> Result<(), Error> {
        if !self.type_parameters.is_empty() {
            return Err(at(
                name,
                "Java declarations cannot introduce Mammoth type parameters.",
            ));
        }
        if self
            .parameters
            .iter()
            .any(|(_, kind)| !kind.is_java_value())
            || self.result.is_some_and(|kind| !kind.is_java_value())
        {
            return Err(at(
                name,
                "Java bindings accept Text, Int, Double, Bool, and declared Java classes.",
            ));
        }
        Ok(())
    }
}

impl Types<'_> {
    fn entry_class(&self) -> &str {
        self.module.unwrap_or("Main")
    }

    fn binary_name(&self, kind: Kind) -> String {
        match kind {
            Kind::Object(index) => self.native_names.classes[index].clone(),
            Kind::Applied(index) => {
                self.native_names.classes[self.applications.borrow()[index].class].clone()
            }
            Kind::Interface(index) => self.native_names.interfaces[index].clone(),
            Kind::AppliedInterface(index) => {
                self.native_names.interfaces[self.applications.borrow()[index].class].clone()
            }
            _ => unreachable!("only Mammoth classes and interfaces have generated names"),
        }
    }
}

fn emit_interface<'a>(
    program: &'a Program<'a>,
    symbols: &'a Symbols<'a>,
    index: usize,
) -> Result<ClassFile, Error> {
    let mut pool = Pool {
        count: 1,
        bytes: Vec::new(),
        members: HashMap::new(),
    };
    let name = symbols.types.binary_name(Kind::Interface(index));
    let this_class = pool.class(&name);
    let object_class = pool.class("java/lang/Object");
    let mut emitter = new_emitter(symbols, pool, this_class, None);
    emitter.line_starts = &program.line_starts;
    emitter.main_class = None;
    let parameter_kinds: Vec<Kind> = symbols.types.interface_parameters.borrow()[index]
        .iter()
        .map(|&(_, kind)| kind)
        .collect();
    let self_kind = if parameter_kinds.is_empty() {
        Kind::Interface(index)
    } else {
        symbols.types.apply_interface(index, parameter_kinds)
    };
    emitter.current_interface = Some(self_kind);
    let (code_name, stack_map_name) = (emitter.code_name, emitter.stack_map_name);
    let mut methods = Vec::new();
    let mut bridge_count = 0;
    let interface = symbols.interface_declarations[index];
    for method in &interface.methods {
        emitter.type_scope = symbols.types.interface_scope(index);
        for &(name, kind) in &symbols.interfaces[index][method.name.text].type_parameters {
            emitter.type_scope.to_mut().names.insert(name.text, kind);
        }
        if !method.is_abstract {
            let signature = &symbols.interfaces[index][method.name.text];
            let body_signature = symbols.interface_body_signature(index, method.name.text);
            let mut code = Method::new(Vec::new());
            emitter.mark_line(&mut code, method.name.offset);
            code.result = signature.result;
            code.boxed_result = signature.result != body_signature.result;
            code.local(Local::Object(this_class));
            code.variables.push(Binding {
                name: "this",
                slot: 0,
                kind: self_kind,
                mutable: false,
            });
            emitter.bind_parameters(&mut code, &body_signature);
            for (position, &(_, original)) in signature.parameters.iter().enumerate() {
                let binding = code.variables[position + 1];
                if binding.kind != original {
                    code.access(binding.kind.load(), binding.slot);
                    emitter.cast_top(&mut code, binding.kind, original, 0);
                    let slot = code.local(emitter.local_type(original));
                    code.access(original.store(), slot);
                    code.variables[position + 1].slot = slot;
                    code.variables[position + 1].kind = original;
                    code.variables_low = code.variables_low.min(position + 1);
                }
            }
            if emitter.block(&mut code, &method.body)? {
                if signature.result.is_some() {
                    return Err(at(
                        method.name,
                        "This function must return a value on every path.",
                    ));
                }
                code.code.push(0xb1);
            }
            let name = emitter.pool.ascii(&format!("${}", method.name.text));
            let descriptor = emitter.pool.ascii(&body_signature.descriptor);
            let start = methods.len();
            code.write(
                &mut methods,
                0x0001,
                name,
                descriptor,
                code_name,
                stack_map_name,
            )?;
            emitter.method_signature(&mut methods, start, &body_signature, method.name)?;
            emitter.annotate_method(&mut methods, start, &method.annotations)?;
            for erased in symbols.interface_bridges(index, method.name.text) {
                emitter.native_bridge(
                    &mut methods,
                    self_kind,
                    this_class,
                    method.name.text,
                    &body_signature,
                    &erased,
                    code_name,
                    stack_map_name,
                )?;
                bridge_count += 1;
            }
            continue;
        }
        let name = emitter.pool.ascii(&format!("${}", method.name.text));
        let descriptor = emitter
            .pool
            .ascii(&symbols.interfaces[index][method.name.text].descriptor);
        let signature = &symbols.interfaces[index][method.name.text];
        let start = methods.len();
        let generic = !signature.type_parameters.is_empty()
            || signature
                .parameters
                .iter()
                .map(|(_, kind)| *kind)
                .chain(signature.result)
                .any(|kind| symbols.types.needs_signature(kind));
        shorts(
            &mut methods,
            &[0x0401, name, descriptor, u16::from(generic)],
        ); // public abstract
        if generic {
            let text = symbols.types.generic_signature(signature);
            if text.len() > u16::MAX as usize {
                return Err(at(
                    method.name,
                    "This generic signature is too long for the JVM.",
                ));
            }
            let attribute = emitter.pool.ascii("Signature");
            let signature = emitter.pool.ascii(&text);
            shorts(&mut methods, &[attribute]);
            word(&mut methods, 2);
            shorts(&mut methods, &[signature]);
        }
        emitter.annotate_method(&mut methods, start, &method.annotations)?;
    }
    if emitter.pool.count > u16::MAX as usize {
        return Err(at(
            interface.name,
            "This interface exceeds the JVM constant pool limit.",
        ));
    }
    let parameters = symbols.types.interface_parameters.borrow()[index].clone();
    let parents = &symbols.interface_parents[index];
    let parent_classes: Vec<u16> = parents
        .iter()
        .map(|&parent| {
            let name = match parent {
                Kind::Java(_) => symbols.types.runtime_class_name(parent).unwrap(),
                _ => symbols.types.binary_name(parent),
            };
            emitter.pool.class(&name)
        })
        .collect();
    let class_signature = if parameters.is_empty()
        && !parents
            .iter()
            .any(|&parent| symbols.types.needs_signature(parent))
    {
        None
    } else {
        let mut text = String::new();
        if !parameters.is_empty() {
            text.push('<');
            for &(name, kind) in &parameters {
                text.push_str(&symbols.types.formal_parameter(name.text, kind));
            }
            text.push('>');
        }
        text.push_str("Ljava/lang/Object;");
        for &parent in parents {
            text.push_str(&symbols.types.signature_type(parent, true));
        }
        Some((emitter.pool.ascii("Signature"), emitter.pool.ascii(&text)))
    };
    if emitter.pool.count > u16::MAX as usize {
        return Err(at(
            interface.name,
            "This interface exceeds the JVM constant pool limit.",
        ));
    }
    emitter.type_scope = symbols.types.interface_scope(index);
    let (annotation_count, annotation_bytes) =
        emitter.annotation_attributes(&interface.annotations, "TYPE")?;
    let bootstrap_name =
        (!emitter.bootstraps.is_empty()).then(|| emitter.pool.ascii("BootstrapMethods"));
    methods.extend(std::mem::take(&mut emitter.synthetic_methods));
    let method_count = interface.methods.len() + bridge_count + emitter.synthetic_count as usize;
    let bootstraps = std::mem::take(&mut emitter.bootstraps);
    let source_file = (!program.source_name.is_empty()).then(|| {
        (
            emitter.pool.ascii("SourceFile"),
            emitter.pool.ascii(&program.source_name),
        )
    });
    let pool = emitter.pool;
    if pool.count > u16::MAX as usize {
        return Err(at(
            interface.name,
            "This interface exceeds the JVM constant pool limit.",
        ));
    }
    let mut bytes = Vec::with_capacity(pool.bytes.len() + methods.len() + 32);
    word(&mut bytes, 0xcafebabe);
    shorts(&mut bytes, &[0, 61, pool.count as u16]);
    bytes.extend(pool.bytes);
    shorts(
        &mut bytes,
        &[
            0x0601,
            this_class,
            object_class,
            parent_classes.len() as u16,
        ],
    );
    shorts(&mut bytes, &parent_classes);
    shorts(&mut bytes, &[0, method_count as u16]);
    bytes.extend(methods);
    shorts(
        &mut bytes,
        &[u16::from(bootstrap_name.is_some())
            + u16::from(class_signature.is_some())
            + u16::from(source_file.is_some())
            + annotation_count],
    );
    if let Some(attribute) = bootstrap_name {
        shorts(&mut bytes, &[attribute]);
        let length: usize = 2 + bootstraps
            .iter()
            .map(|entry| 2 + 2 * entry.len())
            .sum::<usize>();
        word(&mut bytes, length as u32);
        shorts(&mut bytes, &[bootstraps.len() as u16]);
        for entry in &bootstraps {
            shorts(&mut bytes, &[entry[0], (entry.len() - 1) as u16]);
            shorts(&mut bytes, &entry[1..]);
        }
    }
    if let Some((attribute, signature)) = class_signature {
        shorts(&mut bytes, &[attribute]);
        word(&mut bytes, 2);
        shorts(&mut bytes, &[signature]);
    }
    bytes.extend(annotation_bytes);
    if let Some((attribute, file)) = source_file {
        shorts(&mut bytes, &[attribute]);
        word(&mut bytes, 2);
        shorts(&mut bytes, &[file]);
    }
    Ok(ClassFile { name, bytes })
}

/// Creates an emitter that writes code into the class `this_class`, which is
/// Mammoth class `owner`, or the entry class when `owner` is `None`.
fn new_emitter<'a>(
    symbols: &'a Symbols<'a>,
    mut pool: Pool,
    this_class: u16,
    owner: Option<usize>,
) -> Emitter<'a> {
    let system = pool.class("java/lang/System");
    let out = pool.member(9, system, "out", "Ljava/io/PrintStream;");
    let stream = pool.class("java/io/PrintStream");
    let string = pool.class("java/lang/String");
    let array = pool.class("[Ljava/lang/String;");
    let builder = pool.class("java/lang/StringBuilder");
    let refs = References {
        out,
        stream,
        string,
        array,
        builder,
        constructor: pool.member(10, builder, "<init>", "()V"),
        to_string: pool.member(10, builder, "toString", "()Ljava/lang/String;"),
    };
    let code_name = pool.ascii("Code");
    let stack_map_name = pool.ascii("StackMapTable");
    Emitter {
        pool,
        this_class,
        code_name,
        stack_map_name,
        synthetic_methods: Vec::new(),
        synthetic_count: 0,
        nullable_parsers: HashMap::new(),
        lambda_count: 0,
        bootstraps: Vec::new(),
        metafactory: None,
        refs,
        symbols,
        type_scope: owner.map_or(Cow::Borrowed(&symbols.types), |class| {
            symbols.types.class_scope(class)
        }),
        infer_lambda_result: false,
        main_class: if owner.is_none() {
            Some(this_class)
        } else {
            None
        },
        object_classes: owner
            .into_iter()
            .map(|index| (Kind::Object(index), this_class))
            .collect(),
        members: HashMap::new(),
        java_fields: HashMap::new(),
        null_guard: None,
        fields: HashMap::new(),
        strings: HashMap::new(),
        integers: HashMap::new(),
        doubles: HashMap::new(),
        longs: HashMap::new(),
        runtime_classes: HashMap::new(),
        runtime_members: HashMap::new(),
        field_members: HashMap::new(),
        global_members: HashMap::new(),
        allow_null: false,
        generic_receiver: None,
        last_generic_result: None,
        current_class: None,
        this_ready: true,
        current_interface: None,
        line_starts: &[],
        line_table_name: None,
        bridge_offset: None,
        candidate_depth: 0,
        explicit_java_arguments: Vec::new(),
        in_extension: false,
        inner_ready: false,
        pending_fields: Vec::new(),
        pending_statics: Vec::new(),
    }
}

fn emit_class<'a>(
    program: &'a Program<'a>,
    symbols: &'a Symbols<'a>,
    owner: Option<usize>,
) -> Result<ClassFile, Error> {
    let mut pool = Pool {
        count: 1,
        bytes: Vec::new(),
        members: HashMap::new(),
    };
    let name = owner.map_or_else(
        || symbols.types.entry_class().to_owned(),
        |index| symbols.types.binary_name(Kind::Object(index)),
    );
    let this_class = pool.class(&name);
    let object_class = pool.class("java/lang/Object");
    let mut emitter = new_emitter(symbols, pool, this_class, owner);
    emitter.line_starts = &program.line_starts;
    let (code_name, stack_map_name) = (emitter.code_name, emitter.stack_map_name);
    let array = emitter.refs.array;
    emitter.current_class = owner;
    let mut generated_methods = 0usize;
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    let mut super_class = object_class;
    let (field_count, functions, access) = if let Some(index) = owner {
        let class = symbols.class_declarations[index];
        let signature = &symbols.constructors[index];
        // Fields declared here and inherited fields that are not private.
        for (owner_class, field) in symbols.ancestors(index).flat_map(|class| {
            symbols.class_fields[class]
                .iter()
                .map(move |field| (class, field))
        }) {
            if (owner_class != index && field.private) || emitter.fields.contains_key(field.name) {
                continue;
            }
            let reference = emitter.pool.member(
                9,
                this_class,
                field.name,
                &field.kind.descriptor(&symbols.types),
            );
            let map = symbols.ancestor_map(index, owner_class);
            emitter.fields.insert(
                field.name,
                FieldRef {
                    index: reference,
                    kind: symbols.types.replace_parameters(field.kind, &map),
                    erased: field.kind,
                    mutable: field.mutable,
                    lateinit: field.lateinit,
                },
            );
        }
        emitter.type_scope = symbols.types.class_scope(index);
        for (field, declaration) in symbols.class_fields[index].iter().zip(&class.fields) {
            let name_index = emitter.pool.ascii(field.name);
            let type_index = emitter.pool.ascii(&field.kind.descriptor(&symbols.types));
            let mut access = if field.private && !symbols.has_inner_classes(class) {
                0x0002
            } else if field.private {
                0 // Reached from its inner classes in this package.
            } else if field.protected {
                0x0004
            } else {
                0x0001
            };
            if !field.mutable {
                access |= 0x0010; // final
            }
            let field_start = fields.len();
            shorts(&mut fields, &[access, name_index, type_index, 0]);
            if symbols.types.needs_signature(field.kind) {
                let text = symbols.types.signature_type(field.kind, false);
                emitter.signature_attribute(&mut fields, field_start, &text, class.name)?;
            }
            emitter.annotate_field(&mut fields, field_start, &declaration.annotations)?;
        }
        let mut constructor = Method::new(vec![Local::UninitializedThis]);
        emitter.mark_line(&mut constructor, class.name.offset);
        let is_enum = class.enum_values.is_some();
        if is_enum {
            constructor.local(Local::Object(emitter.refs.string));
            constructor.local(Local::Int);
        }
        emitter.bind_parameters(&mut constructor, signature);
        constructor.access(0x19, 0);
        constructor.max_stack = 1;
        emitter.this_ready = false;
        let super_constructor = if is_enum {
            constructor.access(0x19, 1);
            constructor.access(0x15, 2);
            constructor.max_stack = 3;
            super_class = emitter.runtime_class("java/lang/Enum");
            emitter.runtime_member("java/lang/Enum", "<init>", "(Ljava/lang/String;I)V", 10)
        } else if let (Some((parent_name, args)), Some(java)) =
            (&class.extends, symbols.java_supers[index])
        {
            let binary = symbols.types.java_name(java);
            let candidates = symbols.types.java_super_constructors(&binary);
            constructor.stack.push(Local::UninitializedThis);
            let chosen =
                emitter.java_arguments(&mut constructor, *parent_name, &candidates, args, 1)?;
            constructor.stack.clear();
            super_class = emitter.pool_class(&binary);
            emitter
                .pool
                .member_for(10, super_class, "<init>", &chosen.descriptor)
        } else if let Some((parent_name, args)) = &class.extends {
            let parent = symbols.superclasses[index].expect("superclass was resolved");
            constructor.stack.push(Local::UninitializedThis);
            let parent_signature =
                emitter.constructor_for(&mut constructor, parent, *parent_name, args, false)?;
            let args = emitter.complete_args(
                &mut constructor,
                *parent_name,
                parent_signature,
                args,
                DefaultHost::Constructor(parent),
            )?;
            if args.len() != parent_signature.parameters.len() {
                return Err(at(
                    *parent_name,
                    "This call has the wrong number of arguments.",
                ));
            }
            let map = symbols.superclass_kinds[index]
                .map(|kind| symbols.types.class_map(kind))
                .unwrap_or_default();
            emitter.adapted_arguments(&mut constructor, args, 1, parent_signature, &map)?;
            constructor.max_stack = constructor.max_stack.max(1 + parent_signature.slots);
            constructor.stack.clear();
            super_class = emitter.object_class(Kind::Object(parent));
            emitter.member(
                Some(Kind::Object(parent)),
                "<init>",
                &parent_signature.descriptor,
            )
        } else {
            emitter.pool.member(10, object_class, "<init>", "()V")
        };
        instruction(&mut constructor.code, 0xb7, super_constructor);
        emitter.this_ready = true;
        constructor.locals[0] = Local::Object(this_class);
        constructor.variables.push(Binding {
            name: "this",
            slot: 0,
            kind: symbols.types.self_kind(index),
            mutable: false,
        });
        // Reading a field before its starting value runs would see null or
        // zero, so initializers and init blocks may only read earlier ones.
        emitter.pending_fields = class
            .fields
            .iter()
            .filter(|field| field.value.is_some())
            .map(|field| field.name.text)
            .collect();
        for initializer in &class.initializers {
            match initializer {
                Initializer::Field(position) => {
                    let field = &class.fields[*position];
                    let Some(value) = &field.value else { continue };
                    emitter.mark_line(&mut constructor, value.offset());
                    let reference = emitter.fields[field.name.text];
                    constructor.access(0x19, 0);
                    constructor.stack.push(Local::Object(this_class));
                    let kind =
                        emitter.expression_as(&mut constructor, value, 1, Some(reference.kind))?;
                    emitter.coerce(&mut constructor, value, kind, reference.kind, 1)?;
                    constructor.stack.pop();
                    instruction(&mut constructor.code, 0xb5, reference.index); // putfield
                    emitter
                        .pending_fields
                        .retain(|pending| *pending != field.name.text);
                }
                Initializer::Block(statements) => {
                    if !emitter.scoped_block(&mut constructor, statements, &[])? {
                        return Err(Error {
                            offset: statements.last().map_or(0, Statement::offset),
                            message: "An init block cannot return.",
                        });
                    }
                }
            }
        }
        emitter.pending_fields.clear();
        constructor.code.push(0xb1);
        let init = emitter.pool.ascii("<init>");
        let (descriptor, access) = if is_enum {
            (
                format!("(Ljava/lang/String;I{}", &signature.descriptor[1..]),
                0x0002, // private
            )
        } else {
            let access = if class.no_primary || class.private_primary {
                0x0002
            } else {
                0x0001
            };
            (signature.descriptor.clone(), access)
        };
        let kind = emitter.pool.ascii(&descriptor);
        let constructor_start = methods.len();
        constructor.write(&mut methods, access, init, kind, code_name, stack_map_name)?;
        emitter.method_signature(&mut methods, constructor_start, signature, class.name)?;
        generated_methods += emitter.secondary_constructors(
            index,
            class,
            &mut methods,
            init,
            code_name,
            stack_map_name,
        )?;
        let mut field_count = class.fields.len();
        if is_enum {
            let (extra_fields, extra_methods) =
                emitter.enum_members(index, class, &mut fields, &mut methods)?;
            field_count += extra_fields;
            generated_methods += extra_methods;
        }
        if class.is_data {
            generated_methods += emitter.data_members(index, class, &mut methods)?;
        }
        let (static_fields, static_methods) =
            emitter.static_members(index, class, &mut fields, &mut methods)?;
        field_count += static_fields;
        generated_methods += static_methods;
        (field_count, class.methods.as_slice(), 0x0001)
    } else {
        for global in &symbols.globals {
            let name_index = emitter.pool.ascii(global.name);
            let type_index = emitter.pool.ascii(&global.kind.descriptor(&symbols.types));
            shorts(&mut fields, &[0x0009, name_index, type_index, 0]); // public static
        }
        let mut main = Method::new(vec![Local::Object(array)]);
        if emitter.block(&mut main, &program.body)? {
            main.code.push(0xb1);
        }
        let main_name = emitter.pool.ascii("main");
        let main_type = emitter.pool.ascii("([Ljava/lang/String;)V");
        main.write(
            &mut methods,
            0x0009,
            main_name,
            main_type,
            code_name,
            stack_map_name,
        )?;
        (symbols.globals.len(), program.functions.as_slice(), 0x0008) // package-private static
    };
    let mut bridge_count = 0;
    let program_lines = emitter.line_starts;
    for function in functions {
        // The built-in library has no lines in this program's source.
        emitter.line_starts = if function.in_prelude {
            &[]
        } else {
            program_lines
        };
        emitter.in_extension = function.extension_of.is_some();
        emitter.bridge_offset = Some(function.name.offset);
        let signature = &symbols.functions_for(owner)[function.name.text];
        let plain_entry = owner.is_some_and(|index| {
            !function.private
                && (function.exported
                    || symbols.has_java_callback(index, function.name.text)
                    || is_object_bridge(function.name.text, signature))
        });
        let java_entry = owner.is_some_and(|index| {
            !function.private && !symbols.java_bridges(index, function.name.text).is_empty()
        });
        emitter.type_scope = owner.map_or(Cow::Borrowed(&symbols.types), |class| {
            symbols.types.class_scope(class)
        });
        for &(name, kind) in &signature.type_parameters {
            emitter.type_scope.to_mut().names.insert(name.text, kind);
        }
        if function.is_abstract {
            let name = emitter.pool.ascii(&format!("${}", function.name.text));
            let kind = emitter.pool.ascii(&signature.descriptor);
            let start = methods.len();
            let visibility = if function.protected { 0x0004 } else { 0x0001 };
            shorts(&mut methods, &[0x0400 | visibility, name, kind, 0]); // abstract
            emitter.method_signature(&mut methods, start, signature, function.name)?;
            emitter.annotate_method(&mut methods, start, &function.annotations)?;
            continue;
        }
        let mut method = Method::new(Vec::new());
        emitter.mark_line(&mut method, function.name.offset);
        method.result = signature.result;
        if let Some(index) = owner {
            method.local(Local::Object(this_class));
            method.variables.push(Binding {
                name: "this",
                slot: 0,
                kind: symbols.types.self_kind(index),
                mutable: false,
            });
        }
        emitter.bind_parameters(&mut method, signature);
        if function.exported && owner.is_none() {
            emitter.check_java_parameters(&mut method, signature, 0);
        }
        if emitter.block(&mut method, &function.body)? {
            if signature.result.is_some() {
                return Err(at(
                    function.name,
                    "This function must return a value on every path.",
                ));
            }
            method.code.push(0xb1);
        }
        let name = if owner.is_some() {
            emitter.pool.ascii(&format!("${}", function.name.text))
        } else {
            emitter.pool.ascii(function.name.text)
        };
        let kind = emitter.pool.ascii(&signature.descriptor);
        let access = if function.exported {
            access | 0x0001
        } else if function.private
            && owner
                .is_some_and(|index| !symbols.has_inner_classes(symbols.class_declarations[index]))
        {
            0x0002
        } else if function.private && owner.is_some() {
            0 // Reached from its inner classes in this package.
        } else if function.protected {
            0x0004
        } else {
            access
        };
        let method_start = methods.len();
        method.write(&mut methods, access, name, kind, code_name, stack_map_name)?;
        emitter.method_signature(&mut methods, method_start, signature, function.name)?;
        if !plain_entry && !java_entry {
            emitter.annotate_method(&mut methods, method_start, &function.annotations)?;
        }
        if let Some(index) = owner
            && !function.private
            && (function.exported
                || symbols.has_java_callback(index, function.name.text)
                || is_object_bridge(function.name.text, signature))
        {
            let mut bridge = Method::new(vec![Local::Object(this_class)]);
            emitter.mark_line(&mut bridge, function.name.offset);
            emitter.bind_parameters(&mut bridge, signature);
            emitter.check_java_parameters(&mut bridge, signature, 1);
            bridge.access(0x19, 0);
            for (slot, kind) in signature.parameter_slots(1) {
                bridge.access(kind.load(), slot);
            }
            bridge.max_stack = bridge
                .max_stack
                .max(signature.slots + 1)
                .max(signature.result.map_or(0, Kind::slots));
            let target = emitter.member(
                Some(Kind::Object(index)),
                function.name.text,
                &signature.descriptor,
            );
            instruction(&mut bridge.code, 0xb6, target); // invokevirtual
            let opcode = match signature.result {
                Some(kind) => kind.return_opcode(),
                None => 0xb1,
            };
            bridge.code.push(opcode);
            let name = emitter.pool.ascii(function.name.text);
            let start = methods.len();
            bridge.write(&mut methods, 0x0001, name, kind, code_name, stack_map_name)?;
            emitter.method_signature(&mut methods, start, signature, function.name)?;
            emitter.annotate_method(&mut methods, start, &function.annotations)?;
            bridge_count += 1;
        }
        if let Some(index) = owner
            && !function.private
        {
            for erased in symbols.native_bridges(index, function.name.text) {
                emitter.native_bridge(
                    &mut methods,
                    Kind::Object(index),
                    this_class,
                    function.name.text,
                    signature,
                    &erased,
                    code_name,
                    stack_map_name,
                )?;
                bridge_count += 1;
            }
            for required in symbols.java_bridges(index, function.name.text) {
                let duplicate = required.descriptor == signature.descriptor
                    && (function.exported
                        || symbols.has_java_callback(index, function.name.text)
                        || is_object_bridge(function.name.text, signature));
                if duplicate {
                    continue;
                }
                let start = methods.len();
                emitter.java_bridge(
                    &mut methods,
                    index,
                    this_class,
                    function.name.text,
                    signature,
                    &required,
                    code_name,
                    stack_map_name,
                )?;
                emitter.annotate_method(&mut methods, start, &function.annotations)?;
                bridge_count += 1;
            }
        }
    }
    emitter.line_starts = program_lines;
    if let Some(index) = owner {
        emitter.bridge_offset = Some(symbols.class_declarations[index].name.offset);
        for name in symbols.inherited_bridge_names(index) {
            let Some((_, signature)) = symbols.find_method(index, name) else {
                continue;
            };
            for erased in symbols.native_bridges(index, name) {
                emitter.native_bridge(
                    &mut methods,
                    Kind::Object(index),
                    this_class,
                    name,
                    signature,
                    &erased,
                    code_name,
                    stack_map_name,
                )?;
                bridge_count += 1;
            }
        }
    }
    let interfaces: Vec<_> = owner
        .into_iter()
        .flat_map(|index| &symbols.implementations[index])
        .map(|&kind| emitter.object_class(kind))
        .collect();
    let class_signature = if let Some(index) = owner.filter(|&index| {
        !symbols.types.class_parameters.borrow()[index].is_empty()
            || symbols.implementations[index]
                .iter()
                .chain(symbols.superclass_kinds[index].iter())
                .any(|&kind| symbols.types.contains_application(kind))
            || symbols.java_supers[index]
                .is_some_and(|java| symbols.types.needs_signature(Kind::Java(java)))
    }) {
        let text = symbols.class_signature(index);
        if text.len() > u16::MAX as usize {
            return Err(at(
                symbols.class_declarations[index].name,
                "This generic signature is too long for the JVM.",
            ));
        }
        Some((emitter.pool.ascii("Signature"), emitter.pool.ascii(&text)))
    } else {
        None
    };
    let annotations = owner.map_or(&[][..], |index| {
        symbols.class_declarations[index].annotations.as_slice()
    });
    emitter.type_scope = owner.map_or(Cow::Borrowed(&symbols.types), |index| {
        symbols.types.class_scope(index)
    });
    let (annotation_count, annotation_bytes) =
        emitter.annotation_attributes(annotations, "TYPE")?;
    let bootstrap_name =
        (!emitter.bootstraps.is_empty()).then(|| emitter.pool.ascii("BootstrapMethods"));
    methods.extend(std::mem::take(&mut emitter.synthetic_methods));
    let synthetic_count = emitter.synthetic_count as usize;
    let bootstraps = std::mem::take(&mut emitter.bootstraps);
    let source_file = (!program.source_name.is_empty()).then(|| {
        (
            emitter.pool.ascii("SourceFile"),
            emitter.pool.ascii(&program.source_name),
        )
    });
    let pool = emitter.pool;
    if pool.count > u16::MAX as usize {
        return Err(Error {
            offset: 0,
            message: "This program exceeds the JVM constant pool limit.",
        });
    }
    let mut bytes = Vec::with_capacity(pool.bytes.len() + methods.len() + fields.len() + 32);
    word(&mut bytes, 0xcafebabe);
    shorts(&mut bytes, &[0, 61, pool.count as u16]);
    bytes.extend(pool.bytes);
    let class_access = match owner {
        Some(index) if symbols.class_declarations[index].enum_values.is_some() => 0x4031, // public final super enum
        Some(index) if symbols.class_declarations[index].is_abstract => 0x0421, // public abstract super
        Some(_) => 0x0021,                                                      // public super
        None => 0x0031, // public final super
    };
    shorts(
        &mut bytes,
        &[
            class_access,
            this_class,
            super_class,
            interfaces.len() as u16,
        ],
    );
    shorts(&mut bytes, &interfaces);
    shorts(&mut bytes, &[field_count as u16]);
    bytes.extend(fields);
    shorts(
        &mut bytes,
        &[(functions.len() + 1 + bridge_count + synthetic_count + generated_methods) as u16],
    );
    bytes.extend(methods);
    shorts(
        &mut bytes,
        &[u16::from(bootstrap_name.is_some())
            + u16::from(class_signature.is_some())
            + u16::from(source_file.is_some())
            + annotation_count],
    );
    if let Some(attribute) = bootstrap_name {
        shorts(&mut bytes, &[attribute]);
        let length: usize = 2 + bootstraps
            .iter()
            .map(|entry| 2 + 2 * entry.len())
            .sum::<usize>();
        word(&mut bytes, length as u32);
        shorts(&mut bytes, &[bootstraps.len() as u16]);
        for entry in &bootstraps {
            shorts(&mut bytes, &[entry[0], (entry.len() - 1) as u16]);
            shorts(&mut bytes, &entry[1..]);
        }
    }
    if let Some((attribute, signature)) = class_signature {
        shorts(&mut bytes, &[attribute]);
        word(&mut bytes, 2);
        shorts(&mut bytes, &[signature]);
    }
    bytes.extend(annotation_bytes);
    if let Some((attribute, file)) = source_file {
        shorts(&mut bytes, &[attribute]);
        word(&mut bytes, 2);
        shorts(&mut bytes, &[file]);
    }
    Ok(ClassFile { name, bytes })
}

#[derive(Clone)]
struct References {
    out: u16,
    stream: u16,
    string: u16,
    array: u16,
    builder: u16,
    constructor: u16,
    to_string: u16,
}

#[derive(Clone)]
struct Symbols<'a> {
    types: Types<'a>,
    functions: HashMap<&'a str, Signature<'a>>,
    /// For each extension function name, the hidden functions declared for it.
    extensions: HashMap<&'a str, Vec<&'a str>>,
    /// For each overloaded name, the hidden names of its declarations.
    overloads: Rc<HashMap<&'a str, Vec<&'a str>>>,
    methods: Rc<Vec<HashMap<&'a str, Signature<'a>>>>,
    constructors: Rc<Vec<Signature<'a>>>,
    /// For each class, the constructors beyond the one in its header.
    secondary: Rc<Vec<Vec<Signature<'a>>>>,
    interfaces: Rc<Vec<HashMap<&'a str, Signature<'a>>>>,
    /// The interfaces each Mammoth interface extends, in terms of its own
    /// type parameters.
    interface_parents: Rc<Vec<Vec<Kind>>>,
    implementations: Rc<Vec<Vec<Kind>>>,
    java: HashMap<&'a str, JavaTarget<'a>>,
    imports: HashMap<&'a str, JavaTarget<'a>>,
    java_fields: HashMap<&'a str, (Kind, JavaTarget<'a>)>,
    java_classes: Rc<HashMap<usize, JavaClassRef<'a>>>,
    class_declarations: Rc<Vec<&'a Class<'a>>>,
    interface_declarations: Rc<Vec<&'a Interface<'a>>>,
    own_classes: Vec<usize>,
    own_interfaces: Vec<usize>,
    /// The Mammoth superclass of each class, if it extends one.
    superclasses: Rc<Vec<Option<usize>>>,
    /// The declared superclass type of each class, such as `Base<Int>`.
    superclass_kinds: Rc<Vec<Option<Kind>>>,
    /// The Java superclass of each class that directly extends a Java class.
    java_supers: Rc<Vec<Option<usize>>>,
    /// The value names of each enum class.
    enum_values: Rc<Vec<Option<Vec<&'a str>>>>,
    /// For each class, whether each declared method is (private, abstract).
    method_flags: Rc<Vec<HashMap<&'a str, (bool, bool)>>>,
    /// Names of the protected methods each Mammoth class declares.
    protected_methods: Rc<Vec<std::collections::HashSet<&'a str>>>,
    /// `static fun` signatures of each Mammoth class.
    static_methods: Rc<Vec<HashMap<&'a str, Signature<'a>>>>,
    /// `static var`/`static let` fields of each Mammoth class.
    static_fields: Rc<Vec<Vec<StaticField<'a>>>>,
    abstract_classes: Rc<Vec<bool>>,
    class_fields: Rc<Vec<Vec<FieldInfo<'a>>>>,
    globals: Vec<globals::Global<'a>>,
}

#[derive(Clone)]
struct FieldInfo<'a> {
    name: &'a str,
    kind: Kind,
    mutable: bool,
    private: bool,
    protected: bool,
    lateinit: bool,
}

/// Whether a Mammoth method also needs a public bridge under its plain name so that
/// Java's `toString` and `hashCode` use it.
fn is_object_bridge(name: &str, signature: &Signature<'_>) -> bool {
    signature.parameters.is_empty()
        && matches!(
            (name, signature.result),
            ("toString", Some(Kind::Text)) | ("hashCode", Some(Kind::Int))
        )
}

#[derive(Clone)]
struct JavaClassRef<'a> {
    declaration: &'a JavaClass<'a>,
    interface: bool,
    aliases: HashMap<&'a str, &'a str>,
    constructor: Option<Signature<'a>>,
    methods: HashMap<&'a str, Signature<'a>>,
}

impl<'a> Symbols<'a> {
    fn functions_for(&self, owner: Option<usize>) -> &HashMap<&'a str, Signature<'a>> {
        owner.map_or(&self.functions, |index| &self.methods[index])
    }

    fn is_interface(&self, kind: Kind) -> bool {
        match kind {
            Kind::Interface(_) | Kind::AppliedInterface(_) => true,
            Kind::Java(index) => self.java_classes.get(&index).map_or_else(
                || self.types.is_java_interface(&self.types.java_name(index)),
                |class| class.interface,
            ),
            _ => false,
        }
    }

    fn java_owner_is_interface(&self, binary: &str) -> bool {
        self.java_classes
            .iter()
            .any(|(&index, entry)| self.types.java_name(index) == binary && entry.interface)
    }

    fn has_java_callback(&self, class: usize, name: &str) -> bool {
        self.java_interfaces(class).iter().any(|kind| {
            matches!(kind, Kind::Java(index) if self.java_classes.get(index).is_some_and(|class| class.methods.contains_key(name)))
        })
    }
}

#[derive(Clone, Copy)]
struct FieldRef {
    index: u16,
    /// The field's type in the current class, with superclass type arguments
    /// substituted.
    kind: Kind,
    /// The declared (erased) type that the JVM field descriptor uses.
    erased: Kind,
    mutable: bool,
    lateinit: bool,
}

#[derive(Clone)]
struct Emitter<'a> {
    pool: Pool,
    this_class: u16,
    code_name: u16,
    stack_map_name: u16,
    /// Methods generated for lambda bodies, already encoded.
    synthetic_methods: Vec<u8>,
    synthetic_count: u16,
    nullable_parsers: HashMap<Kind, u16>,
    lambda_count: usize,
    /// BootstrapMethods entries: the bootstrap handle followed by its arguments.
    bootstraps: Vec<Vec<u16>>,
    metafactory: Option<u16>,
    refs: References,
    symbols: &'a Symbols<'a>,
    type_scope: Cow<'a, Types<'a>>,
    infer_lambda_result: bool,
    main_class: Option<u16>,
    object_classes: HashMap<Kind, u16>,
    members: HashMap<(Option<Kind>, &'a str), u16>,
    java_fields: HashMap<&'a str, u16>,
    null_guard: Option<(u16, u16)>,
    fields: HashMap<&'a str, FieldRef>,
    strings: HashMap<&'a str, u16>,
    integers: HashMap<i32, u16>,
    doubles: HashMap<u64, u16>,
    longs: HashMap<i64, u16>,
    runtime_classes: HashMap<&'static str, u16>,
    runtime_members: HashMap<(&'static str, &'static str, &'static str, u8), u16>,
    field_members: HashMap<(Kind, &'a str), u16>,
    global_members: HashMap<&'a str, u16>,
    /// Set while evaluating a value whose destination accepts null, so a Java
    /// result that may be null is not checked.
    allow_null: bool,
    /// The receiver of the Java call whose arguments are being chosen, for
    /// working out its generic types.
    generic_receiver: Option<Kind>,
    /// The generic result type of the Java method just chosen.
    last_generic_result: Option<Kind>,
    /// The Mammoth class being emitted, or `None` for the main class.
    current_class: Option<usize>,
    /// False while a constructor evaluates its superclass arguments.
    this_ready: bool,
    /// The interface type whose default method is being compiled.
    current_interface: Option<Kind>,
    /// Line start offsets of the source file being compiled.
    line_starts: &'a [usize],
    /// The constant for the `LineNumberTable` attribute name, once used.
    line_table_name: Option<u16>,
    /// The source offset that generated forwarding methods report, the
    /// declaration of the method they forward to.
    bridge_offset: Option<usize>,
    /// How many overload candidates are re-probing their arguments.
    candidate_depth: u8,
    /// Type arguments written at the Java call being resolved, as in
    /// `Collections.emptyList<Text>()`.
    explicit_java_arguments: Vec<Kind>,
    /// Whether the function being compiled is an extension function.
    in_extension: bool,
    /// The next inner class created already has its outer object as an argument.
    inner_ready: bool,
    /// Instance fields whose starting values have not run yet, while a
    /// constructor runs field initializers and init blocks.
    pending_fields: Vec<&'a str>,
    /// Static fields of a class whose starting values have not run yet,
    /// while its `<clinit>` runs.
    pending_statics: Vec<(usize, &'a str)>,
}

impl<'a> Emitter<'a> {
    /// Rejects reading an instance field whose starting value has not run.
    fn check_initialized(&self, name: Name<'_>) -> Result<(), Error> {
        if self.pending_fields.contains(&name.text) {
            return Err(at(name, FIELD_BEFORE_START));
        }
        Ok(())
    }

    /// Rejects reading a property whose hidden field has no value yet.
    fn check_property_ready(&self, name: Name<'_>) -> Result<(), Error> {
        if !self.pending_fields.is_empty() {
            let hidden = format!("__field_{}", name.text);
            if self.pending_fields.contains(&hidden.as_str()) {
                return Err(at(name, FIELD_BEFORE_START));
            }
        }
        Ok(())
    }

    /// Identifies a source position across the files of a project: the
    /// file (by its line table, which each file owns) and the offset.
    fn site(&self, name: Name<'_>) -> (usize, usize) {
        (self.line_starts.as_ptr() as usize, name.offset)
    }

    fn check_java_parameters(
        &mut self,
        method: &mut Method<'a>,
        signature: &Signature<'a>,
        base_slot: u16,
    ) {
        for (slot, kind) in signature.parameter_slots(base_slot) {
            if kind.is_reference() && kind == kind.non_null() {
                method.access(kind.load(), slot);
                self.guard_non_null(method, kind, 0);
                method.code.push(0x57); // pop checked value
            }
        }
    }

    fn bind_parameters(&mut self, method: &mut Method<'a>, signature: &Signature<'a>) {
        for &(parameter, kind) in &signature.parameters {
            let slot = method.local(self.local_type(kind));
            method.variables.push(Binding {
                name: parameter.text,
                slot,
                kind,
                mutable: false,
            });
        }
    }

    fn return_value(&mut self, method: &mut Method<'a>, result: Kind) {
        if method.boxed_result {
            self.box_value(method, result);
            method.code.push(0xb0);
        } else {
            method.code.push(result.return_opcode());
        }
    }

    fn block(
        &mut self,
        method: &mut Method<'a>,
        statements: &'a [Statement<'a>],
    ) -> Result<bool, Error> {
        let mut falls_through = true;
        for (position, statement) in statements.iter().enumerate() {
            if !falls_through {
                return Err(Error {
                    offset: statement.offset(),
                    message: "This statement cannot run because the function has already returned.",
                });
            }
            self.allow_null = false;
            self.mark_line(method, statement.offset());
            match statement {
                Statement::Return(name, expr) => {
                    if name.text == "return@" && !method.is_lambda {
                        return Err(at(*name, "Use return@name only inside a lambda."));
                    }
                    // The first returned value sets an inferred lambda's result.
                    if let Some(value) = expr
                        && std::mem::take(&mut method.infers_result)
                    {
                        let saved = method.snapshot();
                        let kind = self.expression(method, value, 0);
                        method.restore(saved);
                        method.result = Some(kind?);
                    }
                    match (expr, method.result) {
                        (Some(expr), Some(result)) => {
                            let kind = self.expression_as(method, expr, 0, Some(result))?;
                            self.coerce(method, expr, kind, result, 0)?;
                            if method.tries.iter().any(|context| context.has_finally()) {
                                let slot = method.local(self.local_type(result));
                                method.access(result.store(), slot);
                                let exiting = self.exit_tries(method, 0)?;
                                method.access(result.load(), slot);
                                self.return_value(method, result);
                                self.resume_tries(method, exiting);
                            } else {
                                self.return_value(method, result);
                            }
                        }
                        (None, None) => {
                            let exiting = self.exit_tries(method, 0)?;
                            method.code.push(0xb1); // return
                            self.resume_tries(method, exiting);
                        }
                        (Some(_), None) => {
                            return Err(at(
                                *name,
                                "Use a return value only inside a function that declares a result type.",
                            ));
                        }
                        (None, Some(_)) => {
                            return Err(at(*name, "This function must return a value here."));
                        }
                    }
                    falls_through = false;
                }
                Statement::Break(name) | Statement::Continue(name) => {
                    let is_break = matches!(statement, Statement::Break(_));
                    if method.loops.is_empty() {
                        return Err(at(*name, "Use break and continue only inside a loop."));
                    }
                    // `break@outer` leaves the loop with that label.
                    let target = match name.text.split_once('@') {
                        Some((_, label)) => method
                            .loop_labels
                            .iter()
                            .rev()
                            .find(|(_, text)| *text == label)
                            .map(|&(index, _)| index)
                            .ok_or_else(|| at(*name, "No enclosing loop has this label."))?,
                        None => method.loops.len() - 1,
                    };
                    let depth = self.try_depth_for(method, target + 1);
                    let exiting = self.exit_tries(method, depth)?;
                    let jump = method.jump();
                    self.resume_tries(method, exiting);
                    let frame = &mut method.loops[target];
                    if is_break {
                        frame.breaks.push(jump);
                    } else if let Some(target) = frame.continue_target {
                        method.patch_branch(jump, target, name.offset)?;
                    } else {
                        frame.continues.push(jump);
                    }
                    falls_through = false;
                }
                Statement::Try(name, body, catches, finally) => {
                    falls_through =
                        self.try_statement(method, *name, body, catches, finally.as_deref())?;
                }
                Statement::Throw(name, value) => {
                    self.throw(method, *name, value)?;
                    falls_through = false;
                }
                Statement::When(name, subject, branches) => {
                    let variables = method.variables.len();
                    falls_through =
                        self.when_statement(method, *name, subject.as_ref(), branches)?;
                    method.truncate_variables(variables);
                }
                Statement::MethodCall(Expr::Safe(inner)) => {
                    if let Some(kind) = self.safe_access(method, inner, 0)? {
                        method.code.push(kind.pop_opcode());
                    }
                }
                Statement::SetIndex(list, index, operator, value) => {
                    self.set_index(method, list, index, *operator, value)?;
                }
                Statement::SetField(receiver, name, operator, value) => {
                    self.set_field(method, receiver, *name, *operator, value)?;
                }
                Statement::Call(name, args) => {
                    if name.text == "print" && !self.symbols.functions.contains_key("print") {
                        self.print_call(method, *name, args)?;
                    } else if name.text == "error"
                        && !self.symbols.functions.contains_key("error")
                        && self
                            .current_class
                            .is_none_or(|class| self.symbols.find_method(class, "error").is_none())
                    {
                        self.error_call(method, *name, args)?;
                        falls_through = false;
                    } else if let Some(kind) = {
                        self.allow_null = true;
                        self.call(method, *name, args, 0)?
                    } {
                        method.code.push(kind.pop_opcode());
                    }
                }
                Statement::MethodCall(Expr::MethodCall(receiver, name, args)) => {
                    // A discarded result may be null.
                    self.allow_null = true;
                    if let Some(kind) = self.method_call(method, receiver, *name, args, 0)? {
                        method.code.push(kind.pop_opcode());
                    }
                }
                Statement::MethodCall(_) => unreachable!("parser only creates method calls here"),
                Statement::For(name, values, body) => {
                    self.loop_over(method, *name, values, body)?
                }
                Statement::Binding(name, expr, mutable, annotation) => {
                    let annotation = annotation
                        .map(|name| resolve_type(&self.type_scope, name))
                        .transpose()?;
                    let actual = self.expression_as(method, expr, 0, annotation)?;
                    let kind = if let Some(expected) = annotation {
                        self.coerce(method, expr, actual, expected, 0)?;
                        expected
                    } else {
                        if actual == Kind::Null {
                            return Err(at(
                                *name,
                                "Give this null binding a nullable type, such as Text?.",
                            ));
                        }
                        actual
                    };
                    if self.declare_global(method, *name, kind) {
                        continue;
                    }
                    if *mutable && globals::lambda_assigns(&statements[position + 1..], name.text) {
                        // A lambda changes this variable, so keep it in a
                        // one-item array that the lambda shares.
                        let list =
                            ListKind::for_element(kind).expect("variables have element types");
                        let temporary = method.local(self.local_type(kind));
                        method.access(kind.store(), temporary);
                        self.push_int32(method, 1);
                        self.new_array(method, list);
                        method.code.push(0x59); // dup
                        push_int(&mut method.code, 0);
                        method.access(kind.load(), temporary);
                        method.code.push(list.store());
                        method.max_stack = method.max_stack.max(3 + kind.slots());
                        method.locals.truncate(temporary as usize);
                        let slot = method.local(self.local_type(Kind::List(list)));
                        method.access(0x3a, slot); // astore
                        method.boxed_slots.push(slot);
                        method.variables.push(Binding {
                            name: name.text,
                            slot,
                            kind,
                            mutable: true,
                        });
                        continue;
                    }
                    let slot = method.local(self.local_type(kind));
                    method.access(kind.store(), slot);
                    method.variables.push(Binding {
                        name: name.text,
                        slot,
                        kind,
                        mutable: *mutable,
                    });
                }
                Statement::Assign(name, expr) => {
                    let local = method.binding(*name);
                    if local.is_none()
                        && !self.fields.contains_key(name.text)
                        && self.assign_property(method, *name, expr)?
                    {
                        continue;
                    }
                    let (expected, mutable, field) = if let Some(binding) = local {
                        (binding.kind, binding.mutable, None)
                    } else if self.fields.contains_key(name.text)
                        && method
                            .binding(Name {
                                text: "this",
                                offset: 0,
                            })
                            .is_none()
                        && self.current_class.is_some()
                        && self.this_ready
                    {
                        return Err(at(
                            *name,
                            "A static method cannot use the object's fields. Pass the object as a parameter.",
                        ));
                    } else if let Some(field) = self.fields.get(name.text) {
                        (field.kind, field.mutable, Some((field.index, field.erased)))
                    } else if self.symbols.java_fields.contains_key(name.text) {
                        return Err(at(*name, "A Java field binding is read-only in Mammoth."));
                    } else if let Some(global) = self.symbols.global(name.text) {
                        if !global.mutable {
                            return Err(at(
                                *name,
                                "This binding is read-only. Declare it with var to change its value.",
                            ));
                        }
                        let kind = self.expression_as(method, expr, 0, Some(global.kind))?;
                        self.coerce(method, expr, kind, global.kind, 0)?;
                        self.store_global(method, name.text);
                        continue;
                    } else if let Some(class) = self.current_class
                        && self.symbols.find_static_field(class, name.text).is_some()
                    {
                        self.write_static_field(method, class, *name, expr)?;
                        continue;
                    } else if self.assign_implicit_java_field(method, *name, expr)? {
                        continue;
                    } else if let Some(path) = self.outer_path(method, *name, false) {
                        self.set_field(method, path, *name, None, expr)?;
                        continue;
                    } else {
                        return Err(at(*name, "This variable has not been defined here."));
                    };
                    // `items += value` adds to a list held by a read-only name.
                    if !mutable
                        && let Expr::Arithmetic(left, rest) = expr
                        && let (Expr::Variable(variable), [(Arithmetic::Add, value)]) =
                            (left.as_ref(), rest.as_slice())
                        && variable.text == name.text
                        && (matches!(expected, Kind::MutableList(_))
                            || matches!(expected, Kind::Java(_)))
                    {
                        let add = Name {
                            text: "add",
                            offset: name.offset,
                        };
                        let receiver: &'a Expr<'a> = left;
                        let result = self.method_call(
                            method,
                            receiver,
                            add,
                            std::slice::from_ref(value),
                            0,
                        )?;
                        if let Some(kind) = result {
                            method.code.push(kind.pop_opcode());
                        }
                        continue;
                    }
                    if !mutable {
                        return Err(at(
                            *name,
                            if method.captured.contains(&name.text) {
                                "A lambda cannot change a local variable from outside it. Use a field or a top-level variable instead."
                            } else {
                                "This binding is read-only. Declare it with var to change its value."
                            },
                        ));
                    }
                    if let Some(binding) = local
                        && method.boxed_slots.contains(&binding.slot)
                    {
                        let list = ListKind::for_element(expected).unwrap();
                        method.access(0x19, binding.slot); // aload the shared array
                        push_int(&mut method.code, 0);
                        let array = self.local_type(Kind::List(list));
                        method.stack.extend([array, Local::Int]);
                        let kind = self.expression_as(method, expr, 2, Some(expected))?;
                        self.coerce(method, expr, kind, expected, 2)?;
                        method.stack.clear();
                        method.code.push(list.store());
                        method.max_stack = method.max_stack.max(2 + expected.slots());
                        continue;
                    }
                    if field.is_some() {
                        method.access(0x19, 0); // receiver for putfield
                        method.stack.push(method.locals[0]);
                    }
                    let base = u16::from(field.is_some());
                    let kind = self.expression_as(method, expr, base, Some(expected))?;
                    self.coerce(method, expr, kind, expected, base)?;
                    if let Some((index, erased)) = field {
                        if erased.is_reference() && !expected.is_reference() {
                            self.box_value(method, expected);
                        }
                        instruction(&mut method.code, 0xb5, index); // putfield
                        method.stack.pop();
                    } else if let Some(binding) = local {
                        method.access(expected.store(), binding.slot);
                    }
                }
                Statement::Labeled(label, inner) => {
                    method.loop_labels.push((method.loops.len(), label.text));
                    let result = self.block(method, std::slice::from_ref(inner));
                    method.loop_labels.pop();
                    falls_through = result?;
                }
                Statement::DoWhile(name, body, condition) => {
                    let header = method.code.len();
                    method.frame(method.locals.len());
                    method.loops.push(LoopFrame::new(None));
                    let continues = self.scoped_block(method, body, &[]);
                    let frame = method.loops.pop().unwrap();
                    let reaches_test = continues? || !frame.continues.is_empty();
                    if reaches_test {
                        method.patch_branches(&frame.continues, name.offset)?;
                        if !frame.continues.is_empty() {
                            method.frame(method.locals.len());
                        }
                        for jump in self.branch_when(method, condition, true)? {
                            method.patch_branch(jump, header, name.offset)?;
                        }
                    }
                    if !frame.breaks.is_empty() {
                        method.patch_branches(&frame.breaks, name.offset)?;
                        method.frame(method.locals.len());
                    } else if !reaches_test {
                        falls_through = false;
                    }
                }
                Statement::While(name, condition, body) => {
                    let header = method.code.len();
                    method.frame(method.locals.len());
                    let endless = matches!(condition, Expr::Bool(_, true));
                    let exit = if endless {
                        Vec::new()
                    } else {
                        self.branch_when(method, condition, false)?
                    };
                    let narrowed = method.non_null_facts(condition, true);
                    let types = self.type_facts(method, condition, true);
                    let type_count = method.type_facts.len();
                    method.type_facts.extend(types);
                    method.loops.push(LoopFrame::new(Some(header)));
                    let continues = self.scoped_block(method, body, &narrowed);
                    method.type_facts.truncate(type_count);
                    let frame = method.loops.pop().unwrap();
                    if continues? {
                        let back = method.jump();
                        method.patch_branch(back, header, name.offset)?;
                    }
                    if endless && frame.breaks.is_empty() {
                        // Nothing leaves the loop, so nothing can follow it.
                        falls_through = false;
                        continue;
                    }
                    method.patch_branches(&exit, name.offset)?;
                    method.patch_branches(&frame.breaks, name.offset)?;
                    method.frame(method.locals.len());
                }
                Statement::If(name, condition, body, otherwise) => {
                    falls_through =
                        self.conditional(method, *name, condition, body, otherwise.as_deref())?;
                }
            }
            if method.code.len() >= u16::MAX as usize {
                return Err(Error {
                    offset: 0,
                    message: "This block exceeds the JVM method size limit.",
                });
            }
        }
        Ok(falls_through)
    }

    fn print_call(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
    ) -> Result<(), Error> {
        let base = method.stack.len() as u16;
        instruction(&mut method.code, 0xb2, self.refs.out);
        match args {
            [] => {
                let println = self.runtime_member("java/io/PrintStream", "println", "()V", 10);
                method.max_stack = method.max_stack.max(base + 1);
                instruction(&mut method.code, 0xb6, println);
            }
            [arg] => {
                method.stack.push(Local::Object(self.refs.stream));
                self.allow_null = true;
                let kind = self.expression(method, arg, base + 1)?;
                method.stack.pop();
                self.print_value(method, kind, base + 1, "println");
            }
            _ => return Err(at(name, "print expects one value.")),
        }
        Ok(())
    }

    fn call(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        self.call_expected(method, name, args, base, None)
    }

    /// A call by bare name. `expected` is the destination's type; it reaches
    /// only an inherited Java method called on the current object.
    fn call_expected(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        if name.text == "Any" && args.is_empty() && !self.symbols.functions.contains_key("Any") {
            let object = self.runtime_class("java/lang/Object");
            let init = self.runtime_member("java/lang/Object", "<init>", "()V", 10);
            method.max_stack = method.max_stack.max(base + 2);
            instruction(&mut method.code, 0xbb, object); // new
            method.code.push(0x59); // dup
            instruction(&mut method.code, 0xb7, init); // invokespecial
            return Ok(Some(Kind::Any));
        }
        if let ("__monitorEnter" | "__monitorExit", [lock]) = (name.text, args) {
            let kind = self.expression(method, lock, base)?;
            if !kind.is_reference() || matches!(kind, Kind::Nullable(_) | Kind::Null) {
                return Err(Error {
                    offset: lock.offset(),
                    message: "Only an object that is not null can be used as a lock.",
                });
            }
            method.code.push(if name.text == "__monitorEnter" {
                0xc2
            } else {
                0xc3
            });
            return Ok(None);
        }
        if let Some(kind) = self.collection_builder(method, name, args, base, expected)? {
            return Ok(Some(kind));
        }
        if let Some(kind) = self.sized_collection(method, name, args, base)? {
            return Ok(Some(kind));
        }
        if !self.symbols.overloads.is_empty()
            && method.binding(name).is_none()
            && let Some(chosen) = self.overloaded_function(method, name, args)?
        {
            return self.call_expected(method, chosen, args, base, expected);
        }
        if let Some(class) = self.current_class
            && matches!(name.text, "values" | "valueOf")
            && self.symbols.enum_values[class].is_some()
            && !self.symbols.functions.contains_key(name.text)
        {
            return self.enum_static_call(method, class, name, args, base);
        }
        if let Some(class) = self.current_class
            && !self.symbols.functions.contains_key(name.text)
            && let Some(result) = self.static_call(method, class, name, args, base)?
        {
            return Ok(result);
        }
        if let Some(kind) = self.generic_construct(method, name, args, base, None)? {
            return Ok(Some(kind));
        }
        if let Some(result) = self.generic_call(method, name, args, base, None)? {
            return Ok(result);
        }
        if let Some(result) = self.call_value(method, name, args, base)? {
            return Ok(result);
        }
        if self.is_mutable_list_constructor(name) {
            return self
                .mutable_list(method, args, base, None, name.offset)
                .map(Some);
        }
        if name.text == "requireValue"
            && !self.symbols.functions.contains_key(name.text)
            && !self.symbols.types.names.contains_key(name.text)
        {
            let [arg] = args else {
                return Err(at(name, "requireValue expects one argument."));
            };
            let kind = self.expression(method, arg, base)?;
            if matches!(kind, Kind::Nullable(_)) {
                self.guard_non_null(method, kind.non_null(), base);
            } else if kind == Kind::Null {
                return Err(at(name, "requireValue expects a value with a known type."));
            }
            // A number already known not to be null, such as after an
            // `if value == null` check, needs no check.
            return Ok(Some(kind.non_null()));
        }
        if let Some(result) = self.implicit_this_call(method, name, args, base, expected)? {
            return Ok(result);
        }
        if !self.symbols.functions.contains_key(name.text)
            && !self.symbols.types.names.contains_key(name.text)
            && method.binding(name).is_none()
            && let Some(result) =
                self.java_inherited_static_call(method, name, args, base, expected)?
        {
            return Ok(result);
        }
        if !self.symbols.functions.contains_key(name.text)
            && !self.symbols.types.names.contains_key(name.text)
            && let Some(result) = self.builtin_function(method, name, args, base)?
        {
            return Ok(result);
        }
        if let Some(&kind) = self.symbols.types.names.get(name.text) {
            let signature = match kind {
                Kind::Object(class) if self.symbols.enum_values[class].is_some() => {
                    return Err(at(
                        name,
                        "Enum values are listed in the enum. Use one of them, such as Color.Red.",
                    ));
                }
                Kind::Object(class) => {
                    if self.symbols.class_declarations[class].inner_of.is_some()
                        && !std::mem::take(&mut self.inner_ready)
                    {
                        return self.create_inner(method, class, name, args, base, expected);
                    }
                    if self.symbols.class_declarations[class].is_object
                        && self.current_class != Some(class)
                    {
                        return Err(at(
                            name,
                            "An object has one instance already. Use its name without parentheses.",
                        ));
                    }
                    if self.symbols.is_abstract_class(class) {
                        return Err(at(
                            name,
                            "This class is abstract. Create one of its subclasses instead.",
                        ));
                    }
                    self.constructor_for(method, class, name, args, false)?
                }
                Kind::Java(class) if !self.symbols.java_classes.contains_key(&class) => {
                    return self
                        .java_construct(method, kind, name, args, base)
                        .map(Some);
                }
                Kind::Java(class) => self.symbols.java_classes[&class]
                    .constructor
                    .as_ref()
                    .ok_or_else(|| {
                        at(
                            name,
                            "Declare a constructor for this Java class before creating it.",
                        )
                    })?,
                _ => return Err(at(name, "Create a class that implements this interface.")),
            };
            let receiver = self.object_class(kind);
            let constructor = self.member(Some(kind), "<init>", &signature.descriptor);
            method.max_stack = method.max_stack.max(base + 2);
            let uninitialized = Local::Uninitialized(method.code.len() as u16);
            instruction(&mut method.code, 0xbb, receiver); // new
            method.code.push(0x59); // dup
            method.stack.extend([uninitialized, uninitialized]);
            let args = match kind {
                Kind::Object(class) => self.complete_args(
                    method,
                    name,
                    signature,
                    args,
                    DefaultHost::Constructor(class),
                )?,
                _ => args,
            };
            self.call_arguments(method, name, args, signature, base + 2)?;
            method.stack.truncate(base as usize);
            instruction(&mut method.code, 0xb7, constructor); // invokespecial
            return Ok(Some(kind));
        }
        let Some(signature) = self.symbols.functions.get(name.text) else {
            if name.text.contains('<') {
                let kind = resolve_type(&self.type_scope, name)?;
                return self
                    .java_construct(method, kind, name, args, base)
                    .map(Some);
            }
            if let Some(kind @ Kind::Java(_)) = self.symbols.types.implicit_java_class(name.text) {
                return self
                    .java_construct(method, kind, name, args, base)
                    .map(Some);
            }
            // Inside an extension function, the receiver's members are in scope.
            if self.in_extension {
                let this = Name {
                    text: "this",
                    offset: name.offset,
                };
                if method.binding(this).is_some() {
                    let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(this)));
                    return self.method_call_expected(method, receiver, name, args, base, expected);
                }
            }
            if let Some(path) = self.outer_path(method, name, true) {
                return self.method_call_expected(method, path, name, args, base, expected);
            }
            let known: Vec<&str> = self
                .symbols
                .functions
                .keys()
                .copied()
                .chain(self.symbols.types.names.keys().copied())
                .chain(
                    self.current_class
                        .into_iter()
                        .flat_map(|class| self.symbols.methods[class].keys().copied()),
                )
                .collect();
            return Err(at(
                name,
                suggest(
                    "This function or class has not been defined.",
                    name.text,
                    &known,
                ),
            ));
        };
        let args = self.complete_args(method, name, signature, args, DefaultHost::Function)?;
        self.call_arguments(method, name, args, signature, base)?;
        let foreign = self.symbols.java.get(name.text);
        if let Some(target) = foreign.or_else(|| self.symbols.imports.get(name.text)) {
            let reference = *self.members.entry((None, name.text)).or_insert_with(|| {
                let class = self.pool.class(&target.class);
                let interface =
                    foreign.is_some() && self.symbols.java_owner_is_interface(&target.class);
                self.pool.member(
                    if interface { 11 } else { 10 },
                    class,
                    target.member,
                    &signature.descriptor,
                )
            });
            instruction(&mut method.code, 0xb8, reference); // invokestatic
            if foreign.is_some() {
                self.check_java_result(method, signature.result, base);
            }
            return Ok(signature.result);
        }
        let reference = self.member(None, name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb8, reference); // invokestatic
        Ok(signature.result)
    }

    fn check_java_result(&mut self, method: &mut Method<'a>, result: Option<Kind>, base: u16) {
        if let Some(kind @ (Kind::Text | Kind::Java(_))) = result {
            self.guard_non_null(method, kind, base);
        }
    }

    fn guard_non_null(&mut self, method: &mut Method<'a>, kind: Kind, base: u16) {
        let class = self.instance_class(kind.non_null());
        let (guard, message) = self.null_guard_refs();
        method.max_stack = method.max_stack.max(base + 2);
        load_constant(&mut method.code, message);
        instruction(&mut method.code, 0xb8, guard);
        instruction(&mut method.code, 0xc0, class); // checkcast
        if !kind.is_reference() {
            self.unbox_element(method, kind, base);
        }
    }

    /// A JVM array can come from Java with null elements, so reading a
    /// non-null reference element checks it. The value stays on the stack.
    fn guard_array_element(&mut self, method: &mut Method<'a>, element: Kind, base: u16) {
        if !element.is_reference() || matches!(element, Kind::Nullable(_) | Kind::TypeParameter(..))
        {
            return;
        }
        let (guard, message) = self.null_guard_refs();
        method.code.push(0x59); // dup
        load_constant(&mut method.code, message);
        instruction(&mut method.code, 0xb8, guard);
        method.code.push(0x57); // pop
        method.max_stack = method.max_stack.max(base + 3);
    }

    fn null_guard_refs(&mut self) -> (u16, u16) {
        *self.null_guard.get_or_insert_with(|| {
            let objects = self.pool.class("java/util/Objects");
            let guard = self.pool.member(
                10,
                objects,
                "requireNonNull",
                "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
            );
            let text = self.pool.ascii("Expected a value, but got null.");
            (guard, self.pool.reference(8, &[text]))
        })
    }

    fn method_call_expected(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        if name.text == "sumOf" {
            return self.sum_of(method, receiver, name, args, base);
        }
        if name.text.starts_with(|ch: char| ch.is_ascii_uppercase())
            && let Some(result) =
                self.inner_on_receiver(method, receiver, name, args, base, expected)?
        {
            return Ok(result);
        }
        self.method_call_resolved(method, receiver, name, args, base, expected)
    }

    fn method_call_resolved(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let name = if self.symbols.overloads.is_empty() {
            name
        } else {
            self.overloaded_method(method, receiver, name, args)?
                .unwrap_or(name)
        };
        if let Expr::Variable(variable) = receiver
            && variable.text.starts_with("super")
            && variable.text.contains('<')
        {
            return self.interface_super_call(method, *variable, name, args, base, expected);
        }
        if let Expr::Variable(variable) = receiver
            && variable.text == "super"
            && method.binding(*variable).is_none()
        {
            return self.super_call_expected(method, *variable, name, args, base, expected);
        }
        if self
            .symbols
            .extensions
            .contains_key(name.text.split('<').next().unwrap_or(name.text))
        {
            return self.member_or_extension(method, receiver, name, args, base, expected);
        }
        self.member_call(method, receiver, name, args, base, expected)
    }

    /// A call on a receiver, looking only at the receiver's own members.
    fn member_call(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let allow_null = std::mem::take(&mut self.allow_null);
        if let Some(class) = self.static_class(method, receiver)
            && let Some(result) = self.static_call(method, class, name, args, base)?
        {
            return Ok(result);
        }
        if let Some(class) = self.enum_owner(method, receiver) {
            return self.enum_static_call(method, class, name, args, base);
        }
        if let Some(class) = self.static_owner(method, receiver) {
            self.allow_null = allow_null;
            return self.java_static_call(method, &class, name, args, base, expected);
        }
        let kind = self.expression(method, receiver, base)?;
        if let Some(result) = self.generic_method(
            method,
            generic_methods::MemberCall {
                receiver: kind,
                name,
                args,
                base,
                expected,
                special: false,
            },
        )? {
            return Ok(result);
        }
        self.call_method_on_expected(method, kind, name, args, base, allow_null, expected)
    }

    fn method_call(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        self.method_call_expected(method, receiver, name, args, base, None)
    }

    /// Calls `name` on a receiver of type `kind` that is already on the stack.
    fn call_method_on(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
    ) -> Result<Option<Kind>, Error> {
        self.call_method_on_expected(method, kind, name, args, base, allow_null, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn call_method_on_expected(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        allow_null: bool,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let kind = match self.symbols.member_bound(kind, name.text) {
            Some(bound) => {
                // The value erases to the first bound, so a method of a later
                // bound needs a cast to that interface.
                if Some(bound) != self.symbols.types.bound(kind) {
                    let class = self.object_class(bound);
                    instruction(&mut method.code, 0xc0, class); // checkcast
                }
                if matches!(kind, Kind::Nullable(_)) {
                    bound.nullable().unwrap_or(bound)
                } else {
                    bound
                }
            }
            None => kind,
        };
        let kind = self.symbols.method_receiver(kind, name.text);
        if let Some(result) = self.generic_method(
            method,
            generic_methods::MemberCall {
                receiver: kind,
                name,
                args,
                base,
                expected,
                special: false,
            },
        )? {
            return Ok(result);
        }
        if matches!(kind, Kind::Applied(_)) {
            return self.applied_method(method, kind, name, args, base, allow_null);
        }
        if let Kind::AppliedInterface(_) = kind {
            return self.applied_interface_method(method, kind, name, args, base);
        }
        if let Kind::MutableList(list) = kind {
            let builtin = matches!(
                (name.text, args.len()),
                ("size" | "clear", 0) | ("add" | "get" | "removeAt", 1) | ("set", 2)
            );
            if builtin {
                return self.mutable_list_method(method, list, name, args, base);
            }
            return self
                .java_instance_call_expected(method, kind, name, args, base, allow_null, expected);
        }
        if let Some(result) = self.builtin_method(method, kind, name, args, base)? {
            return Ok(result);
        }
        if matches!(kind, Kind::Text | Kind::Any | Kind::TypeParameter(_)) {
            return self.java_instance_call(method, kind, name, args, base, allow_null);
        }
        if matches!(kind, Kind::List(_)) {
            if name.text != "size" || !args.is_empty() {
                return Err(at(name, "Use list.size() to read the number of items."));
            }
            method.code.push(0xbe); // arraylength
            return Ok(Some(Kind::Int));
        }
        let methods = match kind {
            Kind::Function(index) if name.text == "invoke" => {
                return self.invoke_function(method, index, name, args, base);
            }
            Kind::Object(class)
                if self.symbols.find_method(class, name.text).is_none()
                    && matches!(
                        self.symbols.find_field(class, name.text),
                        Some((_, field)) if matches!(field.kind, Kind::Function(_))
                    ) =>
            {
                let field = self.symbols.find_field(class, name.text).unwrap().1;
                let Kind::Function(index) = field.kind else {
                    unreachable!()
                };
                let owner = self.object_class(kind);
                let reference = self.pool.member_for(
                    9,
                    owner,
                    field.name,
                    &field.kind.descriptor(&self.symbols.types),
                );
                instruction(&mut method.code, 0xb4, reference); // getfield
                return self.invoke_function(method, index, name, args, base);
            }
            Kind::Object(class)
                if self.symbols.find_method(class, name.text).is_none()
                    && self.symbols.java_root(class).is_some() =>
            {
                return self.java_inherited_call(
                    method, kind, class, name, args, base, allow_null, expected,
                );
            }
            Kind::Object(class)
                if self
                    .symbols
                    .find_method(class, name.text)
                    .is_some_and(|(owner, _)| self.symbols.generic_owner(class, owner)) =>
            {
                return self.applied_method(method, kind, name, args, base, allow_null);
            }
            Kind::Object(class) => {
                let signature = self.find_class_method(class, name)?;
                let args =
                    self.complete_args(method, name, signature, args, DefaultHost::Instance(kind))?;
                method.stack.push(self.local_type(kind));
                self.call_arguments(method, name, args, signature, base + 1)?;
                method.stack.pop();
                let reference = self.member(Some(kind), name.text, &signature.descriptor);
                instruction(&mut method.code, 0xb6, reference); // invokevirtual
                return Ok(signature.result);
            }
            Kind::Interface(interface) => &self.symbols.interfaces[interface],
            Kind::Java(class) => match self.symbols.java_classes.get(&class) {
                Some(declared) if declared.methods.contains_key(name.text) => &declared.methods,
                _ => {
                    return self.java_instance_call_expected(
                        method, kind, name, args, base, allow_null, expected,
                    );
                }
            },
            Kind::Nullable(_) | Kind::Null => {
                return Err(at(
                    name,
                    "Check for null and use requireValue before calling a method.",
                ));
            }
            _ => {
                return Err(at(name, "This method needs an object."));
            }
        };
        let signature = methods
            .get(name.text)
            .ok_or_else(|| at(name, "This type has no method with that name."))?;
        let args =
            self.complete_args(method, name, signature, args, DefaultHost::Instance(kind))?;
        method.stack.push(self.local_type(kind));
        self.call_arguments(method, name, args, signature, base + 1)?;
        method.stack.pop();
        let reference = self.member(Some(kind), name.text, &signature.descriptor);
        if self.symbols.is_interface(kind) {
            instruction(&mut method.code, 0xb9, reference); // invokeinterface
            method.code.extend([(signature.slots + 1) as u8, 0]);
        } else {
            instruction(&mut method.code, 0xb6, reference); // invokevirtual
        }
        if matches!(kind, Kind::Java(_)) {
            self.check_java_result(method, signature.result, base);
        }
        Ok(signature.result)
    }

    fn call_arguments(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        signature: &Signature<'_>,
        base: u16,
    ) -> Result<(), Error> {
        if signature.parameters.len() != args.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        for (arg, &(_, expected)) in args.iter().zip(&signature.parameters) {
            let at = method.stack.len() as u16;
            let kind = self.expression_as(method, arg, at, Some(expected))?;
            self.coerce(method, arg, kind, expected, at)?;
            method.push_stack(self.local_type(expected));
        }
        method.stack.truncate(base as usize);
        if let Some(kind) = signature.result {
            method.max_stack = method.max_stack.max(base + kind.slots());
        }
        Ok(())
    }

    fn object_class(&mut self, kind: Kind) -> u16 {
        if kind == Kind::List(ListKind::Text) {
            return self.refs.array;
        }
        if matches!(kind, Kind::MutableList(_)) {
            return self.runtime_class("java/util/ArrayList");
        }
        if let Kind::TypeParameter(index) = kind {
            let erasure = self.symbols.types.parameter_erasure(index);
            return self.object_class(erasure);
        }
        if kind == Kind::Any {
            return self.runtime_class("java/lang/Object");
        }
        if let Kind::Function(index) = kind {
            let interface = self
                .symbols
                .types
                .function_type(index)
                .interface()
                .unwrap()
                .0;
            return self.runtime_class(interface);
        }
        *self.object_classes.entry(kind).or_insert_with(|| {
            let name = match kind {
                Kind::Object(_)
                | Kind::Interface(_)
                | Kind::Applied(_)
                | Kind::AppliedInterface(_) => Cow::Owned(self.symbols.types.binary_name(kind)),
                Kind::Java(index) => Cow::Owned(self.symbols.types.java_name(index)),
                Kind::List(kind) => kind.descriptor(&self.symbols.types),
                _ => unreachable!("only class and interface types are registered here"),
            };
            self.pool.class(&name)
        })
    }

    /// Updates the LocalVariableTable at the current code position: opens
    /// entries for variables that came into scope and closes entries for
    /// variables that left it. Called at statement boundaries and scope exits.
    fn sync_locals(&mut self, method: &mut Method<'a>) {
        if self.line_starts.is_empty() {
            return;
        }
        let Ok(pc) = u16::try_from(method.code.len()) else {
            return;
        };
        // Bindings form a stack, so only those at or above the low-water
        // mark can have changed since the last synchronization.
        let low = method.variables_low.min(method.variables.len());
        let split = method
            .open_locals
            .partition_point(|&(index, _)| index < low);
        let mut kept = Vec::new();
        for (index, table) in method.open_locals.split_off(split) {
            let entry = &mut method.local_table[table];
            let same = method
                .variables
                .get(index)
                .is_some_and(|binding| binding.slot == entry.slot && binding.name == entry.label);
            if same {
                kept.push((index, table));
            } else {
                entry.end = Some(pc);
            }
        }
        let mut kept = kept.into_iter().peekable();
        for index in low..method.variables.len() {
            if let Some(&(open, table)) = kept.peek()
                && open == index
            {
                method.open_locals.push((open, table));
                kept.next();
                continue;
            }
            let Binding {
                name, slot, kind, ..
            } = method.variables[index];
            if name.is_empty() || name.starts_with('$') {
                continue;
            }
            let stored = if method.boxed_slots.contains(&slot) {
                ListKind::for_element(kind).map_or(kind, Kind::List)
            } else {
                kind
            };
            let descriptor = self.pool.ascii(&stored.descriptor(&self.symbols.types));
            let name_index = self.pool.utf8(&modified_utf8(name));
            if method.local_table_name == 0 {
                method.local_table_name = self.pool.ascii("LocalVariableTable");
            }
            method.open_locals.push((index, method.local_table.len()));
            method.local_table.push(LocalEntry {
                label: name.to_owned(),
                start: pc,
                end: None,
                name: name_index,
                descriptor,
                slot,
            });
        }
        method.variables_low = method.variables.len();
    }

    /// Marks a generated forwarding method with its source declaration's line.
    fn mark_bridge(&mut self, method: &mut Method<'a>) {
        if let Some(offset) = self.bridge_offset {
            self.mark_line(method, offset);
        }
    }

    /// Records that code emitted from here on comes from the source line
    /// containing `offset`, for the JVM LineNumberTable.
    fn mark_line(&mut self, method: &mut Method<'a>, offset: usize) {
        if self.line_starts.is_empty() {
            return;
        }
        self.sync_locals(method);
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let (Ok(line), Ok(pc)) = (u16::try_from(line), u16::try_from(method.code.len())) else {
            return;
        };
        if method.lines.last().is_some_and(|&(_, last)| last == line) {
            return;
        }
        if method.lines.last().is_some_and(|&(last, _)| last == pc) {
            method.lines.pop();
        }
        if method.line_table_name == 0 {
            let name = *self
                .line_table_name
                .get_or_insert_with(|| self.pool.ascii("LineNumberTable"));
            method.line_table_name = name;
        }
        method.lines.push((pc, line));
    }

    fn member(&mut self, owner: Option<Kind>, name: &'a str, descriptor: &str) -> u16 {
        // Constructors share a name, so only their descriptors tell them apart.
        if name != "<init>"
            && let Some(&index) = self.members.get(&(owner, name))
        {
            return index;
        }
        let class = match owner {
            Some(kind) => self.object_class(kind),
            None => *self
                .main_class
                .get_or_insert_with(|| self.pool.class(self.symbols.types.entry_class())),
        };
        let mangled;
        let binary = match owner {
            Some(
                Kind::Object(_) | Kind::Interface(_) | Kind::Applied(_) | Kind::AppliedInterface(_),
            ) if name != "<init>" => {
                mangled = format!("${name}");
                &mangled
            }
            Some(Kind::Java(index)) => self
                .symbols
                .java_classes
                .get(&index)
                .and_then(|class| class.aliases.get(name).copied())
                .unwrap_or(name),
            _ => name,
        };
        let tag = if owner.is_some_and(|kind| self.symbols.is_interface(kind)) {
            11
        } else {
            10
        };
        let reference = self.pool.member(tag, class, binary, descriptor);
        self.members.insert((owner, name), reference);
        reference
    }

    fn local_type(&mut self, kind: Kind) -> Local {
        if kind.is_nullable_primitive() {
            return Local::Object(self.instance_class(kind.non_null()));
        }
        match kind.non_null() {
            Kind::Text => Local::Object(self.refs.string),
            kind @ (Kind::Object(_)
            | Kind::Interface(_)
            | Kind::Java(_)
            | Kind::List(_)
            | Kind::MutableList(_)
            | Kind::Function(_)
            | Kind::Any
            | Kind::Applied(_)
            | Kind::AppliedInterface(_)
            | Kind::TypeParameter(_)) => Local::Object(self.object_class(kind)),
            Kind::Int | Kind::Bool | Kind::Char => Local::Int,
            Kind::Long => Local::Long,
            Kind::Double => Local::Double,
            Kind::Null | Kind::Nullable(_) => {
                unreachable!("stored references must have a declared or inferred reference type")
            }
        }
    }

    fn load(&mut self, method: &mut Method<'a>, name: Name<'a>, base: u16) -> Result<Kind, Error> {
        if name.text.starts_with("this@") {
            let path = self.labeled_this(method, name)?;
            return self.expression(method, path, base);
        }
        if let Some(binding) = method.binding(name) {
            if method.boxed_slots.contains(&binding.slot) {
                let list = ListKind::for_element(binding.kind).unwrap();
                method.access(0x19, binding.slot);
                push_int(&mut method.code, 0);
                method.code.push(list.load());
                method.max_stack = method
                    .max_stack
                    .max(base + 2)
                    .max(base + binding.kind.slots());
                return Ok(binding.kind);
            }
            method.access(binding.kind.load(), binding.slot);
            if let Some(&(_, narrowed)) = method
                .type_facts
                .iter()
                .rev()
                .find(|(slot, _)| *slot == binding.slot)
                && narrowed != binding.kind
            {
                self.cast_top(method, binding.kind, narrowed, base);
                return Ok(narrowed);
            }
            let kind = if method.non_null_slots.contains(&binding.slot) {
                binding.kind.non_null()
            } else {
                binding.kind
            };
            if kind != binding.kind && !kind.is_reference() {
                self.unbox_element(method, kind, base);
            }
            return Ok(kind);
        }
        if self.fields.contains_key(name.text)
            && method
                .binding(Name {
                    text: "this",
                    offset: 0,
                })
                .is_none()
            && self.current_class.is_some()
            && self.this_ready
        {
            return Err(at(
                name,
                "A static method cannot use the object's fields. Pass the object as a parameter.",
            ));
        }
        if let Some(&field) = self.fields.get(name.text) {
            if !self.this_ready {
                return Err(at(
                    name,
                    "Fields cannot be used before the superclass constructor runs.",
                ));
            }
            self.check_initialized(name)?;
            method.access(0x19, 0);
            instruction(&mut method.code, 0xb4, field.index); // getfield
            method.max_stack = method.max_stack.max(base + field.erased.slots().max(1));
            if field.lateinit {
                self.guard_non_null(method, field.erased, base);
            }
            if field.kind != field.erased {
                self.cast_top(method, field.erased, field.kind, base);
            }
            return Ok(field.kind);
        }
        if let Some((kind, target)) = self.symbols.java_fields.get(name.text) {
            let kind = *kind;
            let field = *self.java_fields.entry(name.text).or_insert_with(|| {
                let class = self.pool.class(&target.class);
                self.pool.member(
                    9,
                    class,
                    target.member,
                    &kind.descriptor(&self.symbols.types),
                )
            });
            instruction(&mut method.code, 0xb2, field); // getstatic
            self.check_java_result(method, Some(kind), base);
            return Ok(kind);
        }
        if let Some(class) = self.current_class
            && let Some(kind) = self.read_static_field(method, class, name, base)?
        {
            return Ok(kind);
        }
        // Inside an enum, `name` and `ordinal` describe the current value.
        if matches!(name.text, "name" | "ordinal")
            && self
                .current_class
                .is_some_and(|class| self.symbols.enum_values[class].is_some())
        {
            let this = Name {
                text: "this",
                offset: name.offset,
            };
            if method.binding(this).is_some() {
                let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(this)));
                if let Some(kind) = self.method_call(method, receiver, name, &[], base)? {
                    return Ok(kind);
                }
            }
        }
        if let Some(kind) = self.implicit_java_field(method, name, base)? {
            return Ok(kind);
        }
        if let Some(kind) = self.load_global(method, name, base) {
            return Ok(kind);
        }
        if let Some(class) = self.current_class
            && let Some(kind) = self.inherited_java_static_field(method, class, name, base, true)?
        {
            return Ok(kind);
        }
        // Inside an extension function, the receiver's fields are in scope.
        let this = Name {
            text: "this",
            offset: name.offset,
        };
        if self.in_extension
            && method.binding(this).is_some()
            && !self.type_scope.names.contains_key(name.text)
        {
            let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(this)));
            return self.field_access(method, receiver, name, base);
        }
        // A `by lazy` field is read through its method.
        if let Some(class) = self.current_class
            && self.symbols.find_method(class, name.text).is_some()
        {
            self.check_property_ready(name)?;
            let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(Name {
                text: "this",
                offset: name.offset,
            })));
            let attempt = method.snapshot();
            if let Ok(Some(kind)) = self.method_call(method, receiver, name, &[], base) {
                return Ok(kind);
            }
            method.restore(attempt);
        }
        // Inside an interface's default method, a property is a method call.
        if self.current_class.is_none() && !self.in_extension && method.binding(this).is_some() {
            let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(this)));
            let attempt = method.snapshot();
            if let Ok(Some(kind)) = self.method_call(method, receiver, name, &[], base) {
                return Ok(kind);
            }
            method.restore(attempt);
        }
        if let Some(&Kind::Object(class)) = self.type_scope.names.get(name.text)
            && self.symbols.class_declarations[class].is_object
        {
            let instance = Name {
                text: "INSTANCE",
                offset: name.offset,
            };
            if let Some(kind) = self.read_static_field(method, class, instance, base)? {
                return Ok(kind);
            }
        }
        if let Some(path) = self.outer_path(method, name, false) {
            return self.field_access(method, path, name, base);
        }
        let known: Vec<&str> = method
            .variables
            .iter()
            .map(|binding| binding.name)
            .chain(self.fields.keys().copied())
            .chain(self.symbols.globals.iter().map(|global| global.name))
            .collect();
        Err(at(
            name,
            suggest(
                "This variable has not been defined here.",
                name.text,
                &known,
            ),
        ))
    }

    fn runtime_class(&mut self, name: &'static str) -> u16 {
        *self
            .runtime_classes
            .entry(name)
            .or_insert_with(|| self.pool.class(name))
    }

    fn runtime_member(
        &mut self,
        owner: &'static str,
        name: &'static str,
        descriptor: &'static str,
        tag: u8,
    ) -> u16 {
        let class = self.runtime_class(owner);
        *self
            .runtime_members
            .entry((owner, name, descriptor, tag))
            .or_insert_with(|| self.pool.member(tag, class, name, descriptor))
    }

    fn expression_as(
        &mut self,
        method: &mut Method<'a>,
        expr: &'a Expr<'a>,
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Kind, Error> {
        let live = method.locals.len();
        let result = self.value_as(method, expr, base, expected);
        method.locals.truncate(live);
        result
    }

    fn value_as(
        &mut self,
        method: &mut Method<'a>,
        expr: &'a Expr<'a>,
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Kind, Error> {
        if let Expr::Safe(inner) = expr {
            return self
                .safe_access_expected(method, inner, base, expected)?
                .ok_or_else(|| Error {
                    offset: expr.offset(),
                    message: "This method does not give a value.",
                });
        }
        if let Expr::MethodCall(receiver, name, args) = expr {
            self.allow_null = matches!(expected, Some(Kind::Nullable(_)));
            let result = self.method_call_expected(method, receiver, *name, args, base, expected);
            self.allow_null = false;
            return result?.ok_or_else(|| at(*name, "This method does not give a value."));
        }
        if let Expr::Call(name, args) = expr
            && let Some(result) =
                self.implicit_generic_method(method, *name, args, base, expected)?
        {
            return result.ok_or_else(|| at(*name, "This method does not give a value."));
        }
        if let Expr::Call(name, args) = expr
            && let Some(kind) = self.generic_construct(method, *name, args, base, expected)?
        {
            return Ok(kind);
        }
        if let Expr::Call(name, args) = expr
            && let Some(result) = self.generic_call(method, *name, args, base, expected)?
        {
            return result.ok_or_else(|| at(*name, "This function does not give a value."));
        }
        // Infer omitted Java constructor arguments from the declared destination.
        // An existing raw value must not be retyped through this path.
        if let (Expr::Call(name, args), Some(expected)) = (expr, expected)
            && !name.text.contains('<')
            && let Some(kind @ Kind::Java(index)) = self.type_scope.names.get(name.text).copied()
            && !self.symbols.java_classes.contains_key(&index)
            && !self.symbols.functions.contains_key(name.text)
            && method.binding(*name).is_none()
            && !self
                .symbols
                .types
                .type_arguments(expected.non_null())
                .is_empty()
            && self.symbols.types.runtime_class_name(kind)
                == self.symbols.types.runtime_class_name(expected.non_null())
        {
            return self.java_construct(method, expected.non_null(), *name, args, base);
        }
        if let Expr::Lambda(lambda) = expr {
            return self.lambda(method, lambda, expected, base);
        }
        if let (Expr::When(offset, subject, branches), Some(expected)) = (expr, expected) {
            return self
                .when_value(
                    method,
                    *offset,
                    subject.as_deref(),
                    branches,
                    Some(expected),
                    base,
                )?
                .ok_or(Error {
                    offset: *offset,
                    message: "This when does not give a value.",
                });
        }
        if let (Expr::Call(name, values), Some(Kind::MutableList(list))) =
            (expr, expected.map(Kind::non_null))
            && self.is_mutable_list_constructor(*name)
        {
            return self.mutable_list(method, values, base, Some(list), name.offset);
        }
        if let (Expr::List(offset, values), Some(Kind::List(list))) =
            (expr, expected.map(Kind::non_null))
        {
            self.list_expression(method, *offset, values, base, Some(list))
        } else {
            self.allow_null = matches!(expected, Some(Kind::Nullable(_)));
            let result = match expr {
                Expr::Call(name, args) if expected.is_some() && name.text != "print" => self
                    .call_expected(method, *name, args, base, expected)
                    .and_then(|result| {
                        result.ok_or_else(|| at(*name, "This function does not return a value."))
                    }),
                _ => self.expression(method, expr, base),
            };
            self.allow_null = false;
            result
        }
    }

    /// The Java class named by `receiver` when it is a type name used for a
    /// static member, such as `Math` in `Math.max(1, 2)`.
    fn static_owner(&self, method: &Method<'a>, receiver: &Expr<'a>) -> Option<String> {
        let Expr::Variable(name) = receiver else {
            return None;
        };
        if method.binding(*name).is_some()
            || self.fields.contains_key(name.text)
            || self.symbols.java_fields.contains_key(name.text)
            || self.symbols.global(name.text).is_some()
            || !name.text.starts_with(|ch: char| ch.is_ascii_uppercase())
        {
            return None;
        }
        let kind = resolve_type(&self.type_scope, *name).ok()?;
        let wrapper = match kind {
            Kind::Int => "java/lang/Integer",
            Kind::Long => "java/lang/Long",
            Kind::Double => "java/lang/Double",
            Kind::Bool => "java/lang/Boolean",
            Kind::Char => "java/lang/Character",
            _ => return self.symbols.types.runtime_class_name(kind),
        };
        Some(wrapper.to_owned())
    }

    fn list_expression(
        &mut self,
        method: &mut Method<'a>,
        offset: usize,
        values: &'a [Expr<'a>],
        base: u16,
        expected: Option<ListKind>,
    ) -> Result<Kind, Error> {
        let length = i16::try_from(values.len()).map_err(|_| Error {
            offset,
            message: "This list has too many items.",
        })?;
        method.max_stack = method.max_stack.max(base + 1);
        let list = if let Some(first) = values.first() {
            let kind = self.expression(method, first, base)?;
            let list = expected
                .or_else(|| ListKind::for_element(kind))
                .ok_or_else(|| Error {
                    offset: first.offset(),
                    message: "A list needs elements of a supported non-null type.",
                })?;
            self.coerce(method, first, kind, list.element(), base)?;
            list
        } else {
            expected.unwrap_or(ListKind::Text)
        };
        if self.symbols.types.contains_parameter(list.element()) {
            return Err(Error {
                offset,
                message: "Generic code cannot create a List<T>, because the element type is erased. Take the list as a parameter, or use MutableList<T>.",
            });
        }

        let wide = list.element().slots() == 2;
        let spill = if wide && !values.is_empty() {
            let slot = method.local(self.local_type(list.element()));
            method.access(list.element().store(), slot);
            Some(slot)
        } else {
            None
        };
        push_int(&mut method.code, length);
        self.new_array(method, list);
        if values.is_empty() {
            return Ok(Kind::List(list));
        }
        self.list_items(method, values, list, base, spill)
    }

    /// With a length on the stack, creates an array for `list` elements.
    fn new_array(&mut self, method: &mut Method<'a>, list: ListKind) {
        match list {
            ListKind::Text => instruction(&mut method.code, 0xbd, self.refs.string), // anewarray
            ListKind::Int => method.code.extend([0xbc, 10]),                         // newarray int
            ListKind::Bool => method.code.extend([0xbc, 4]), // newarray boolean
            ListKind::Double => method.code.extend([0xbc, 7]), // newarray double
            ListKind::Long => method.code.extend([0xbc, 11]), // newarray long
            ListKind::Char => method.code.extend([0xbc, 5]), // newarray char
            ListKind::Byte => method.code.extend([0xbc, 8]), // newarray byte
            ListKind::Object(_)
            | ListKind::Interface(_)
            | ListKind::Java(_)
            | ListKind::Any
            | ListKind::Nested(_) => {
                let Local::Object(component) = self.local_type(list.element()) else {
                    unreachable!("reference elements have a class")
                };
                instruction(&mut method.code, 0xbd, component); // anewarray
            }
        }
    }

    fn list_items(
        &mut self,
        method: &mut Method<'a>,
        values: &'a [Expr<'a>],
        list: ListKind,
        base: u16,
        spill: Option<u16>,
    ) -> Result<Kind, Error> {
        if let Some(slot) = spill {
            method.code.extend([0x59, 0x03]); // dup, iconst_0
            method.access(list.element().load(), slot);
            method.code.push(list.store());
            method.locals.truncate(slot as usize);
            method.max_stack = method.max_stack.max(base + 5);
        } else {
            // Rearrange [item, array] to [array, array, 0, item] and store the first item.
            method.code.extend([0x5a, 0x5f, 0x03, 0x5f, list.store()]);
            method.max_stack = method.max_stack.max(base + 4);
        }
        let array = self.local_type(Kind::List(list));
        for (position, value) in values.iter().enumerate().skip(1) {
            method.code.push(0x59); // dup
            push_int(&mut method.code, position as i16);
            method.stack.extend([array, array, Local::Int]);
            let kind = self.expression(method, value, base + 3)?;
            self.coerce(method, value, kind, list.element(), base + 3)?;
            method.stack.truncate(base as usize);
            method.code.push(list.store());
        }
        Ok(Kind::List(list))
    }

    fn expression(
        &mut self,
        method: &mut Method<'a>,
        expr: &'a Expr<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        debug_assert_eq!(base as usize, method.stack.len());
        // Locals made while computing a value end with it, so a branch that
        // made some still joins cleanly with one that did not.
        let live = method.locals.len();
        let result = self.value(method, expr, base);
        method.locals.truncate(live);
        result
    }

    fn value(
        &mut self,
        method: &mut Method<'a>,
        expr: &'a Expr<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        // Only a call or field read can use permission to return null.
        let allow_null = std::mem::take(&mut self.allow_null);
        if matches!(
            expr,
            Expr::Call(..) | Expr::MethodCall(..) | Expr::Field(..) | Expr::Index(..)
        ) {
            self.allow_null = allow_null;
        }
        let kind = self.expression_inner(method, expr, base)?;
        self.allow_null = false;
        method.max_stack = method.max_stack.max(base + kind.slots());
        Ok(kind)
    }

    fn expression_inner(
        &mut self,
        method: &mut Method<'a>,
        expr: &'a Expr<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        match expr {
            Expr::List(offset, values) => self.list_expression(method, *offset, values, base, None),
            Expr::Index(receiver, index) => {
                let allow_null = std::mem::take(&mut self.allow_null);
                let kind = self.expression(method, receiver, base)?;
                if self.is_java_map(kind) {
                    return self.map_get(method, kind, index, base, allow_null);
                }
                let (list, mutable) = match kind {
                    Kind::List(list) => (list, false),
                    Kind::MutableList(list) => (list, true),
                    Kind::Text => {
                        let char_at = Name {
                            text: "charAt",
                            offset: receiver.offset(),
                        };
                        return self
                            .call_method_on(
                                method,
                                kind,
                                char_at,
                                std::slice::from_ref(index.as_ref()),
                                base,
                                false,
                            )?
                            .ok_or_else(|| at(char_at, "charAt must give a value."));
                    }
                    Kind::Object(_)
                    | Kind::Applied(_)
                    | Kind::Java(_)
                    | Kind::Interface(_)
                    | Kind::AppliedInterface(_) => {
                        // `a[i]` calls `a.get(i)` on an object.
                        let get = Name {
                            text: "get",
                            offset: receiver.offset(),
                        };
                        self.allow_null = allow_null;
                        return self
                            .call_method_on(
                                method,
                                kind,
                                get,
                                std::slice::from_ref(index.as_ref()),
                                base,
                                allow_null,
                            )?
                            .ok_or_else(|| at(get, "get must give a value."));
                    }
                    _ => {
                        return Err(Error {
                            offset: receiver.offset(),
                            message: "Indexing needs a list.",
                        });
                    }
                };
                method.stack.push(self.local_type(kind));
                let kind = self.expression(method, index, base + 1)?;
                method.stack.pop();
                self.symbols.require(index, kind, Kind::Int)?;
                if mutable {
                    let get = self.runtime_member(
                        "java/util/ArrayList",
                        "get",
                        "(I)Ljava/lang/Object;",
                        10,
                    );
                    instruction(&mut method.code, 0xb6, get);
                    self.unbox_element(method, list.element(), base);
                } else {
                    method.code.push(list.load());
                    if !allow_null {
                        self.guard_array_element(method, list.element(), base);
                    }
                }
                Ok(list.element())
            }
            Expr::Cast(value, annotation, safe) => {
                self.cast(method, value, *annotation, *safe, base)
            }
            Expr::Compare(..) | Expr::Logic(..) | Expr::Not(..) | Expr::Is(..) => {
                let false_jumps = self.branch_when(method, expr, false)?;
                push_int(&mut method.code, 1);
                let end = method.jump();
                method.patch_branches(&false_jumps, expr.offset())?;
                method.frame(method.locals.len());
                push_int(&mut method.code, 0);
                method.patch_branch(end, method.code.len(), expr.offset())?;
                method.stack.push(Local::Int);
                method.frame(method.locals.len());
                method.stack.pop();
                Ok(Kind::Bool)
            }
            Expr::Null(_) => {
                method.code.push(0x01); // aconst_null
                Ok(Kind::Null)
            }
            Expr::Jump(statement) => Err(Error {
                offset: statement.offset(),
                message: "This jump cannot be used as a value here. Use it after ?: or in a when or if branch.",
            }),
            Expr::Progression(start, ..) => Err(Error {
                offset: start.offset(),
                message: "A range can be used in a for loop or after in.",
            }),
            Expr::InRange(value, range, negated) => {
                let Expr::Range(start, end, inclusive) = range.as_ref() else {
                    unreachable!("the parser only wraps ranges")
                };
                let kind = self.expression(method, value, base)?;
                let slot = method.local(self.local_type(kind));
                method.access(kind.store(), slot);
                let held = || Box::new(Expr::Temp(kind, slot));
                let upper = if *inclusive {
                    Comparison::Le
                } else {
                    Comparison::Lt
                };
                let check = Expr::Logic(
                    crate::syntax::Logic::And,
                    vec![
                        Expr::Compare(held(), Comparison::Ge, start.clone()),
                        Expr::Compare(held(), upper, end.clone()),
                    ],
                );
                let check = if *negated {
                    Expr::Not(value.offset(), true, Box::new(check))
                } else {
                    check
                };
                let check: &'a Expr<'a> = Box::leak(Box::new(check));
                self.expression(method, check, base)
            }
            Expr::TryValue(holder, probe, statement) => {
                if !method.stack.is_empty() {
                    return Err(at(
                        *holder,
                        "A try used as a value must be on its own line, as in let x = try { ... }.",
                    ));
                }
                let crate::syntax::Statement::Try(_, body, ..) = statement.as_ref() else {
                    unreachable!("the parser builds a try statement")
                };
                // The first value shows the type; its code is thrown away.
                let saved = method.snapshot();
                let variables = method.variables.len();
                let kind = self
                    .block(method, body.split_last().map_or(&[][..], |(_, rest)| rest))
                    .and_then(|_| self.expression(method, probe, 0));
                method.truncate_variables(variables);
                method.restore(saved);
                let kind = kind?;
                if kind == Kind::Null {
                    return Err(at(
                        *holder,
                        "Give this value a type, such as `let name: Text? = ...`.",
                    ));
                }
                let slot = method.local(self.local_type(kind));
                match kind {
                    Kind::Long => method.code.push(0x09),
                    Kind::Double => method.code.push(0x0e),
                    kind if kind.is_reference() => method.code.push(0x01),
                    _ => method.code.push(0x03),
                }
                method.access(kind.store(), slot);
                method.max_stack = method.max_stack.max(base + kind.slots());
                method.variables.push(Binding {
                    name: holder.text,
                    slot,
                    kind,
                    mutable: true,
                });
                let result = self.block(method, std::slice::from_ref(statement.as_ref()));
                method.pop_variable();
                result?;
                method.access(kind.load(), slot);
                Ok(kind)
            }
            Expr::Spread(inner) => Err(Error {
                offset: inner.offset(),
                message: "Use *items only as the last argument of a vararg parameter.",
            }),
            Expr::Named(label, _) => Err(at(
                *label,
                "Named arguments work only in calls to functions written in Mammoth.",
            )),
            Expr::Temp(kind, slot) => {
                method.access(kind.load(), *slot);
                method.max_stack = method.max_stack.max(base + kind.slots());
                Ok(*kind)
            }
            Expr::StaticHelper(class, name, args) => self
                .static_call(method, *class, *name, args, base)?
                .flatten()
                .ok_or_else(|| at(*name, "This default value does not give a value.")),
            Expr::Call(name, args) => {
                if name.text == "print" {
                    return Err(at(*name, "print does not return a value."));
                }
                self.call(method, *name, args, base)?
                    .ok_or_else(|| at(*name, "This function does not return a value."))
            }
            Expr::MethodCall(receiver, name, args) => self
                .method_call(method, receiver, *name, args, base)?
                .ok_or_else(|| at(*name, "This method does not return a value.")),
            Expr::Field(receiver, name) => self.field_access(method, receiver, *name, base),
            Expr::Variable(name) => self.load(method, *name, base),
            Expr::Int(_, value) => {
                self.push_int32(method, *value);
                Ok(Kind::Int)
            }
            Expr::Double(_, value) => {
                let bits = value.to_bits();
                match bits {
                    0 => method.code.push(0x0e),                  // dconst_0
                    0x3ff0000000000000 => method.code.push(0x0f), // dconst_1
                    _ => {
                        let index = *self.doubles.entry(bits).or_insert_with(|| {
                            let index = self.pool.entry(6); // CONSTANT_Double
                            self.pool.count += 1; // Double constants occupy two pool slots.
                            self.pool.bytes.extend(bits.to_be_bytes());
                            index
                        });
                        instruction(&mut method.code, 0x14, index); // ldc2_w
                    }
                }
                Ok(Kind::Double)
            }
            Expr::Bool(_, value) => {
                push_int(&mut method.code, i16::from(*value));
                Ok(Kind::Bool)
            }
            Expr::Long(_, value) => {
                match value {
                    0 => method.code.push(0x09), // lconst_0
                    1 => method.code.push(0x0a), // lconst_1
                    _ => {
                        let index = *self.longs.entry(*value).or_insert_with(|| {
                            let index = self.pool.entry(5); // CONSTANT_Long
                            self.pool.count += 1; // Long constants occupy two pool slots.
                            self.pool.bytes.extend(value.to_be_bytes());
                            index
                        });
                        instruction(&mut method.code, 0x14, index); // ldc2_w
                    }
                }
                Ok(Kind::Long)
            }
            Expr::Char(_, value) => {
                self.push_int32(method, i32::from(*value));
                Ok(Kind::Char)
            }
            Expr::Negate(offset, inner) => {
                let kind = self.expression(method, inner, base)?;
                let opcode = match kind {
                    Kind::Int => 0x74,    // ineg
                    Kind::Long => 0x75,   // lneg
                    Kind::Double => 0x77, // dneg
                    other if self.has_unary_minus(other) => {
                        let negate = Name {
                            text: "unaryMinus",
                            offset: *offset,
                        };
                        return self
                            .call_method_on(method, other, negate, &[], base, false)?
                            .ok_or(Error {
                                offset: *offset,
                                message: "unaryMinus must return a value.",
                            });
                    }
                    _ => {
                        return Err(Error {
                            offset: *offset,
                            message: "Only Int, Long, and Double values can be negated.",
                        });
                    }
                };
                method.code.push(opcode);
                Ok(kind)
            }
            Expr::Range(..) => Err(Error {
                offset: expr.offset(),
                message: "Use a range such as 1..10 in a for loop.",
            }),
            Expr::Lambda(lambda) => self.lambda(method, lambda, None, base),
            Expr::When(offset, subject, branches) => self
                .when_value(method, *offset, subject.as_deref(), branches, None, base)?
                .ok_or(Error {
                    offset: *offset,
                    message: "This when does not give a value.",
                }),
            Expr::Safe(inner) => self.safe_access(method, inner, base)?.ok_or(Error {
                offset: inner.offset(),
                message: "This call does not give a value.",
            }),
            Expr::Elvis(value, fallback) => self.elvis(method, value, fallback, base),
            Expr::Arithmetic(left, rest) => {
                let kind = self.expression(method, left, base)?;
                let rest: Vec<_> = rest
                    .iter()
                    .map(|(operator, right)| (*operator, right))
                    .collect();
                self.arithmetic_tail(method, left, kind, &rest, base)
            }
            Expr::Text(offset, parts) => {
                if let [Part::Literal(_)] = parts.as_slice() {
                    return self.part(method, *offset, &parts[0], base);
                }
                method.max_stack = method.max_stack.max(base + 2);
                instruction(&mut method.code, 0xbb, self.refs.builder);
                method.code.push(0x59); // dup
                instruction(&mut method.code, 0xb7, self.refs.constructor);
                for part in parts {
                    method.stack.push(Local::Object(self.refs.builder));
                    let kind = self.part(method, *offset, part, base + 1)?;
                    method.stack.pop();
                    method.max_stack = method.max_stack.max(base + 1 + kind.slots());
                    self.append_value(method, kind, base + 1);
                }
                instruction(&mut method.code, 0xb6, self.refs.to_string);
                Ok(Kind::Text)
            }
        }
    }

    /// Applies `rest` to a value of type `kind` that is already on the stack at `base`.
    fn arithmetic_tail(
        &mut self,
        method: &mut Method<'a>,
        left: &'a Expr<'a>,
        mut kind: Kind,
        rest: &[(Arithmetic, &'a Expr<'a>)],
        base: u16,
    ) -> Result<Kind, Error> {
        if kind == Kind::Text {
            // Turn [text] into [builder] and append each operand.
            method.max_stack = method.max_stack.max(base + 3);
            instruction(&mut method.code, 0xbb, self.refs.builder); // new
            method.code.extend([0x5a, 0x5f]); // dup_x1, swap
            let init = self.runtime_member(
                "java/lang/StringBuilder",
                "<init>",
                "(Ljava/lang/String;)V",
                10,
            );
            instruction(&mut method.code, 0xb7, init);
            for &(operator, right) in rest {
                if !matches!(operator, Arithmetic::Add) {
                    return Err(Error {
                        offset: right.offset(),
                        message: "Text can only be joined with +.",
                    });
                }
                method.stack.push(Local::Object(self.refs.builder));
                self.allow_null = true; // Joined text shows null as "null".
                let rhs = self.expression(method, right, base + 1)?;
                method.stack.pop();
                self.append_value(method, rhs, base + 1);
            }
            instruction(&mut method.code, 0xb6, self.refs.to_string);
            return Ok(Kind::Text);
        }
        if matches!(
            kind,
            Kind::Object(_)
                | Kind::Applied(_)
                | Kind::Java(_)
                | Kind::Interface(_)
                | Kind::AppliedInterface(_)
                | Kind::MutableList(_)
        ) && !rest.is_empty()
        {
            // `a + b` calls `a.plus(b)` on an object.
            for &(operator, right) in rest {
                // java.math names these operations differently.
                let math = self
                    .symbols
                    .types
                    .runtime_class_name(kind)
                    .is_some_and(|class| class.starts_with("java/math/"));
                let name = Name {
                    text: match (operator, math) {
                        (Arithmetic::Add, false) => "plus",
                        (Arithmetic::Sub, false) => "minus",
                        (Arithmetic::Mul, false) => "times",
                        (Arithmetic::Div, false) => "div",
                        (Arithmetic::Rem, false) => "rem",
                        (Arithmetic::Add, true) => "add",
                        (Arithmetic::Sub, true) => "subtract",
                        (Arithmetic::Mul, true) => "multiply",
                        (Arithmetic::Div, true) => "divide",
                        (Arithmetic::Rem, true) => "remainder",
                        (Arithmetic::Shl, _) => "shl",
                        (Arithmetic::Shr, _) => "shr",
                        (Arithmetic::Ushr, _) => "ushr",
                        (Arithmetic::BitAnd, _) => "and",
                        (Arithmetic::BitOr, _) => "or",
                        (Arithmetic::BitXor, _) => "xor",
                    },
                    offset: right.offset(),
                };
                let argument = std::slice::from_ref(right);
                if self.symbols.extensions.contains_key(name.text) {
                    let attempt = method.snapshot();
                    if let Ok(Some(Some(result))) =
                        self.extension_on_stack(method, kind, name, argument, base, None)
                    {
                        kind = result;
                        continue;
                    }
                    method.restore(attempt);
                }
                kind = self
                    .call_method_on(method, kind, name, argument, base, false)
                    .map_err(|error| {
                        if error.message.contains("no method") {
                            Error {
                                offset: left.offset(),
                                message: "Arithmetic needs an Int, Long, or Double, or an object with a method such as plus.",
                            }
                        } else {
                            error
                        }
                    })?
                    .ok_or_else(|| at(name, "This operator method does not give a value."))?;
            }
            return Ok(kind);
        }
        if kind == Kind::Char
            && let Some((&(operator, right), others)) = rest.split_first()
        {
            // `'a' + 1` is a Char and `'c' - 'a'` is an Int.
            method.push_stack(Local::Int);
            let rhs = self.expression(method, right, base + 1)?;
            method.pop_stack();
            let result = match (operator, rhs) {
                (Arithmetic::Add, Kind::Int) => {
                    method.code.extend([0x60, 0x92]); // iadd, i2c
                    Kind::Char
                }
                (Arithmetic::Sub, Kind::Int) => {
                    method.code.extend([0x64, 0x92]); // isub, i2c
                    Kind::Char
                }
                (Arithmetic::Sub, Kind::Char) => {
                    method.code.push(0x64); // isub
                    Kind::Int
                }
                _ => {
                    return Err(Error {
                        offset: right.offset(),
                        message: "A Char can only add or subtract an Int, or subtract a Char.",
                    });
                }
            };
            method.max_stack = method.max_stack.max(base + 2);
            if others.is_empty() {
                return Ok(result);
            }
            return self.arithmetic_tail(method, left, result, others, base);
        }
        if !kind.is_numeric() {
            let joins_text = matches!(rest.first(), Some((Arithmetic::Add, Expr::Text(..))));
            return Err(Error {
                offset: left.offset(),
                message: if joins_text {
                    "To join text, start with the text, such as \"Total: \" + count, or use \"{count}\"."
                } else {
                    "Arithmetic needs an Int, Long, or Double."
                },
            });
        }
        for &(operator, right) in rest {
            method.push_stack(self.local_type(kind));
            let rhs = self.expression(method, right, base + kind.slots())?;
            method.pop_stack();
            if !rhs.is_numeric() {
                return Err(Error {
                    offset: right.offset(),
                    message: if rhs == Kind::Text && matches!(operator, Arithmetic::Add) {
                        "To join text, start with the text, such as \"Total: \" + count, or use \"{count}\"."
                    } else {
                        "Arithmetic needs an Int, Long, or Double."
                    },
                });
            }
            if matches!(
                operator,
                Arithmetic::Shl
                    | Arithmetic::Shr
                    | Arithmetic::Ushr
                    | Arithmetic::BitAnd
                    | Arithmetic::BitOr
                    | Arithmetic::BitXor
            ) && (kind == Kind::Double || rhs == Kind::Double)
            {
                return Err(Error {
                    offset: right.offset(),
                    message: "Bit operations need an Int or a Long.",
                });
            }
            if matches!(
                operator,
                Arithmetic::Shl | Arithmetic::Shr | Arithmetic::Ushr
            ) {
                if rhs != Kind::Int {
                    return Err(Error {
                        offset: right.offset(),
                        message: "The shift distance must be an Int.",
                    });
                }
                let opcode: u8 = match operator {
                    Arithmetic::Shl => 0x78,
                    Arithmetic::Shr => 0x7a,
                    _ => 0x7c,
                };
                method.code.push(opcode + u8::from(kind == Kind::Long));
                continue;
            }
            kind = self.widen_pair(method, kind, rhs, base);
            let opcode = match operator {
                Arithmetic::Shl | Arithmetic::Shr | Arithmetic::Ushr => unreachable!(),
                Arithmetic::BitAnd => 0x7e,
                Arithmetic::BitOr => 0x80,
                Arithmetic::BitXor => 0x82,
                Arithmetic::Add => 0x60,
                Arithmetic::Sub => 0x64,
                Arithmetic::Mul => 0x68,
                Arithmetic::Div => 0x6c,
                Arithmetic::Rem => 0x70,
            };
            let offset = match kind {
                Kind::Long => 1,
                Kind::Double => 3,
                _ => 0,
            };
            method.code.push(opcode + offset);
        }
        Ok(kind)
    }

    /// With [left: `left`, right: `right`] on the stack at `base`, converts the
    /// narrower operand so both have the wider numeric type, and returns that type.
    fn widen_pair(&mut self, method: &mut Method<'a>, left: Kind, right: Kind, base: u16) -> Kind {
        if left == right {
            return left;
        }
        let wider = if numeric_rank(left) > numeric_rank(right) {
            left
        } else {
            right
        };
        method.max_stack = method.max_stack.max(base + 2 * wider.slots());
        if right != wider {
            method.code.push(widening_opcode(right, wider));
        } else {
            // Move the right operand aside so the left one can be converted.
            let slot = method.local(self.local_type(right));
            method.access(right.store(), slot);
            method.code.push(widening_opcode(left, wider));
            method.access(right.load(), slot);
            method.locals.truncate(slot as usize);
        }
        wider
    }

    /// Checks that a value of type `actual` on top of the stack can be used where
    /// `expected` is required, converting Int and Long values to wider numbers.
    fn coerce(
        &mut self,
        method: &mut Method<'a>,
        expr: &Expr<'_>,
        actual: Kind,
        expected: Kind,
        base: u16,
    ) -> Result<(), Error> {
        if expected.is_nullable_primitive() && !actual.is_reference() {
            let primitive = expected.non_null();
            self.coerce(method, expr, actual, primitive, base)?;
            self.box_value(method, primitive);
            return Ok(());
        }
        if actual != expected
            && actual.is_numeric()
            && expected.is_numeric()
            && numeric_rank(actual) < numeric_rank(expected)
        {
            method.code.push(widening_opcode(actual, expected));
            method.max_stack = method.max_stack.max(base + expected.slots());
            return Ok(());
        }
        if expected.non_null() == Kind::Any && !actual.is_reference() {
            self.box_value(method, actual);
            method.max_stack = method.max_stack.max(base + actual.slots());
            return Ok(());
        }
        self.symbols.require(expr, actual, expected)
    }

    fn push_int32(&mut self, method: &mut Method<'a>, value: i32) {
        if let Ok(value) = i16::try_from(value) {
            push_int(&mut method.code, value);
        } else {
            let index = *self.integers.entry(value).or_insert_with(|| {
                let index = self.pool.entry(3);
                word(&mut self.pool.bytes, value as u32);
                index
            });
            load_constant(&mut method.code, index);
        }
    }

    fn part(
        &mut self,
        method: &mut Method<'a>,
        offset: usize,
        part: &'a Part<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        let text = match part {
            Part::Literal(text) => text,
            Part::Value(expr) => {
                self.allow_null = true; // A template shows null as "null".
                return self.expression(method, expr, base);
            }
        };
        let index = match self.strings.get(text.as_str()) {
            Some(&index) => index,
            None => {
                let encoded = modified_utf8(text);
                if encoded.len() > u16::MAX as usize {
                    return Err(Error {
                        offset,
                        message: "This text is too long for one JVM string constant.",
                    });
                }
                let utf8 = self.pool.utf8(&encoded);
                let index = self.pool.reference(8, &[utf8]);
                self.strings.insert(text.as_str(), index);
                index
            }
        };
        load_constant(&mut method.code, index);
        Ok(Kind::Text)
    }

    fn branch_when(
        &mut self,
        method: &mut Method<'a>,
        condition: &'a Condition<'a>,
        when: bool,
    ) -> Result<Vec<usize>, Error> {
        let base = method.stack.len() as u16;
        let opcode = match condition {
            Expr::Not(_, odd, inner) => return self.branch_when(method, inner, when ^ odd),
            Expr::Is(value, annotation) => {
                let kind = self.expression(method, value, base)?;
                let target = resolve_type(&self.type_scope, *annotation)?.non_null();
                self.symbols
                    .check_runtime_target(target, kind, *annotation)?;
                if !kind.is_reference() {
                    return Err(Error {
                        offset: value.offset(),
                        message: "Use is with values whose type is not known exactly, such as Any.",
                    });
                }
                let class = self.instance_class(target);
                instruction(&mut method.code, 0xc1, class); // instanceof
                if when { 0x9a } else { 0x99 } // ifne or ifeq
            }
            Expr::Logic(logic, terms) => {
                let short_circuit = *logic == Logic::Or;
                let facts = method.non_null_slots.len();
                let type_count = method.type_facts.len();
                let (last, preceding) = terms.split_last().unwrap();
                let mut branches = Vec::new();
                let mut skips = Vec::new();
                for term in preceding {
                    let jumps = self.branch_when(method, term, short_circuit)?;
                    if short_circuit == when {
                        branches.extend(jumps);
                    } else {
                        skips.extend(jumps);
                    }
                    let narrowed = method.non_null_facts(term, !short_circuit);
                    method.non_null_slots.extend(narrowed);
                    let types = self.type_facts(method, term, !short_circuit);
                    method.type_facts.extend(types);
                }
                branches.extend(self.branch_when(method, last, when)?);
                method.non_null_slots.truncate(facts);
                method.type_facts.truncate(type_count);
                if !skips.is_empty() {
                    method.patch_branches(&skips, condition.offset())?;
                    method.frame(method.locals.len());
                }
                return Ok(branches);
            }
            Expr::Compare(left, comparison, right) => {
                // `==` and `!=` handle null, so a null Java result is fine here.
                let equality = matches!(comparison, Comparison::Eq | Comparison::Ne);
                self.allow_null = equality;
                let lhs = self.expression(method, left, base)?;
                if !equality
                    && matches!(
                        lhs,
                        Kind::Object(_)
                            | Kind::Applied(_)
                            | Kind::Java(_)
                            | Kind::Interface(_)
                            | Kind::AppliedInterface(_)
                    )
                {
                    // `a < b` asks `a.compareTo(b) < 0`.
                    let name = Name {
                        text: "compareTo",
                        offset: left.offset(),
                    };
                    let _order = self
                        .call_method_on(
                            method,
                            lhs,
                            name,
                            std::slice::from_ref(right.as_ref()),
                            base,
                            false,
                        )?
                        .filter(|&kind| kind == Kind::Int)
                        .ok_or_else(|| at(name, "compareTo must give an Int."))?;
                    let (yes, no) = match comparison {
                        Comparison::Lt => (0x9b, 0x9c),
                        Comparison::Le => (0x9e, 0x9d),
                        Comparison::Gt => (0x9d, 0x9e),
                        _ => (0x9c, 0x9b),
                    };
                    let branch = method.code.len();
                    instruction(&mut method.code, if when { yes } else { no }, 0);
                    return Ok(vec![branch]);
                }
                let operand = if lhs == Kind::Null {
                    Local::Null
                } else {
                    self.local_type(lhs)
                };
                method.push_stack(operand);
                self.allow_null = equality;
                let rhs = self.expression(method, right, base + lhs.slots())?;
                method.pop_stack();
                let (yes, no) =
                    self.compare_kinds(method, left, lhs, right, rhs, *comparison, base)?;
                if when { yes } else { no }
            }
            _ => {
                let kind = self.expression(method, condition, base)?;
                self.symbols.require(condition, kind, Kind::Bool)?;
                if when { 0x9a } else { 0x99 } // ifne or ifeq
            }
        };
        let branch = method.code.len();
        instruction(&mut method.code, opcode, 0);
        Ok(vec![branch])
    }

    /// Whether two class types share no subclass relation. Interfaces and
    /// generic types are left out, since one object can be both.
    fn unrelated_classes(&self, lhs: Kind, rhs: Kind) -> bool {
        let (lhs, rhs) = (lhs.non_null(), rhs.non_null());
        matches!(lhs, Kind::Object(_) | Kind::Text)
            && matches!(rhs, Kind::Object(_) | Kind::Text)
            && !self.symbols.fits(lhs, rhs)
            && !self.symbols.fits(rhs, lhs)
    }

    /// With two values of types `lhs` and `rhs` on the stack, emits what is
    /// needed to compare them and returns the (branch-if-true,
    /// branch-if-false) opcodes. `left` and `right` locate errors.
    #[allow(clippy::too_many_arguments)]
    fn compare_kinds(
        &mut self,
        method: &mut Method<'a>,
        left: &Expr<'_>,
        lhs: Kind,
        right: &Expr<'_>,
        rhs: Kind,
        comparison: Comparison,
        base: u16,
    ) -> Result<(u8, u8), Error> {
        let (lhs, rhs) = if lhs != rhs && lhs.is_numeric() && rhs.is_numeric() {
            let wider = self.widen_pair(method, lhs, rhs, base);
            (wider, wider)
        } else if matches!(comparison, Comparison::Eq | Comparison::Ne)
            && !lhs.is_reference()
            && lhs.nullable() == Some(rhs)
        {
            let slot = method.local(self.local_type(rhs));
            method.access(rhs.store(), slot);
            self.box_value(method, lhs);
            method.access(rhs.load(), slot);
            method.locals.truncate(slot as usize);
            (rhs, rhs)
        } else if matches!(comparison, Comparison::Eq | Comparison::Ne)
            && !rhs.is_reference()
            && rhs.nullable() == Some(lhs)
        {
            self.box_value(method, rhs);
            (lhs, lhs)
        } else {
            (lhs, rhs)
        };
        let both_references =
            lhs.is_reference() && rhs.is_reference() && lhs != Kind::Null && rhs != Kind::Null;
        let (yes, no) = if lhs == Kind::Long && rhs == Kind::Long {
            method.code.push(0x94); // lcmp
            match comparison {
                Comparison::Eq => (0x99, 0x9a),
                Comparison::Ne => (0x9a, 0x99),
                Comparison::Lt => (0x9b, 0x9c),
                Comparison::Le => (0x9e, 0x9d),
                Comparison::Gt => (0x9d, 0x9e),
                Comparison::Ge => (0x9c, 0x9b),
            }
        } else if both_references {
            if self.unrelated_classes(lhs, rhs) {
                return Err(Error {
                    offset: left.offset(),
                    message: "These values have unrelated types, so they can never be equal.",
                });
            }
            self.compare_references(method, left, lhs, rhs, comparison)?
        } else if lhs == Kind::Double && rhs == Kind::Double {
            // Choose the NaN ordering that makes ordered comparisons false.
            let (compare, yes, no) = match comparison {
                Comparison::Eq => (0x97, 0x99, 0x9a),
                Comparison::Ne => (0x97, 0x9a, 0x99),
                Comparison::Lt => (0x98, 0x9b, 0x9c),
                Comparison::Le => (0x98, 0x9e, 0x9d),
                Comparison::Gt => (0x97, 0x9d, 0x9e),
                Comparison::Ge => (0x97, 0x9c, 0x9b),
            };
            method.code.push(compare); // dcmpl or dcmpg
            (yes, no)
        } else if lhs == Kind::Null || rhs == Kind::Null {
            match comparison {
                Comparison::Eq if lhs.is_reference() && rhs.is_reference() => (0xa5, 0xa6),
                Comparison::Ne if lhs.is_reference() && rhs.is_reference() => (0xa6, 0xa5),
                _ => {
                    return Err(Error {
                        offset: left.offset(),
                        message: if matches!(comparison, Comparison::Eq | Comparison::Ne) {
                            "This value cannot be null. If Java can pass null to a lambda parameter, declare it with a nullable type such as `value: Int?`."
                        } else {
                            "Use == or != to compare a reference to null."
                        },
                    });
                }
            }
        } else {
            let boolean_equality = lhs == Kind::Bool
                && rhs == Kind::Bool
                && matches!(comparison, Comparison::Eq | Comparison::Ne);
            let characters = lhs == Kind::Char && rhs == Kind::Char;
            if !boolean_equality && !characters {
                if lhs == Kind::Double {
                    self.symbols.require(right, rhs, Kind::Double)?;
                } else {
                    self.symbols.require(left, lhs, Kind::Int)?;
                    self.symbols.require(right, rhs, Kind::Int)?;
                }
            }
            match comparison {
                Comparison::Eq => (0x9f, 0xa0),
                Comparison::Ne => (0xa0, 0x9f),
                Comparison::Lt => (0xa1, 0xa2),
                Comparison::Le => (0xa4, 0xa3),
                Comparison::Gt => (0xa3, 0xa4),
                Comparison::Ge => (0xa2, 0xa1),
            }
        };
        Ok((yes, no))
    }

    fn conditional(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        condition: &'a Condition<'a>,
        body: &'a [Statement<'a>],
        otherwise: Option<&'a [Statement<'a>]>,
    ) -> Result<bool, Error> {
        let branch = self.branch_when(method, condition, false)?;
        let then_facts = method.non_null_facts(condition, true);
        let else_facts = method.non_null_facts(condition, false);
        let then_types = self.type_facts(method, condition, true);
        let else_types = self.type_facts(method, condition, false);
        let types = method.type_facts.len();
        method.type_facts.extend_from_slice(&then_types);
        let then_continues = self.scoped_block(method, body, &then_facts);
        method.type_facts.truncate(types);
        let then_continues = then_continues?;
        let else_continues = if let Some(otherwise) = otherwise {
            let skip_else = then_continues.then(|| method.jump());
            method.patch_branches(&branch, name.offset)?;
            method.frame(method.locals.len());
            method.type_facts.extend_from_slice(&else_types);
            let else_continues = self.scoped_block(method, otherwise, &else_facts);
            method.type_facts.truncate(types);
            let else_continues = else_continues?;
            if let Some(skip_else) = skip_else {
                method.patch_branch(skip_else, method.code.len(), name.offset)?;
                method.frame(method.locals.len());
            }
            else_continues
        } else {
            method.patch_branches(&branch, name.offset)?;
            method.frame(method.locals.len());
            true
        };
        if then_continues != else_continues {
            method.non_null_slots.extend(if then_continues {
                then_facts
            } else {
                else_facts
            });
            method.type_facts.extend(if then_continues {
                then_types
            } else {
                else_types
            });
        }
        Ok(then_continues || else_continues)
    }

    /// Read-only variables that `condition` proves to have a more specific
    /// type when it evaluates to `when`.
    fn type_facts(
        &self,
        method: &Method<'a>,
        condition: &Condition<'a>,
        when: bool,
    ) -> Vec<(u16, Kind)> {
        let mut facts = Vec::new();
        self.collect_type_facts(method, condition, when, &mut facts);
        facts
    }

    fn collect_type_facts(
        &self,
        method: &Method<'a>,
        condition: &Condition<'a>,
        when: bool,
        facts: &mut Vec<(u16, Kind)>,
    ) {
        match condition {
            Expr::Not(_, odd, inner) => self.collect_type_facts(method, inner, when ^ odd, facts),
            Expr::Logic(logic, terms) if when == (*logic == Logic::And) => {
                for term in terms {
                    self.collect_type_facts(method, term, when, facts);
                }
            }
            Expr::Is(value, annotation) if when => {
                if let Expr::Variable(name) = value.as_ref()
                    && let Some(binding) = method.binding(*name)
                    && !binding.mutable
                    && let Ok(kind) = resolve_type(&self.type_scope, *annotation)
                {
                    facts.push((binding.slot, kind.non_null()));
                }
            }
            _ => {}
        }
    }

    fn scoped_block(
        &mut self,
        method: &mut Method<'a>,
        body: &'a [Statement<'a>],
        narrowed: &[u16],
    ) -> Result<bool, Error> {
        let locals = method.locals.len();
        let variables = method.variables.len();
        let facts = method.non_null_slots.len();
        let types = method.type_facts.len();
        method.non_null_slots.extend_from_slice(narrowed);
        let continues = self.block(method, body)?;
        method.non_null_slots.truncate(facts);
        method.type_facts.truncate(types);
        method.truncate_variables(variables);
        method.locals.truncate(locals);
        self.sync_locals(method);
        Ok(continues)
    }

    fn loop_over(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        values: &'a Expr<'a>,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        if let Expr::Range(start, end, inclusive) = values {
            return self.loop_range(method, name, start, end, *inclusive, body);
        }
        if let Expr::Progression(start, end, inclusive, down, step) = values {
            return self.loop_progression(
                method,
                name,
                start,
                end,
                *inclusive,
                *down,
                step.as_deref(),
                body,
            );
        }
        let mut kind = self.expression(method, values, 0)?;
        if kind == Kind::Text {
            // Looping over text visits its characters.
            let chars = Name {
                text: "toCharArray",
                offset: name.offset,
            };
            kind = self
                .call_method_on(method, kind, chars, &[], 0, false)?
                .expect("toCharArray returns a list");
        }
        if self.is_java_map(kind) {
            // Looping over a map visits its entries.
            let entries = Name {
                text: "entrySet",
                offset: name.offset,
            };
            kind = self
                .call_method_on(method, kind, entries, &[], 0, false)?
                .expect("entrySet returns a set");
        }
        if matches!(kind, Kind::Java(_)) || self.symbols.types.native_class(kind).is_some() {
            let types = &self.symbols.types;
            let view = self
                .symbols
                .java_generic_views(kind)
                .into_iter()
                .find(|&view| {
                    types
                        .runtime_class_name(view)
                        .is_some_and(|binary| types.is_java_subclass(&binary, "java/lang/Iterable"))
                });
            if let Some(view) = view {
                let element = types
                    .substitution(view, "java/lang/Iterable")
                    .get("T")
                    .copied()
                    .unwrap_or(Kind::Any);
                return self.loop_iterable(method, name, element, body);
            }
        }
        if let Kind::MutableList(list) = kind {
            return self.loop_mutable(method, name, list, body);
        }
        let Kind::List(list) = kind else {
            return Err(Error {
                offset: values.offset(),
                message: "A for loop needs a list.",
            });
        };
        let element = list.element();
        method.max_stack = method.max_stack.max(2);
        let array = method.local(self.local_type(kind));
        let index = method.local(Local::Int);
        let item = method.local(self.local_type(element));
        method.access(0x3a, array); // astore
        push_int(&mut method.code, 0);
        method.access(0x36, index); // istore
        let header = method.code.len();
        method.frame(array as usize + 2);
        method.access(0x15, index); // iload
        method.access(0x19, array);
        method.code.push(0xbe); // arraylength
        let branch = method.code.len();
        instruction(&mut method.code, 0xa2, 0); // if_icmpge
        method.access(0x19, array);
        method.access(0x15, index);
        method.code.push(list.load());
        self.guard_array_element(method, element, 1);
        method.access(element.store(), item);
        method.variables.push(Binding {
            name: name.text,
            slot: item,
            kind: element,
            mutable: false,
        });
        method.loops.push(LoopFrame::new(None));
        let continues = self.scoped_block(method, body, &[]);
        let frame = method.loops.pop().unwrap();
        method.pop_variable();
        if continues? || !frame.continues.is_empty() {
            method.patch_branches(&frame.continues, name.offset)?;
            if !frame.continues.is_empty() {
                method.frame(array as usize + 2);
            }
            method.increment(index);
            let back = method.jump();
            method.patch_branch(back, header, name.offset)?;
        }
        let exit = method.code.len();
        method.patch_branch(branch, exit, name.offset)?;
        method.patch_branches(&frame.breaks, name.offset)?;
        method.frame(array as usize);
        method.locals.truncate(array as usize);
        Ok(())
    }

    /// `for x in a downTo b` and ranges with `step`.
    #[allow(clippy::too_many_arguments)]
    fn loop_progression(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        start: &'a Expr<'a>,
        end: &'a Expr<'a>,
        inclusive: bool,
        down: bool,
        step: Option<&'a Expr<'a>>,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        let kind = self.expression(method, start, 0)?;
        if !matches!(kind, Kind::Int | Kind::Char) {
            return Err(Error {
                offset: start.offset(),
                message: "A range needs Int or Char values, such as 1..10.",
            });
        }
        let counter = method.local(Local::Int);
        method.access(0x36, counter); // istore
        let actual = self.expression(method, end, 0)?;
        self.symbols.require(end, actual, kind)?;
        let limit = method.local(Local::Int);
        method.access(0x36, limit);
        let stride = method.local(Local::Int);
        match step {
            Some(step) => {
                let actual = self.expression(method, step, 0)?;
                self.symbols.require(step, actual, Kind::Int)?;
            }
            None => push_int(&mut method.code, 1),
        }
        method.access(0x36, stride);
        method.max_stack = method.max_stack.max(5);
        // A step must be positive.
        method.access(0x15, stride);
        let positive = method.code.len();
        instruction(&mut method.code, 0x9d, 0); // ifgt
        let error = self.pool_class("java/lang/IllegalArgumentException");
        instruction(&mut method.code, 0xbb, error); // new
        method.code.extend([0x59]); // dup
        let init = self.pool.member_for(10, error, "<init>", "()V");
        instruction(&mut method.code, 0xb7, init);
        method.code.push(0xbf); // athrow
        let live = stride as usize + 1;
        method.patch_branch(positive, method.code.len(), name.offset)?;
        method.frame(live);
        let header = method.code.len();
        method.frame(live);
        method.access(0x15, counter);
        method.access(0x15, limit);
        let branch = method.code.len();
        let opcode = match (down, inclusive) {
            (true, _) => 0xa1,      // if_icmplt
            (false, true) => 0xa3,  // if_icmpgt
            (false, false) => 0xa2, // if_icmpge
        };
        instruction(&mut method.code, opcode, 0);
        method.variables.push(Binding {
            name: name.text,
            slot: counter,
            kind,
            mutable: false,
        });
        method.loops.push(LoopFrame::new(None));
        let continues = self.scoped_block(method, body, &[]);
        let frame = method.loops.pop().unwrap();
        method.pop_variable();
        let mut last = None;
        if continues? || !frame.continues.is_empty() {
            method.patch_branches(&frame.continues, name.offset)?;
            if !frame.continues.is_empty() {
                method.frame(live);
            }
            // Stop before the counter can overflow.
            let (first, second) = if down {
                (counter, limit)
            } else {
                (limit, counter)
            };
            method.access(0x15, first);
            method.code.push(0x85); // i2l
            method.access(0x15, second);
            method.code.push(0x85);
            method.code.push(0x65); // lsub
            method.access(0x15, stride);
            method.code.push(0x85);
            method.code.push(0x94); // lcmp
            last = Some(method.code.len());
            instruction(&mut method.code, 0x9b, 0); // iflt
            method.access(0x15, counter);
            method.access(0x15, stride);
            method.code.push(if down { 0x64 } else { 0x60 });
            method.access(0x36, counter);
            let back = method.jump();
            method.patch_branch(back, header, name.offset)?;
        }
        method.patch_branch(branch, method.code.len(), name.offset)?;
        if let Some(last) = last {
            method.patch_branch(last, method.code.len(), name.offset)?;
        }
        method.patch_branches(&frame.breaks, name.offset)?;
        method.frame(counter as usize);
        method.locals.truncate(counter as usize);
        Ok(())
    }

    fn loop_range(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        start: &'a Expr<'a>,
        end: &'a Expr<'a>,
        inclusive: bool,
        body: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        let kind = self.expression(method, start, 0)?;
        if !matches!(kind, Kind::Int | Kind::Char) {
            return Err(Error {
                offset: start.offset(),
                message: "A range needs Int or Char values, such as 1..10.",
            });
        }
        let counter = method.local(Local::Int);
        method.access(0x36, counter); // istore
        let actual = self.expression(method, end, 0)?;
        self.symbols.require(end, actual, kind)?;
        let limit = method.local(Local::Int);
        method.access(0x36, limit);
        method.max_stack = method.max_stack.max(2);
        let header = method.code.len();
        method.frame(limit as usize + 1);
        method.access(0x15, counter);
        method.access(0x15, limit);
        let branch = method.code.len();
        instruction(&mut method.code, if inclusive { 0xa3 } else { 0xa2 }, 0); // if_icmpgt / if_icmpge
        method.variables.push(Binding {
            name: name.text,
            slot: counter,
            kind,
            mutable: false,
        });
        method.loops.push(LoopFrame::new(None));
        let continues = self.scoped_block(method, body, &[]);
        let frame = method.loops.pop().unwrap();
        method.pop_variable();
        if continues? || !frame.continues.is_empty() {
            method.patch_branches(&frame.continues, name.offset)?;
            if !frame.continues.is_empty() {
                method.frame(limit as usize + 1);
            }
            method.increment(counter);
            let back = method.jump();
            method.patch_branch(back, header, name.offset)?;
        }
        method.patch_branch(branch, method.code.len(), name.offset)?;
        method.patch_branches(&frame.breaks, name.offset)?;
        method.frame(counter as usize);
        method.locals.truncate(counter as usize);
        Ok(())
    }

    fn set_index(
        &mut self,
        method: &mut Method<'a>,
        list: &'a Expr<'a>,
        index: &'a Expr<'a>,
        operator: Option<Arithmetic>,
        value: &'a Expr<'a>,
    ) -> Result<(), Error> {
        let kind = self.expression(method, list, 0)?;
        if self.is_java_map(kind) {
            return self.map_put(method, kind, index, operator, value);
        }
        let (element_list, mutable) = match kind {
            Kind::List(list) => (list, false),
            Kind::MutableList(list) => (list, true),
            Kind::Object(_)
            | Kind::Applied(_)
            | Kind::Java(_)
            | Kind::Interface(_)
            | Kind::AppliedInterface(_)
                if operator.is_none() =>
            {
                // `a[i] = v` calls `a.set(i, v)` on an object.
                let set = Name {
                    text: "set",
                    offset: list.offset(),
                };
                let args: &'a [Expr<'a>] =
                    Box::leak(vec![index.clone(), value.clone()].into_boxed_slice());
                if let Some(kind) = self.call_method_on(method, kind, set, args, 0, true)? {
                    method.code.push(kind.pop_opcode());
                }
                return Ok(());
            }
            _ => {
                return Err(Error {
                    offset: list.offset(),
                    message: "Only list items can be changed with [index].",
                });
            }
        };
        let element = element_list.element();
        let receiver = self.local_type(kind);
        method.stack.push(receiver);
        let actual = self.expression(method, index, 1)?;
        self.symbols.require(index, actual, Kind::Int)?;
        method.stack.push(Local::Int);
        method.max_stack = method.max_stack.max(2);
        let mut result = if let Some(operator) = operator {
            method.code.push(0x5c); // dup2
            method.max_stack = method.max_stack.max(4);
            if mutable {
                let get = self.runtime_member(ARRAY_LIST, "get", "(I)Ljava/lang/Object;", 10);
                instruction(&mut method.code, 0xb6, get);
                self.unbox_element(method, element, 2);
            } else {
                method.code.push(element_list.load());
            }
            method.max_stack = method.max_stack.max(2 + element.slots());
            self.arithmetic_tail(method, value, element, &[(operator, value)], 2)?
        } else {
            self.expression_as(method, value, 2, Some(element))?
        };
        if result != element {
            self.coerce(method, value, result, element, 2)?;
            result = element;
        }
        method.stack.clear();
        if mutable {
            self.box_value(method, result);
            let set = self.runtime_member(
                ARRAY_LIST,
                "set",
                "(ILjava/lang/Object;)Ljava/lang/Object;",
                10,
            );
            instruction(&mut method.code, 0xb6, set);
            method.code.push(0x57); // pop the previous item
        } else {
            method.code.push(element_list.store());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct LoopFrame {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    continue_target: Option<usize>,
}

impl LoopFrame {
    fn new(continue_target: Option<usize>) -> Self {
        Self {
            breaks: Vec::new(),
            continues: Vec::new(),
            continue_target,
        }
    }
}

fn numeric_rank(kind: Kind) -> u8 {
    match kind {
        Kind::Int => 0,
        Kind::Long => 1,
        _ => 2,
    }
}

fn widening_opcode(from: Kind, to: Kind) -> u8 {
    match (from, to) {
        (Kind::Int, Kind::Long) => 0x85,    // i2l
        (Kind::Int, Kind::Double) => 0x87,  // i2d
        (Kind::Long, Kind::Double) => 0x8a, // l2d
        _ => unreachable!("only Int and Long values widen"),
    }
}

/// The number of single-letter edits between two short names.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, a) in left.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = if a == *b {
                previous
            } else {
                1 + previous.min(current).min(row[j])
            };
            previous = current;
        }
    }
    row[right.len()]
}

/// `message`, followed by the closest of `known` if one looks like a typo.
fn suggest(message: &'static str, wanted: &str, known: &[&str]) -> &'static str {
    let wanted = wanted.split('<').next().unwrap_or(wanted);
    let lowered = wanted.to_lowercase();
    let limit = (wanted.chars().count() / 3).clamp(1, 3);
    let best = known
        .iter()
        .filter(|name| !name.starts_with("__") && !name.contains('$') && **name != wanted)
        .filter(|name| name.len().abs_diff(wanted.len()) <= limit)
        .map(|name| (edit_distance(&lowered, &name.to_lowercase()), *name))
        .filter(|(distance, _)| *distance <= limit)
        .min();
    match best {
        Some((_, name)) => Box::leak(format!("{message} Did you mean {name}?").into_boxed_str()),
        None => message,
    }
}

fn at(name: Name<'_>, message: &'static str) -> Error {
    Error {
        offset: name.offset,
        message,
    }
}

impl Symbols<'_> {
    fn require(&self, expr: &Expr<'_>, actual: Kind, expected: Kind) -> Result<(), Error> {
        if self.types.bound(actual).is_some()
            && !self.types.same_kind(actual, expected)
            && self.bound_fits(actual, expected)
            && (matches!(expected, Kind::Nullable(_)) || !matches!(actual, Kind::Nullable(_)))
        {
            return Ok(());
        }
        // JVM subclass checks erase type arguments. Validate the parameterized
        // view first, including Java superclasses of Mammoth classes.
        if actual != Kind::Null && !self.java_type_arguments_fit(actual, expected) {
            return Err(Error {
                offset: expr.offset(),
                message: "Java generic type arguments must match the declared type.",
            });
        }
        let nullable = matches!(expected, Kind::Nullable(_));
        let expected_kind = expected.non_null();
        let actual_kind = if nullable { actual.non_null() } else { actual };
        let implements = match (self.types.native_class(actual_kind), expected_kind) {
            (Some(_), Kind::AppliedInterface(_)) => self
                .interface_view(
                    actual_kind,
                    self.types.interface_root(expected_kind).unwrap(),
                )
                .is_some_and(|view| self.applied_fits(view, expected_kind)),
            (Some(_), Kind::Applied(_)) => self
                .class_view(actual_kind, self.types.native_class(expected_kind).unwrap())
                .is_some_and(|view| self.applied_fits(view, expected_kind)),
            (Some(class), expected) => self.object_is(class, expected),
            (None, Kind::Java(index)) if self.types.interface_root(actual_kind).is_some() => {
                let expected = self.types.java_name(index);
                self.java_parents(actual_kind).into_iter().any(|parent| {
                    self.types
                        .runtime_class_name(parent)
                        .is_some_and(|name| self.types.is_java_subclass(&name, &expected))
                })
            }
            (None, Kind::Interface(_) | Kind::AppliedInterface(_))
                if self.types.interface_root(actual_kind).is_some() =>
            {
                self.interface_view(
                    actual_kind,
                    self.types.interface_root(expected_kind).unwrap(),
                )
                .is_some_and(|view| self.applied_fits(view, expected_kind))
            }
            _ => false,
        };
        let java_object = (expected_kind == Kind::Any
            || matches!(expected_kind, Kind::Java(index) if self.types.java_name(index) == "java/lang/Object"))
            && actual.is_reference()
            && actual != Kind::Null
            && (nullable || actual == actual.non_null());
        // Java classes: a subclass fits, and type arguments are not checked.
        let java_subtype = (matches!(expected_kind, Kind::Java(_))
            || expected.is_nullable_primitive())
            && actual.is_reference()
            && actual != Kind::Null
            && (nullable || actual == actual.non_null())
            && self
                .types
                .runtime_class_name(actual)
                .zip(
                    self.types
                        .runtime_class_name(if expected.is_nullable_primitive() {
                            expected
                        } else {
                            expected_kind
                        }),
                )
                .is_some_and(|(actual, expected)| self.types.is_java_subclass(&actual, &expected));
        if self.types.same_kind(actual_kind, expected_kind)
            || implements
            || java_object
            || java_subtype
            || (nullable && actual == Kind::Null)
        {
            return Ok(());
        }
        if actual.is_reference()
            && matches!(actual, Kind::Null | Kind::Nullable(_))
            && !nullable
            && expected.is_reference()
        {
            return Err(Error {
                offset: expr.offset(),
                message: "Expected a non-null value here. Check for null and use requireValue.",
            });
        }
        Err(Error {
            offset: expr.offset(),
            message: match expected {
                Kind::Text => "Expected Text here.",
                Kind::Int => "Expected an Int here.",
                Kind::Long => "Expected a Long here.",
                Kind::Char => "Expected a Char here, such as 'a'.",
                Kind::Double => "Expected a Double here. Use toDouble() to convert an Int.",
                Kind::Bool => "Expected a Bool here. Use true, false, or an integer comparison.",
                Kind::Object(_) | Kind::Java(_) | Kind::Applied(_) => {
                    "Expected an object of the declared class here."
                }
                Kind::Interface(_) | Kind::AppliedInterface(_) => {
                    "Expected an object that implements the declared interface here."
                }
                Kind::List(_) | Kind::MutableList(_) => {
                    "Expected a list with the declared type and element type."
                }
                Kind::Nullable(_) => "Expected a value of the declared nullable type here.",
                Kind::Any => "Expected a non-null value here.",
                Kind::TypeParameter(_) => {
                    "Expected a value of the declared type parameter. Generic code must work for every allowed type."
                }
                Kind::Function(_) => {
                    "Expected a function with the declared parameter and result types here."
                }
                Kind::Null => unreachable!("null alone cannot be a declared or inferred type"),
            },
        })
    }
}

impl Kind {
    fn nullable(self) -> Option<Self> {
        let reference = match self {
            Self::Int => Reference::Int,
            Self::Long => Reference::Long,
            Self::Char => Reference::Char,
            Self::Double => Reference::Double,
            Self::Bool => Reference::Bool,
            Self::Text => Reference::Text,
            Self::Any => Reference::Any,
            Self::TypeParameter(index) => Reference::TypeParameter(index),
            Self::Applied(index) => Reference::Applied(index),
            Self::AppliedInterface(index) => Reference::AppliedInterface(index),
            Self::Function(index) => Reference::Function(index),
            Self::Object(index) => Reference::Object(index),
            Self::Interface(index) => Reference::Interface(index),
            Self::Java(index) => Reference::Java(index),
            Self::List(kind) => Reference::List(kind),
            Self::MutableList(kind) => Reference::MutableList(kind),
            _ => return None,
        };
        Some(Self::Nullable(reference))
    }

    fn non_null(self) -> Self {
        match self {
            Self::Nullable(Reference::TypeParameter(index)) => Self::TypeParameter(index),
            Self::Nullable(Reference::Applied(index)) => Self::Applied(index),
            Self::Nullable(Reference::AppliedInterface(index)) => Self::AppliedInterface(index),
            Self::Nullable(Reference::Int) => Self::Int,
            Self::Nullable(Reference::Long) => Self::Long,
            Self::Nullable(Reference::Char) => Self::Char,
            Self::Nullable(Reference::Double) => Self::Double,
            Self::Nullable(Reference::Bool) => Self::Bool,
            Self::Nullable(Reference::Text) => Self::Text,
            Self::Nullable(Reference::Any) => Self::Any,
            Self::Nullable(Reference::Function(index)) => Self::Function(index),
            Self::Nullable(Reference::Object(index)) => Self::Object(index),
            Self::Nullable(Reference::Interface(index)) => Self::Interface(index),
            Self::Nullable(Reference::Java(index)) => Self::Java(index),
            Self::Nullable(Reference::List(kind)) => Self::List(kind),
            Self::Nullable(Reference::MutableList(kind)) => Self::MutableList(kind),
            kind => kind,
        }
    }

    fn is_nullable_primitive(self) -> bool {
        matches!(
            self,
            Self::Nullable(
                Reference::Int
                    | Reference::Long
                    | Reference::Char
                    | Reference::Double
                    | Reference::Bool
            )
        )
    }

    fn is_java_value(self) -> bool {
        matches!(
            self.non_null(),
            Self::Text
                | Self::Int
                | Self::Long
                | Self::Char
                | Self::Bool
                | Self::Double
                | Self::Java(_)
                | Self::Any
        )
    }

    fn descriptor(self, types: &Types<'_>) -> Cow<'static, str> {
        match self {
            Self::Text => Cow::Borrowed("Ljava/lang/String;"),
            Self::Int => Cow::Borrowed("I"),
            Self::Double => Cow::Borrowed("D"),
            Self::Long => Cow::Borrowed("J"),
            Self::Char => Cow::Borrowed("C"),
            Self::Bool => Cow::Borrowed("Z"),
            Self::Object(_) | Self::Interface(_) => {
                Cow::Owned(format!("L{};", types.binary_name(self)))
            }
            Self::Java(index) => {
                let name = types.java_name(index);
                if name.starts_with('[') {
                    Cow::Owned(name)
                } else {
                    Cow::Owned(format!("L{name};"))
                }
            }
            Self::Any => Cow::Borrowed("Ljava/lang/Object;"),
            Self::TypeParameter(index) => types.parameter_erasure(index).descriptor(types),
            Self::Applied(_) | Self::AppliedInterface(_) => {
                Cow::Owned(format!("L{};", types.binary_name(self)))
            }
            Self::Function(index) => Cow::Owned(format!(
                "L{};",
                types
                    .function_type(index)
                    .interface()
                    .expect("function types are checked when declared")
                    .0
            )),
            Self::List(kind) => kind.descriptor(types),
            Self::MutableList(_) => Cow::Borrowed("Ljava/util/ArrayList;"),
            Self::Nullable(_) if self.is_nullable_primitive() => Cow::Owned(format!(
                "L{};",
                Types::boxed_class(self.non_null()).unwrap()
            )),
            Self::Nullable(_) => self.non_null().descriptor(types),
            Self::Null => unreachable!("null alone cannot be a declared type"),
        }
    }

    fn is_reference(self) -> bool {
        !matches!(
            self,
            Self::Int | Self::Bool | Self::Double | Self::Long | Self::Char
        )
    }

    fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::Long | Self::Double)
    }

    fn slots(self) -> u16 {
        if matches!(self, Self::Double | Self::Long) {
            2
        } else {
            1
        }
    }

    fn load(self) -> u8 {
        match self {
            Self::Int | Self::Bool | Self::Char => 0x15, // iload
            Self::Long => 0x16,                          // lload
            Self::Double => 0x18,                        // dload
            _ => 0x19,                                   // aload
        }
    }

    fn store(self) -> u8 {
        match self {
            Self::Int | Self::Bool | Self::Char => 0x36, // istore
            Self::Long => 0x37,                          // lstore
            Self::Double => 0x39,                        // dstore
            _ => 0x3a,                                   // astore
        }
    }

    fn return_opcode(self) -> u8 {
        match self {
            Self::Int | Self::Bool | Self::Char => 0xac, // ireturn
            Self::Long => 0xad,                          // lreturn
            Self::Double => 0xaf,                        // dreturn
            _ => 0xb0,                                   // areturn
        }
    }

    fn pop_opcode(self) -> u8 {
        if self.slots() == 2 { 0x58 } else { 0x57 } // pop2 or pop
    }
}

/// Element types that `ListKind` stores by number, such as `List<Int>` in
/// `List<List<Int>>`. A `Kind` is a plain value, so one table can serve
/// every program the compiler handles.
const FIELD_BEFORE_START: &str = "This field is read before its starting value is set, so it would still be null or zero. Declare it above this line.";

static NESTED_ELEMENTS: std::sync::Mutex<Vec<Kind>> = std::sync::Mutex::new(Vec::new());

impl ListKind {
    fn for_element(kind: Kind) -> Option<Self> {
        Some(match kind {
            Kind::Text => Self::Text,
            Kind::Int => Self::Int,
            Kind::Bool => Self::Bool,
            Kind::Double => Self::Double,
            Kind::Long => Self::Long,
            Kind::Char => Self::Char,
            Kind::Object(index) => Self::Object(index),
            Kind::Interface(index) => Self::Interface(index),
            Kind::Java(index) => Self::Java(index),
            Kind::Any => Self::Any,
            Kind::Null => return None,
            nested => {
                let mut table = NESTED_ELEMENTS.lock().unwrap();
                let index = table
                    .iter()
                    .position(|existing| *existing == nested)
                    .unwrap_or_else(|| {
                        table.push(nested);
                        table.len() - 1
                    });
                Self::Nested(index)
            }
        })
    }

    fn element(self) -> Kind {
        match self {
            Self::Text => Kind::Text,
            Self::Int => Kind::Int,
            Self::Bool => Kind::Bool,
            Self::Double => Kind::Double,
            Self::Long => Kind::Long,
            Self::Char => Kind::Char,
            Self::Byte => Kind::Int,
            Self::Object(index) => Kind::Object(index),
            Self::Interface(index) => Kind::Interface(index),
            Self::Java(index) => Kind::Java(index),
            Self::Any => Kind::Any,
            Self::Nested(index) => NESTED_ELEMENTS.lock().unwrap()[index],
        }
    }

    fn descriptor(self, types: &Types<'_>) -> Cow<'static, str> {
        match self {
            Self::Text => Cow::Borrowed("[Ljava/lang/String;"),
            Self::Int => Cow::Borrowed("[I"),
            Self::Bool => Cow::Borrowed("[Z"),
            Self::Double => Cow::Borrowed("[D"),
            Self::Long => Cow::Borrowed("[J"),
            Self::Char => Cow::Borrowed("[C"),
            Self::Byte => Cow::Borrowed("[B"),
            _ => Cow::Owned(format!("[{}", self.element().descriptor(types))),
        }
    }

    fn load(self) -> u8 {
        match self {
            Self::Text
            | Self::Object(_)
            | Self::Interface(_)
            | Self::Java(_)
            | Self::Any
            | Self::Nested(_) => 0x32, // aaload
            Self::Int => 0x2e,    // iaload
            Self::Bool => 0x33,   // baload
            Self::Double => 0x31, // daload
            Self::Long => 0x2f,   // laload
            Self::Char => 0x34,   // caload
            Self::Byte => 0x33,   // baload
        }
    }

    fn store(self) -> u8 {
        match self {
            Self::Text
            | Self::Object(_)
            | Self::Interface(_)
            | Self::Java(_)
            | Self::Any
            | Self::Nested(_) => 0x53, // aastore
            Self::Int => 0x4f,    // iastore
            Self::Bool => 0x54,   // bastore
            Self::Double => 0x52, // dastore
            Self::Long => 0x50,   // lastore
            Self::Char => 0x55,   // castore
            Self::Byte => 0x54,   // bastore
        }
    }
}

#[derive(Clone, Copy)]
enum Local {
    Int,
    Long,
    Double,
    WideTail,
    Null,
    Object(u16),
    Uninitialized(u16),
    UninitializedThis,
}

#[derive(Clone, Copy)]
struct Binding<'a> {
    name: &'a str,
    slot: u16,
    kind: Kind,
    mutable: bool,
}

/// A `static var` or `static let` field of a Mammoth class.
#[derive(Clone, Copy)]
struct StaticField<'a> {
    name: &'a str,
    kind: Kind,
    mutable: bool,
    private: bool,
}

/// A named local variable's live code range, for debuggers.
#[derive(Clone)]
struct LocalEntry {
    /// The source name, used to match the entry with its binding.
    label: String,
    start: u16,
    /// `None` while the variable is still in scope.
    end: Option<u16>,
    name: u16,
    descriptor: u16,
    slot: u16,
}

struct MethodState {
    local_table: usize,
    open_locals: Vec<(usize, usize)>,
    variables_low: usize,
    type_facts: usize,
    code: usize,
    line_count: usize,
    last_line: Option<(u16, u16)>,
    locals: Vec<Local>,
    stack: Vec<Local>,
    variables: usize,
    non_null_slots: usize,
    frames: Vec<u8>,
    frame_count: u16,
    next_frame_base: usize,
    last_frame_start: usize,
}

#[derive(Clone)]
struct Method<'a> {
    code: Vec<u8>,
    result: Option<Kind>,
    /// A specialized generic interface body keeps primitive source checking
    /// while returning a boxed value through its Java-compatible entry.
    boxed_result: bool,
    /// A lambda whose result type comes from its first returned value.
    infers_result: bool,
    is_lambda: bool,
    locals: Vec<Local>,
    stack: Vec<Local>,
    max_stack: u16,
    max_locals: u16,
    variables: Vec<Binding<'a>>,
    /// Local slots known to hold a more specific type after an `is` check.
    type_facts: Vec<(u16, Kind)>,
    /// Local slots that hold a one-item array shared with lambdas, in place
    /// of the variable's value.
    boxed_slots: Vec<u16>,
    /// Names a lambda body copied from the enclosing method.
    captured: Vec<&'a str>,
    non_null_slots: Vec<u16>,
    loops: Vec<LoopFrame>,
    /// Labels of enclosing loops, with the index each loop will have.
    loop_labels: Vec<(usize, &'a str)>,
    tries: Vec<exceptions::TryContext<'a>>,
    /// Exception table entries: start, end, handler, catch type.
    exceptions: Vec<[u16; 4]>,
    frames: Vec<u8>,
    frame_count: u16,
    next_frame_base: usize,
    last_frame_start: usize,
    /// LineNumberTable entries: bytecode offset and 1-based source line.
    lines: Vec<(u16, u16)>,
    /// LocalVariableTable entries, in the order variables came into scope.
    local_table: Vec<LocalEntry>,
    /// The constant pool index of the name `LocalVariableTable`.
    local_table_name: u16,
    /// LocalVariableTable entries still open: the index in `variables` of
    /// the binding and the index in `local_table`, by binding index.
    open_locals: Vec<(usize, usize)>,
    /// The fewest bindings `variables` has held since locals were last
    /// synchronized; bindings below it are unchanged.
    variables_low: usize,
    /// The constant pool index of the name `LineNumberTable`.
    line_table_name: u16,
}

impl<'a> Method<'a> {
    fn new(locals: Vec<Local>) -> Self {
        Self {
            code: Vec::new(),
            result: None,
            boxed_result: false,
            infers_result: false,
            is_lambda: false,
            max_stack: 0,
            max_locals: locals.len() as u16,
            locals,
            stack: Vec::new(),
            variables: Vec::new(),
            type_facts: Vec::new(),
            boxed_slots: Vec::new(),
            captured: Vec::new(),
            non_null_slots: Vec::new(),
            loops: Vec::new(),
            loop_labels: Vec::new(),
            tries: Vec::new(),
            exceptions: Vec::new(),
            frames: Vec::new(),
            frame_count: 0,
            next_frame_base: 0,
            last_frame_start: 0,
            lines: Vec::new(),
            line_table_name: 0,
            local_table: Vec::new(),
            local_table_name: 0,
            open_locals: Vec::new(),
            variables_low: 0,
        }
    }

    /// Removes bindings that went out of scope.
    fn truncate_variables(&mut self, len: usize) {
        self.variables.truncate(len);
        self.variables_low = self.variables_low.min(len);
    }

    fn pop_variable(&mut self) {
        self.variables.pop();
        self.variables_low = self.variables_low.min(self.variables.len());
    }

    fn snapshot(&self) -> MethodState {
        MethodState {
            local_table: self.local_table.len(),
            open_locals: self.open_locals.clone(),
            variables_low: self.variables_low,
            type_facts: self.type_facts.len(),
            code: self.code.len(),
            line_count: self.lines.len(),
            last_line: self.lines.last().copied(),
            locals: self.locals.clone(),
            stack: self.stack.clone(),
            variables: self.variables.len(),
            non_null_slots: self.non_null_slots.len(),
            frames: self.frames.clone(),
            frame_count: self.frame_count,
            next_frame_base: self.next_frame_base,
            last_frame_start: self.last_frame_start,
        }
    }

    /// Discards code emitted since `snapshot`.
    fn restore(&mut self, state: MethodState) {
        self.code.truncate(state.code);
        // A statement's own entry sits at this offset and must survive.
        self.lines.truncate(state.line_count.saturating_sub(1));
        self.lines.extend(state.last_line);
        self.local_table.truncate(state.local_table);
        for entry in &mut self.local_table {
            if entry.end.is_some_and(|end| usize::from(end) > state.code) {
                entry.end = None;
            }
        }
        self.open_locals = state.open_locals;
        self.type_facts.truncate(state.type_facts);
        self.locals = state.locals;
        self.stack = state.stack;
        self.variables.truncate(state.variables);
        self.variables_low = state.variables_low.min(state.variables);
        self.non_null_slots.truncate(state.non_null_slots);
        self.frames = state.frames;
        self.frame_count = state.frame_count;
        self.next_frame_base = state.next_frame_base;
        self.last_frame_start = state.last_frame_start;
    }

    fn local(&mut self, kind: Local) -> u16 {
        let index = self.locals.len() as u16;
        // A slot at or above this one belonged to a binding now out of scope.
        self.boxed_slots.retain(|&slot| slot < index);
        self.locals.push(kind);
        if matches!(kind, Local::Double | Local::Long) {
            self.locals.push(Local::WideTail);
        }
        self.max_locals = self.max_locals.max(self.locals.len() as u16);
        index
    }

    fn push_stack(&mut self, value: Local) {
        self.stack.push(value);
        if matches!(value, Local::Double | Local::Long) {
            self.stack.push(Local::WideTail);
        }
    }

    fn pop_stack(&mut self) {
        if matches!(self.stack.pop(), Some(Local::WideTail)) {
            self.stack.pop();
        }
    }

    fn access(&mut self, opcode: u8, local: u16) {
        if let Ok(local) = u8::try_from(local) {
            self.code.extend([opcode, local]);
        } else {
            self.code.push(0xc4); // wide
            instruction(&mut self.code, opcode, local);
        }
    }

    fn binding(&self, name: Name<'a>) -> Option<Binding<'a>> {
        self.variables
            .iter()
            .rev()
            .find(|binding| binding.name == name.text)
            .copied()
    }

    fn increment(&mut self, local: u16) {
        if let Ok(local) = u8::try_from(local) {
            self.code.extend([0x84, local, 1]); // iinc
        } else {
            self.code.extend([0xc4, 0x84]); // wide iinc
            shorts(&mut self.code, &[local, 1]);
        }
    }

    fn non_null_facts(&self, condition: &Condition<'a>, when: bool) -> Vec<u16> {
        let mut slots = Vec::new();
        self.collect_non_null_facts(condition, when, &mut slots);
        slots
    }

    fn collect_non_null_facts(&self, condition: &Condition<'a>, when: bool, slots: &mut Vec<u16>) {
        match condition {
            Expr::Not(_, odd, inner) => self.collect_non_null_facts(inner, when ^ odd, slots),
            Expr::Logic(logic, terms) if when == (*logic == Logic::And) => {
                for term in terms {
                    self.collect_non_null_facts(term, when, slots);
                }
            }
            Expr::Compare(left, comparison, right)
                if matches!(
                    (comparison, when),
                    (Comparison::Eq, false) | (Comparison::Ne, true)
                ) =>
            {
                let name = match (left.as_ref(), right.as_ref()) {
                    (Expr::Variable(name), Expr::Null(_))
                    | (Expr::Null(_), Expr::Variable(name)) => *name,
                    _ => return,
                };
                // A later assignment must not invalidate the fact established by this check.
                if let Some(binding) = self.binding(name)
                    && !binding.mutable
                    && matches!(binding.kind, Kind::Nullable(_))
                {
                    slots.push(binding.slot);
                }
            }
            _ => {}
        }
    }

    fn frame(&mut self, live: usize) {
        let delta = if self.code.len() + 1 == self.next_frame_base {
            // A nested block can end at its enclosing block's branch target.
            // Replace its frame so only the enclosing block's locals are live.
            let start = self.last_frame_start;
            let delta = u16::from_be_bytes([self.frames[start + 1], self.frames[start + 2]]);
            self.frames.truncate(start);
            delta
        } else {
            self.frame_count += 1;
            (self.code.len() - self.next_frame_base) as u16
        };
        self.last_frame_start = self.frames.len();
        self.frames.push(255); // full_frame
        shorts(&mut self.frames, &[delta]);
        self.next_frame_base = self.code.len() + 1;
        verification_types(&mut self.frames, &self.locals[..live]);
        verification_types(&mut self.frames, &self.stack);
    }

    fn jump(&mut self) -> usize {
        let offset = self.code.len();
        instruction(&mut self.code, 0xa7, 0); // goto
        offset
    }

    fn patch_branches(&mut self, branches: &[usize], source_offset: usize) -> Result<(), Error> {
        for &branch in branches {
            self.patch_branch(branch, self.code.len(), source_offset)?;
        }
        Ok(())
    }

    fn patch_branch(&mut self, from: usize, to: usize, source_offset: usize) -> Result<(), Error> {
        let displacement = i16::try_from(to as isize - from as isize).map_err(|_| Error {
            offset: source_offset,
            message: "This block exceeds the JVM branch size supported by this compiler.",
        })?;
        self.code[from + 1..from + 3].copy_from_slice(&displacement.to_be_bytes());
        Ok(())
    }

    fn write(
        self,
        output: &mut Vec<u8>,
        access: u16,
        name: u16,
        kind: u16,
        code_name: u16,
        stack_map_name: u16,
    ) -> Result<(), Error> {
        if self.code.len() > u16::MAX as usize {
            return Err(Error {
                offset: 0,
                message: "This block exceeds the JVM method size limit.",
            });
        }
        shorts(output, &[access, name, kind, 1, code_name]);
        let frame_size = if self.frame_count == 0 {
            0
        } else {
            8 + self.frames.len() as u32
        };
        let line_size = if self.lines.is_empty() {
            0
        } else {
            8 + 4 * self.lines.len() as u32
        };
        let code_end = self.code.len() as u16;
        let locals: Vec<LocalEntry> = self
            .local_table
            .iter()
            .map(|entry| LocalEntry {
                end: Some(entry.end.unwrap_or(code_end).min(code_end)),
                ..entry.clone()
            })
            .filter(|entry| entry.end.unwrap() > entry.start)
            .collect();
        let local_size = if locals.is_empty() {
            0
        } else {
            8 + 10 * locals.len() as u32
        };
        let exception_size = 8 * self.exceptions.len() as u32;
        word(
            output,
            self.code.len() as u32 + 12 + frame_size + exception_size + line_size + local_size,
        );

        shorts(output, &[self.max_stack, self.max_locals]);
        word(output, self.code.len() as u32);
        output.extend(self.code);
        shorts(output, &[self.exceptions.len() as u16]);
        for entry in &self.exceptions {
            shorts(output, entry);
        }
        shorts(
            output,
            &[u16::from(self.frame_count != 0)
                + u16::from(!self.lines.is_empty())
                + u16::from(!locals.is_empty())],
        );
        if self.frame_count != 0 {
            shorts(output, &[stack_map_name]);
            word(output, 2 + self.frames.len() as u32);
            shorts(output, &[self.frame_count]);
            output.extend(self.frames);
        }
        if !self.lines.is_empty() {
            shorts(output, &[self.line_table_name]);
            word(output, 2 + 4 * self.lines.len() as u32);
            shorts(output, &[self.lines.len() as u16]);
            for (offset, line) in self.lines {
                shorts(output, &[offset, line]);
            }
        }
        if !locals.is_empty() {
            shorts(output, &[self.local_table_name]);
            word(output, 2 + 10 * locals.len() as u32);
            shorts(output, &[locals.len() as u16]);
            for entry in locals {
                let end = entry.end.unwrap();
                shorts(
                    output,
                    &[
                        entry.start,
                        end - entry.start,
                        entry.name,
                        entry.descriptor,
                        entry.slot,
                    ],
                );
            }
        }
        Ok(())
    }
}

fn verification_types(bytes: &mut Vec<u8>, values: &[Local]) {
    let count = values
        .iter()
        .filter(|value| !matches!(value, Local::WideTail))
        .count();
    shorts(bytes, &[count as u16]);
    for &value in values {
        match value {
            Local::Int => bytes.push(1),
            Local::Double => bytes.push(3),
            Local::Long => bytes.push(4),
            Local::Null => bytes.push(5),
            Local::Object(class) => instruction(bytes, 7, class),
            Local::Uninitialized(offset) => instruction(bytes, 8, offset),
            Local::UninitializedThis => bytes.push(6),
            Local::WideTail => {}
        }
    }
}

fn push_int(code: &mut Vec<u8>, value: i16) {
    match value {
        -1..=5 => code.push((0x03 + value) as u8),      // iconst
        -128..=127 => code.extend([0x10, value as u8]), // bipush
        _ => instruction(code, 0x11, value as u16),     // sipush
    }
}

fn load_constant(code: &mut Vec<u8>, index: u16) {
    if let Ok(index) = u8::try_from(index) {
        code.extend([0x12, index]); // ldc
    } else {
        instruction(code, 0x13, index); // ldc_w
    }
}

#[derive(Clone)]
struct Pool {
    count: usize,
    bytes: Vec<u8>,
    members: HashMap<(u8, u16, String, String), u16>,
}

impl Pool {
    fn entry(&mut self, tag: u8) -> u16 {
        let index = self.count as u16;
        self.count += 1;
        self.bytes.push(tag);
        index
    }

    fn utf8(&mut self, bytes: &[u8]) -> u16 {
        let index = self.entry(1);
        shorts(&mut self.bytes, &[bytes.len() as u16]);
        self.bytes.extend_from_slice(bytes);
        index
    }

    fn ascii(&mut self, value: &str) -> u16 {
        debug_assert!(value.is_ascii());
        self.utf8(value.as_bytes())
    }

    fn reference(&mut self, tag: u8, indices: &[u16]) -> u16 {
        let index = self.entry(tag);
        shorts(&mut self.bytes, indices);
        index
    }

    fn class(&mut self, name: &str) -> u16 {
        let name = self.ascii(name);
        self.reference(7, &[name])
    }

    /// Like `member`, but reuses an existing entry for the same reference.
    fn member_for(&mut self, tag: u8, class: u16, name: &str, descriptor: &str) -> u16 {
        let key = (tag, class, name.to_owned(), descriptor.to_owned());
        if let Some(&index) = self.members.get(&key) {
            return index;
        }
        let index = self.member(tag, class, name, descriptor);
        self.members.insert(key, index);
        index
    }

    fn member(&mut self, tag: u8, class: u16, name: &str, descriptor: &str) -> u16 {
        let name = self.ascii(name);
        let descriptor = self.ascii(descriptor);
        let signature = self.reference(12, &[name, descriptor]);
        self.reference(tag, &[class, signature])
    }
}

fn instruction(code: &mut Vec<u8>, opcode: u8, operand: u16) {
    code.push(opcode);
    shorts(code, &[operand]);
}

fn shorts(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        bytes.extend(value.to_be_bytes());
    }
}

fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_be_bytes());
}

fn modified_utf8(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for unit in text.encode_utf16() {
        match unit {
            1..=0x7f => bytes.push(unit as u8),
            0..=0x7ff => bytes.extend([(0xc0 | (unit >> 6)) as u8, (0x80 | (unit & 0x3f)) as u8]),
            _ => bytes.extend([
                (0xe0 | (unit >> 12)) as u8,
                (0x80 | ((unit >> 6) & 0x3f)) as u8,
                (0x80 | (unit & 0x3f)) as u8,
            ]),
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    #[test]
    fn nullable_object_uses_its_reference_type_in_local_slots() {
        let program = crate::syntax::parse(
            r#"
                class Counter {}
                var value: Counter? = null
                while value == null { value = Counter() }
            "#,
        )
        .unwrap();
        assert!(super::emit(&program).is_ok());
    }

    #[test]
    fn narrowing_is_limited_to_immutable_bindings_and_checked_paths() {
        let cases = [
            (
                "let value: Text? = null if value != null { show(value) }",
                true,
            ),
            (
                "let value: Text? = null if value == null {} else { show(value) }",
                true,
            ),
            (
                "fun get(value: Text?): Text { if value == null { return \"none\" } return value }",
                true,
            ),
            (
                "var value: Text? = \"ok\" if value != null { value = null show(value) }",
                false,
            ),
            (
                "let value: Text? = null if value != null {} show(value)",
                false,
            ),
            (
                "let value: Text? = \"ok\" if value != null { let value: Text? = null show(value) }",
                false,
            ),
        ];
        for (source, succeeds) in cases {
            let source = format!("fun show(value: Text) {{}} {source}");
            let program = crate::syntax::parse(&source).unwrap();
            assert_eq!(super::emit(&program).is_ok(), succeeds, "{source}");
        }
    }

    #[test]
    fn boolean_branches_verify_with_uninitialized_objects_on_the_stack() {
        let program = crate::syntax::parse(
            r#"
                class Flag(number: Double, value: Bool) {
                    private let amount = number
                    private let flag = value
                    fun get(): Bool { return amount >= 1.5 and flag }
                }
                print(Flag(1.5, true or 1 / 0 == 1).get())
            "#,
        )
        .unwrap();
        let classes = super::emit(&program).unwrap();
        let result = crate::with_classes(&classes, |_, directory| {
            let output = std::process::Command::new("java")
                .args(["-Xverify:all", "-cp"])
                .arg(directory)
                .arg("Main")
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "true"
            {
                return Err(String::from_utf8_lossy(&output.stderr).into_owned());
            }
            Ok(std::process::ExitCode::SUCCESS)
        });
        result.unwrap();
    }
}
