#[derive(Debug, Clone, Copy)]
pub struct Error {
    pub offset: usize,
    pub message: &'static str,
}

impl Error {
    pub fn display(&self, path: &str, source: &str) -> String {
        diagnostic(path, source, self.offset, self.message)
    }
}

pub fn diagnostic(path: &str, source: &str, offset: usize, message: &str) -> String {
    if offset > source.len() || !source.is_char_boundary(offset) {
        let line = PRELUDE[..offset.min(PRELUDE.len())]
            .matches(char::from(10))
            .count()
            + 1;
        return format!("{path}: in the built-in library, line {line}: {message}");
    }
    let before = &source[..offset];
    let line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = source[start..offset].chars().count() + 1;
    let line_text = source[start..]
        .split('\n')
        .next()
        .unwrap()
        .trim_end_matches('\r');
    let indent: String = source[start..offset]
        .chars()
        .map(|ch| if ch == '\t' { '\t' } else { ' ' })
        .collect();
    format!("{path}:{line}:{column}: {message}\n  {line_text}\n  {indent}^")
}

#[derive(Clone, Copy)]
pub struct Name<'a> {
    pub text: &'a str,
    pub offset: usize,
}

pub struct Program<'a> {
    /// The file name written to class files' SourceFile attribute, such as
    /// `main.mam`. Set by the caller that read the file.
    pub source_name: String,
    /// The byte offset where each source line starts, for JVM line numbers.
    pub line_starts: Vec<usize>,
    pub module: Option<String>,
    pub imports: Vec<Import<'a>>,
    pub java: Vec<JavaBinding<'a>>,
    pub java_fields: Vec<JavaField<'a>>,
    pub java_classes: Vec<JavaClass<'a>>,
    pub interfaces: Vec<Interface<'a>>,
    pub classes: Vec<Class<'a>>,
    pub functions: Vec<Function<'a>>,
    pub body: Vec<Statement<'a>>,
    /// `typealias Name = Type`.
    pub aliases: Vec<(Name<'a>, Name<'a>)>,
}

pub struct Import<'a> {
    pub module: String,
    pub name: Name<'a>,
    pub local: Name<'a>,
    /// `import some.package.*` imports every class in a Java package.
    pub wildcard: bool,
}

pub struct JavaBinding<'a> {
    pub function: Function<'a>,
    pub target: Name<'a>,
}

pub struct JavaField<'a> {
    pub name: Name<'a>,
    pub annotation: Name<'a>,
    pub target: Name<'a>,
}

pub struct JavaClass<'a> {
    pub interface: bool,
    pub name: Name<'a>,
    pub target: Name<'a>,
    pub constructor: Option<Function<'a>>,
    pub methods: Vec<Function<'a>>,
}

/// How a parameterized interface type relates to its type arguments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variance {
    /// `Source<Dog>` and `Source<Animal>` are unrelated.
    Invariant,
    /// `out T`: `Source<Dog>` is a `Source<Animal>`.
    Out,
    /// `in T`: `Sink<Animal>` is a `Sink<Dog>`.
    In,
}

pub struct Interface<'a> {
    pub annotations: Vec<Annotation<'a>>,
    pub type_parameters: Vec<Name<'a>>,
    /// The declared variance of each type parameter.
    pub variances: Vec<Variance>,
    /// The bound of each type parameter, such as `Animal` in `T: Animal`.
    pub bounds: Vec<Vec<Name<'a>>>,
    pub name: Name<'a>,
    /// The interfaces this interface extends, such as `Source<T>`.
    pub extends: Vec<Name<'a>>,
    pub methods: Vec<Function<'a>>,
    /// `sealed interface`: implementations must be in the same file.
    pub is_sealed: bool,
}

pub struct Class<'a> {
    pub annotations: Vec<Annotation<'a>>,
    pub type_parameters: Vec<Name<'a>>,
    pub bounds: Vec<Vec<Name<'a>>>,
    pub name: Name<'a>,
    pub parameters: Vec<(Name<'a>, Name<'a>)>,
    pub implements: Vec<Name<'a>>,
    /// The superclass and the arguments passed to its constructor.
    pub extends: Option<(Name<'a>, Vec<Expr<'a>>)>,
    pub is_abstract: bool,
    /// `sealed class`: subclasses must be declared in the same file.
    pub is_sealed: bool,
    /// `object`: the one instance is the static field `INSTANCE`.
    pub is_object: bool,
    pub fields: Vec<Field<'a>>,
    pub methods: Vec<Function<'a>>,
    /// `static var`/`static let` fields, initialized once in declaration order.
    pub static_fields: Vec<Field<'a>>,
    /// `static fun` methods, called on the class rather than an object.
    pub static_methods: Vec<Function<'a>>,
    /// Static field initializers and `static init` blocks, in source order.
    pub static_initializers: Vec<Initializer<'a>>,
    /// Field initializers and `init` blocks, in the order they run.
    pub initializers: Vec<Initializer<'a>>,
    /// `data class`: generate `toString`, `equals`, and `hashCode`.
    pub is_data: bool,
    /// The `out` or `in` of each type parameter.
    pub variances: Vec<Variance>,
    /// For an `enum`, its values and their constructor arguments.
    pub enum_values: Option<Vec<(Name<'a>, Vec<Expr<'a>>)>>,
    /// Constructors beyond the one in the class header.
    pub secondary: Vec<SecondaryConstructor<'a>>,
    /// The class has only secondary constructors, so nothing outside it may
    /// call the generated header constructor.
    pub no_primary: bool,
    /// `class K private constructor(...)`.
    pub private_primary: bool,
    /// `implements I by value`: the interface and the field that answers it.
    pub delegates: Vec<(Name<'a>, Name<'a>)>,
    /// For an `inner class`, the name of the class it belongs to.
    pub inner_of: Option<&'a str>,
}

/// `constructor(s: Text) : this(s.length) { ... }`.
pub struct SecondaryConstructor<'a> {
    pub keyword: Name<'a>,
    pub parameters: Vec<(Name<'a>, Name<'a>)>,
    /// Whether `: this(...)` was written.
    pub delegates: bool,
    pub arguments: Vec<Expr<'a>>,
    pub body: Vec<Statement<'a>>,
}

pub enum Initializer<'a> {
    Field(usize),
    Block(Vec<Statement<'a>>),
}

pub struct Field<'a> {
    pub annotations: Vec<Annotation<'a>>,
    pub protected: bool,
    pub name: Name<'a>,
    pub annotation: Option<Name<'a>>,
    /// The starting value. `None` means the field starts as null.
    pub value: Option<Expr<'a>>,
    pub mutable: bool,
    pub private: bool,
    /// `lateinit var`: non-null, set later, and checked when it is read.
    pub is_lateinit: bool,
}

pub struct Function<'a> {
    pub annotations: Vec<Annotation<'a>>,
    pub type_parameters: Vec<Name<'a>>,
    pub bounds: Vec<Vec<Name<'a>>>,
    pub exported: bool,
    pub foreign_name: Option<Name<'a>>,
    pub name: Name<'a>,
    pub parameters: Vec<(Name<'a>, Name<'a>)>,
    pub result: Option<Name<'a>>,
    pub body: Vec<Statement<'a>>,
    pub private: bool,
    /// Visible to the declaring class and its subclasses (JVM ACC_PROTECTED).
    pub protected: bool,
    pub is_abstract: bool,
    pub is_override: bool,
    /// For `fun Text.shout()`, the written name `shout`. The function's own
    /// name is then a hidden one, and `this` is its first parameter.
    pub extension_of: Option<&'a str>,
    /// Whether this came from the built-in library rather than the program.
    pub in_prelude: bool,
    /// Whether the last parameter was written `vararg`.
    pub vararg: bool,
    /// For one of several declarations of a name, the written name.
    pub overload_of: Option<&'a str>,
}

pub struct Annotation<'a> {
    pub name: Name<'a>,
    pub arguments: Vec<(Name<'a>, AnnotationArgument<'a>)>,
}

/// Annotation values have their own grammar: nested annotations are constants,
/// not expressions that can execute in an ordinary Mammoth program.
pub enum AnnotationArgument<'a> {
    Constant(Expr<'a>),
    Nested(Box<Annotation<'a>>),
    Array(usize, Vec<AnnotationArgument<'a>>),
}

impl AnnotationArgument<'_> {
    pub fn offset(&self) -> usize {
        match self {
            Self::Constant(value) => value.offset(),
            Self::Nested(value) => value.name.offset,
            Self::Array(offset, _) => *offset,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Text,
    Int,
    Long,
    Char,
    Double,
    Bool,
    Object(usize),
    Interface(usize),
    Java(usize),
    List(ListKind),
    MutableList(ListKind),
    Nullable(Reference),
    Null,
    /// Any non-null reference, Java's `Object`.
    Any,
    /// A function value, such as `(Int) -> Int`.
    Function(usize),
    Applied(usize),
    /// A parameterized Mammoth interface, such as `Producer<Int>`.
    AppliedInterface(usize),
    TypeParameter(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reference {
    Applied(usize),
    AppliedInterface(usize),
    TypeParameter(usize),
    Int,
    Long,
    Char,
    Double,
    Bool,
    Any,
    Function(usize),
    Text,
    Object(usize),
    Interface(usize),
    Java(usize),
    List(ListKind),
    MutableList(ListKind),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
    Text,
    Int,
    Long,
    Char,
    /// A Java `byte[]`. Its elements read and write as `Int`.
    Byte,
    Bool,
    Double,
    Object(usize),
    Interface(usize),
    Java(usize),
    Any,
    /// Any other element type, such as a list, a function, or a nullable
    /// value. The number identifies the interned element type.
    Nested(usize),
}

#[derive(Clone)]
pub enum WhenCondition<'a> {
    /// `not in 1..5` or `!is Text`.
    Not(Box<WhenCondition<'a>>),
    /// The subject equals this value (or, without a subject, this is true).
    Value(Expr<'a>),
    Is(Name<'a>),
    /// `in start..end` or `in start until end`.
    In(Expr<'a>, Expr<'a>, bool),
}

#[derive(Clone)]
pub struct WhenBranch<'a> {
    /// Empty for the `else` branch.
    pub conditions: Vec<WhenCondition<'a>>,
    pub body: Vec<Statement<'a>>,
    /// The branch's result when `when` is used as a value.
    pub value: Option<Box<Expr<'a>>>,
}

#[derive(Clone)]
pub struct Catch<'a> {
    pub name: Name<'a>,
    /// The exception type to catch. `None` catches any `Exception`.
    pub kind: Option<Name<'a>>,
    pub body: Vec<Statement<'a>>,
}

#[derive(Clone)]
pub enum Statement<'a> {
    /// `return value`, or a bare `return` in a function without a result.
    Return(Name<'a>, Option<Expr<'a>>),
    Try(
        Name<'a>,
        Vec<Statement<'a>>,
        Vec<Catch<'a>>,
        Option<Vec<Statement<'a>>>,
    ),
    Throw(Name<'a>, Expr<'a>),
    When(Name<'a>, Option<Expr<'a>>, Vec<WhenBranch<'a>>),
    Call(Name<'a>, Vec<Expr<'a>>),
    MethodCall(Expr<'a>),
    For(Name<'a>, Expr<'a>, Vec<Statement<'a>>),
    Binding(Name<'a>, Expr<'a>, bool, Option<Name<'a>>),
    Assign(Name<'a>, Expr<'a>),
    /// `list[index] = value`, or `list[index] += value` when an operator is present.
    SetIndex(Expr<'a>, Expr<'a>, Option<Arithmetic>, Expr<'a>),
    /// `object.field = value`, or `object.field += value` when an operator is present.
    SetField(Expr<'a>, Name<'a>, Option<Arithmetic>, Expr<'a>),
    Break(Name<'a>),
    Continue(Name<'a>),
    While(Name<'a>, Condition<'a>, Vec<Statement<'a>>),
    /// `do { ... } while condition`.
    DoWhile(Name<'a>, Vec<Statement<'a>>, Condition<'a>),
    /// `name@ for ...`, a loop that `break@name` can leave.
    Labeled(Name<'a>, Box<Statement<'a>>),
    If(
        Name<'a>,
        Condition<'a>,
        Vec<Statement<'a>>,
        Option<Vec<Statement<'a>>>,
    ),
}

impl Statement<'_> {
    pub fn offset(&self) -> usize {
        match self {
            Self::Return(name, _)
            | Self::Binding(name, _, _, _)
            | Self::Assign(name, _)
            | Self::While(name, _, _)
            | Self::DoWhile(name, _, _)
            | Self::Labeled(name, _)
            | Self::If(name, _, _, _)
            | Self::For(name, _, _)
            | Self::Break(name)
            | Self::Continue(name)
            | Self::Try(name, _, _, _)
            | Self::Throw(name, _)
            | Self::When(name, _, _)
            | Self::Call(name, _) => name.offset,
            Self::MethodCall(expr)
            | Self::SetIndex(expr, _, _, _)
            | Self::SetField(expr, _, _, _) => expr.offset(),
        }
    }
}

pub type Condition<'a> = Expr<'a>;

#[derive(Clone, Copy)]
pub enum Comparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Logic {
    And,
    Or,
}

#[derive(Clone, Copy)]
pub enum Arithmetic {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    Ushr,
    BitAnd,
    BitOr,
    BitXor,
}

#[derive(Clone)]
pub enum Expr<'a> {
    List(usize, Vec<Expr<'a>>),
    Index(Box<Expr<'a>>, Box<Expr<'a>>),
    Compare(Box<Expr<'a>>, Comparison, Box<Expr<'a>>),
    Logic(Logic, Vec<Expr<'a>>),
    Not(usize, bool, Box<Expr<'a>>),
    Null(usize),
    Call(Name<'a>, Vec<Expr<'a>>),
    MethodCall(Box<Expr<'a>>, Name<'a>, Vec<Expr<'a>>),
    Field(Box<Expr<'a>>, Name<'a>),
    Text(usize, Vec<Part<'a>>),
    Variable(Name<'a>),
    Int(usize, i32),
    Long(usize, i64),
    Char(usize, u16),
    Double(usize, f64),
    Bool(usize, bool),
    Arithmetic(Box<Expr<'a>>, Vec<(Arithmetic, Expr<'a>)>),
    Negate(usize, Box<Expr<'a>>),
    Lambda(Box<Lambda<'a>>),
    /// `value as Type`, or `value as? Type` (null when the value is another type).
    Cast(Box<Expr<'a>>, Name<'a>, bool),
    /// `value is Type`.
    Is(Box<Expr<'a>>, Name<'a>),
    /// `when` or `if` used as a value.
    When(usize, Option<Box<Expr<'a>>>, Vec<WhenBranch<'a>>),
    /// `receiver?.member`: the inner method call or field read, skipped when
    /// the receiver is null.
    Safe(Box<Expr<'a>>),
    /// `value ?: fallback`.
    Elvis(Box<Expr<'a>>, Box<Expr<'a>>),
    /// `start..end` (inclusive) or `start until end` (exclusive), used by `for` loops.
    Range(Box<Expr<'a>>, Box<Expr<'a>>, bool),
    /// `start downTo end` or a range with `step`: start, end, whether the
    /// end is included, whether it counts down, and the step.
    Progression(
        Box<Expr<'a>>,
        Box<Expr<'a>>,
        bool,
        bool,
        Option<Box<Expr<'a>>>,
    ),
    /// `value in start..end`: the value, the range, and whether it is `not in`.
    InRange(Box<Expr<'a>>, Box<Expr<'a>>, bool),
    /// `parameter = value` in a call.
    Named(Name<'a>, Box<Expr<'a>>),
    /// `try` used as a value: the hidden variable, a value expression that
    /// shows the type, and the statement that assigns the variable.
    TryValue(Name<'a>, Box<Expr<'a>>, Box<Statement<'a>>),
    /// `*items`, passing a list to a vararg parameter.
    Spread(Box<Expr<'a>>),
    /// `return`, `throw`, `break` or `continue` where a value could go, such
    /// as `value ?: return 0`. It never produces a value.
    Jump(Box<Statement<'a>>),
    /// A value the compiler stored in a local while rewriting a call.
    Temp(Kind, u16),
    /// A compiler-made call to a hidden static method of a class.
    StaticHelper(usize, Name<'a>, Vec<Expr<'a>>),
}

impl Expr<'_> {
    pub fn offset(&self) -> usize {
        match self {
            Self::Text(offset, _)
            | Self::Int(offset, _)
            | Self::Long(offset, _)
            | Self::Char(offset, _)
            | Self::Negate(offset, _)
            | Self::Double(offset, _)
            | Self::Bool(offset, _)
            | Self::List(offset, _)
            | Self::Null(offset) => *offset,
            Self::Named(name, _) | Self::StaticHelper(_, name, _) => name.offset,
            Self::Spread(inner) => inner.offset(),
            Self::TryValue(name, ..) => name.offset,
            Self::Temp(..) => 0,
            Self::Progression(start, ..) | Self::InRange(start, ..) => start.offset(),
            Self::Jump(statement) => statement.offset(),
            Self::Variable(name) | Self::Call(name, _) => name.offset,
            Self::MethodCall(receiver, _, _)
            | Self::Index(receiver, _)
            | Self::Field(receiver, _) => receiver.offset(),
            Self::Arithmetic(left, _)
            | Self::Compare(left, _, _)
            | Self::Range(left, _, _)
            | Self::Cast(left, _, _)
            | Self::Is(left, _)
            | Self::Elvis(left, _)
            | Self::Safe(left) => left.offset(),
            Self::When(offset, _, _) => *offset,
            Self::Logic(_, terms) => terms[0].offset(),
            Self::Lambda(lambda) => lambda.offset,
            Self::Not(offset, _, _) => *offset,
        }
    }
}

#[derive(Clone)]
pub struct Lambda<'a> {
    pub offset: usize,
    /// Parameter names with their optional types.
    pub parameters: Vec<(Name<'a>, Option<Name<'a>>)>,
    /// Whether the lambda wrote a parameter list ending in `->`.
    pub explicit: bool,
    pub body: Vec<Statement<'a>>,
    /// A final expression that gives the lambda's result.
    pub value: Option<Box<Expr<'a>>>,
    /// In `x.apply { ... }`, the implicit parameter is the receiver.
    pub receiver: bool,
}

#[derive(Clone)]
pub enum Part<'a> {
    Literal(String),
    Value(Expr<'a>),
}

/// Makes a lambda's parameter its implicit receiver, as in `x.apply { ... }`.
fn recursive_call(text: &str, name: &str) -> bool {
    let after_declaration = text.find(name).map_or("", |at| &text[at + name.len()..]);
    after_declaration.match_indices(name).any(|(at, _)| {
        after_declaration[at + name.len()..].starts_with('(')
            && !after_declaration[..at].ends_with(|ch: char| ch.is_alphanumeric() || ch == '_')
    })
}

fn with_receiver(lambda: Expr<'_>) -> Expr<'_> {
    let Expr::Lambda(mut inner) = lambda else {
        return lambda;
    };
    if inner.parameters.is_empty() {
        inner.receiver = true;
    }
    Expr::Lambda(inner)
}

/// `let x: T by lazy { ... }` becomes a hidden nullable field and a method
/// `x()` that computes it on first use. It is not synchronized.
fn lazy_field<'a>(
    name: Name<'a>,
    result: Name<'a>,
    mut body: Vec<Statement<'a>>,
    value: Expr<'a>,
    private: bool,
) -> (Field<'a>, Function<'a>) {
    let hidden = Name {
        text: Box::leak(format!("__lazy_{}", name.text).into_boxed_str()),
        offset: name.offset,
    };
    let nullable = Name {
        text: Box::leak(format!("{}?", result.text).into_boxed_str()),
        offset: result.offset,
    };
    let field = Field {
        annotations: Vec::new(),
        protected: false,
        name: hidden,
        annotation: Some(nullable),
        value: None,
        mutable: true,
        private: true,
        is_lateinit: false,
    };
    body.push(Statement::Assign(hidden, value));
    let unset = Expr::Compare(
        Box::new(Expr::Variable(hidden)),
        Comparison::Eq,
        Box::new(Expr::Null(name.offset)),
    );
    let require = Name {
        text: "requireValue",
        offset: name.offset,
    };
    let getter = Function {
        annotations: Vec::new(),
        type_parameters: Vec::new(),
        bounds: Vec::new(),
        exported: false,
        foreign_name: None,
        name,
        parameters: Vec::new(),
        result: Some(result),
        body: vec![
            Statement::If(name, unset, body, None),
            Statement::Return(
                name,
                Some(Expr::Call(require, vec![Expr::Variable(hidden)])),
            ),
        ],
        private,
        protected: false,
        is_abstract: false,
        is_override: false,
        extension_of: None,
        in_prelude: false,
        vararg: false,
        overload_of: None,
    };
    (field, getter)
}

/// Gives a data class `component1()` and so on, and `copy` with every
/// property optional, unless the class declares those itself.
fn add_data_members<'a>(class: &mut Class<'a>) {
    let properties: Vec<(Name<'a>, Name<'a>)> = class
        .parameters
        .iter()
        .filter(|(parameter, _)| {
            class
                .fields
                .iter()
                .any(|field| field.name.text == parameter.text)
        })
        .copied()
        .collect();
    let offset = class.name.offset;
    let name_at = |text: &'a str| Name { text, offset };
    let this = || Expr::Variable(name_at("this"));
    let method = |name: Name<'a>,
                  parameters: Vec<(Name<'a>, Name<'a>)>,
                  result: Name<'a>,
                  value: Expr<'a>| Function {
        annotations: Vec::new(),
        type_parameters: Vec::new(),
        bounds: Vec::new(),
        exported: false,
        foreign_name: None,
        name,
        parameters,
        result: Some(result),
        body: vec![Statement::Return(name, Some(value))],
        private: false,
        protected: false,
        is_abstract: false,
        is_override: false,
        extension_of: None,
        in_prelude: false,
        vararg: false,
        overload_of: None,
    };
    let declared = |methods: &[Function<'a>], name: &str| {
        methods.iter().any(|method| method.name.text == name)
    };
    for (index, (property, annotation)) in properties.iter().enumerate() {
        let text: &'a str = Box::leak(format!("component{}", index + 1).into_boxed_str());
        if !declared(&class.methods, text) {
            let value = Expr::Field(Box::new(this()), *property);
            class
                .methods
                .push(method(name_at(text), Vec::new(), *annotation, value));
        }
    }
    // A constructor parameter that is not a property cannot be copied.
    if properties.len() != class.parameters.len() || declared(&class.methods, "copy") {
        return;
    }
    let result: &'a str = if class.type_parameters.is_empty() {
        class.name.text
    } else {
        let arguments: Vec<&str> = class.type_parameters.iter().map(|name| name.text).collect();
        Box::leak(format!("{}<{}>", class.name.text, arguments.join(", ")).into_boxed_str())
    };
    let arguments = properties
        .iter()
        .map(|(name, _)| Expr::Variable(*name))
        .collect();
    let call = Expr::Call(class.name, arguments);
    class.methods.push(method(
        name_at("copy"),
        properties.clone(),
        name_at(result),
        call,
    ));
    for (index, (property, annotation)) in properties.iter().enumerate() {
        let text: &'a str = Box::leak(format!("copy$default${index}").into_boxed_str());
        let value = Expr::Field(Box::new(this()), *property);
        class
            .methods
            .push(method(name_at(text), Vec::new(), *annotation, value));
    }
}

/// The built-in library. Only the functions a program mentions are kept.
const PRELUDE: &str = include_str!("prelude.mam");

/// Whether `word` appears outside comments and text on some line.
fn in_code(source: &str, word: &str) -> bool {
    source.lines().any(|line| {
        let code = line.split("//").next().unwrap_or(line);
        code.split('"').step_by(2).any(|part| part.contains(word))
    })
}

/// Whether `source` calls a member named `name`, as `.name(` or `.name {`.
fn calls_member(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(at, _)| {
        let after = source[at + name.len()..].chars().next();
        source[..at].ends_with('.') && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    })
}

/// Functions of the built-in library called without a receiver.
const PRELUDE_FUNCTIONS: &[&str] = &[
    "repeat",
    "maxOf",
    "minOf",
    "println",
    "require",
    "check",
    "TODO",
    "with",
    "buildString",
    "compareBy",
    "compareByDescending",
    "buildList",
    "readLine",
    "generateSequence",
];

/// Whether `source` has a bare call of `name`, as `name(` or `name {`.
fn calls_function(source: &str, name: &str) -> bool {
    source.match_indices(name).any(|(at, _)| {
        let before = source[..at].chars().next_back();
        let after = source[at + name.len()..]
            .trim_start_matches(' ')
            .chars()
            .next();
        !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.')
            && matches!(after, Some('(' | '{'))
    })
}

/// What was written after a property's starting value.
#[derive(Default)]
struct Accessors<'a> {
    getter: Option<Vec<Statement<'a>>>,
    setter: Option<(Name<'a>, Vec<Statement<'a>>)>,
    setter_declared: bool,
    setter_private: bool,
    setter_protected: bool,
    uses_field: bool,
}

impl Accessors<'_> {
    fn declared(&self) -> bool {
        self.getter.is_some() || self.setter_declared
    }
}

#[derive(Clone, Copy)]
struct PropertyAccess {
    private: bool,
    protected: bool,
    is_override: bool,
}

/// The hidden field that stores a property reached through accessors.
fn backing_name(property: Name<'_>) -> &'static str {
    Box::leak(format!("__field_{}", property.text).into_boxed_str())
}

/// The hidden method that changes a property.
fn setter_name<'a>(property: Name<'a>) -> Name<'a> {
    Name {
        text: Box::leak(format!("{}$set", property.text).into_boxed_str()),
        offset: property.offset,
    }
}

/// The type of a literal starting value, written as a type name.
fn literal_type<'a>(value: &Expr<'a>, at: Name<'a>) -> Option<Name<'a>> {
    let text = match value {
        Expr::Int(..) => "Int",
        Expr::Long(..) => "Long",
        Expr::Double(..) => "Double",
        Expr::Bool(..) => "Bool",
        Expr::Text(..) => "Text",
        _ => return None,
    };
    Some(Name {
        text,
        offset: at.offset,
    })
}

/// A method generated for a property.
fn accessor<'a>(
    name: Name<'a>,
    parameters: Vec<(Name<'a>, Name<'a>)>,
    result: Option<Name<'a>>,
    body: Vec<Statement<'a>>,
) -> Function<'a> {
    Function {
        annotations: Vec::new(),
        type_parameters: Vec::new(),
        bounds: Vec::new(),
        exported: false,
        foreign_name: None,
        name,
        parameters,
        result,
        body,
        private: false,
        protected: false,
        is_abstract: false,
        is_override: false,
        extension_of: None,
        in_prelude: false,
        vararg: false,
        overload_of: None,
    }
}

/// Writes the forwarding methods of each `implements I by value`.
fn add_delegated_methods<'a>(program: &mut Program<'a>) -> Result<(), Error> {
    for index in 0..program.classes.len() {
        for position in 0..program.classes[index].delegates.len() {
            let (written, field) = program.classes[index].delegates[position];
            let plain = written.text.split('<').next().unwrap_or(written.text);
            let plain = plain.rsplit('.').next().unwrap_or(plain);
            let Some(interface) = program
                .interfaces
                .iter()
                .find(|interface| interface.name.text == plain)
            else {
                return Err(Error {
                    offset: written.offset,
                    message: "Only an interface declared in this file can be implemented with by.",
                });
            };
            if !interface.type_parameters.is_empty() {
                return Err(Error {
                    offset: written.offset,
                    message: "Only an interface without type parameters can be implemented with by yet.",
                });
            }
            let mut forwarders = Vec::new();
            for method in &interface.methods {
                if !method.is_abstract
                    || program.classes[index]
                        .methods
                        .iter()
                        .any(|existing| existing.name.text == method.name.text)
                {
                    continue;
                }
                let this = Expr::Variable(Name {
                    text: "this",
                    offset: written.offset,
                });
                let args = method
                    .parameters
                    .iter()
                    .map(|&(name, _)| Expr::Variable(name))
                    .collect();
                let name = Name {
                    text: method.name.text,
                    offset: written.offset,
                };
                let call =
                    Expr::MethodCall(Box::new(Expr::Field(Box::new(this), field)), name, args);
                let body = if method.result.is_some() {
                    vec![Statement::Return(name, Some(call))]
                } else {
                    vec![Statement::MethodCall(call)]
                };
                let mut forwarder = accessor(name, method.parameters.clone(), method.result, body);
                forwarder.is_override = true;
                forwarders.push(forwarder);
            }
            program.classes[index].methods.append(&mut forwarders);
        }
    }
    Ok(())
}

/// Every function whose header says `: Nothing`.
fn nothing_names(source: &str) -> Vec<&str> {
    let mut names = Vec::new();
    for (at, _) in source.match_indices("fun ") {
        let rest = &source[at + 4..];
        let header = rest.split(['\n', '{', '=']).next().unwrap_or("");
        if header.trim_end().ends_with(": Nothing")
            && let Some(name) = header.split(['(', '<']).next()
        {
            names.push(name.trim());
        }
    }
    names
}

/// Every name declared with `infix fun`, and the library's own.
fn infix_names(source: &str) -> Vec<&str> {
    let mut names = vec!["union", "intersect", "subtract"];
    for (at, _) in source.match_indices("infix fun ") {
        let rest = &source[at + "infix fun ".len()..];
        let header = rest.split(['(', '<']).next().unwrap_or("");
        // `Int.addTo<T>` has its name after the receiver's last dot, and a
        // generic receiver has its name after the closing angle bracket.
        let header = if rest[header.len()..].starts_with('<') && !header.contains('.') {
            let end = rest.find(">.").map_or(0, |end| end + 2);
            rest[end..].split(['(', '<']).next().unwrap_or("")
        } else {
            header
        };
        let name = header.rsplit('.').next().unwrap_or("").trim();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn add_prelude<'a>(program: &mut Program<'a>, source: &str) {
    let parsed = parse_source(PRELUDE).expect("the built-in library parses");
    let extensions: Vec<&str> = parsed
        .functions
        .iter()
        .filter_map(|function| function.extension_of)
        .collect();
    let destructuring = ["let (", "var (", "for ("]
        .iter()
        .any(|text| source.contains(text));
    if !destructuring
        && !calls_function(source, "Pair")
        && !calls_function(source, "Regex")
        && !in_code(source, "..")
        && !in_code(source, " union ")
        && !in_code(source, " intersect ")
        && !in_code(source, " subtract ")
        && !in_code(source, "List<")
        && !in_code(source, " in ")
        && !in_code(source, " to ")
        && !source.contains("Triple(")
        && !PRELUDE_FUNCTIONS
            .iter()
            .chain(&[
                "listOf",
                "mutableListOf",
                "setOf",
                "mutableSetOf",
                "mapOf",
                "mutableMapOf",
            ])
            .any(|name| calls_function(source, name))
        && !extensions.iter().any(|name| calls_member(source, name))
    {
        return;
    }
    let declared: Vec<&str> = program.functions.iter().map(|f| f.name.text).collect();
    let user_pair = program
        .classes
        .iter()
        .any(|class| class.name.text == "Pair")
        || program
            .interfaces
            .iter()
            .any(|interface| interface.name.text == "Pair");
    let destructures = ["let (", "var (", "for ("]
        .iter()
        .any(|text| source.contains(text));
    let list_operators =
        (source.contains(" + ") || source.contains(" - ") || source.contains("+="))
            && ["MutableList", "listOf", "ArrayList", "setOf", "mapOf"]
                .iter()
                .any(|text| source.contains(text));
    let wanted: Vec<&str> = parsed
        .functions
        .iter()
        .filter(|function| {
            function.extension_of.is_some_and(|name| {
                let needs_pair = matches!(name, "zip" | "partition");
                name == "toMutableList"
                    || (name.starts_with("__sumOf") && calls_member(source, "sumOf"))
                    || (matches!(name, "union" | "intersect" | "subtract")
                        && in_code(source, &format!(" {name} ")))
                    || (destructures && name.starts_with("component"))
                    || (name == "contains" && in_code(source, " in "))
                    || (matches!(name, "plus" | "minus") && list_operators)
                    || (name == "toSet"
                        && (calls_function(source, "setOf")
                            || calls_function(source, "mutableSetOf")))
                    || (calls_member(source, name) && !(needs_pair && user_pair))
            })
        })
        .map(|function| function.name.text)
        .collect();
    let needs_map = calls_function(source, "mapOf") || calls_function(source, "mutableMapOf");
    let needs_pair = !user_pair
        && (calls_member(source, "zip")
            || calls_member(source, "partition")
            || calls_function(source, "Pair")
            || in_code(source, " to ")
            || calls_member(source, "toList")
            || calls_member(source, "associate")
            || needs_map);
    let needs_indexed = calls_member(source, "withIndex");
    let needs_regex = calls_function(source, "Regex") || calls_member(source, "toRegex");
    let needs_sequence =
        calls_member(source, "asSequence") || calls_function(source, "generateSequence");
    for class in parsed.classes {
        if (class.name.text == "Pair" && needs_pair)
            || (class.name.text == "__Indexed" && needs_indexed)
            || ((class.name.text == "Sequence" || class.name.text.starts_with("__Seq"))
                && needs_sequence)
            || (class.name.text == "Regex"
                && needs_regex
                && !program
                    .classes
                    .iter()
                    .any(|class| class.name.text == "Regex"))
            || (class.name.text == "Triple"
                && source.contains("Triple(")
                && !program
                    .classes
                    .iter()
                    .any(|class| class.name.text == "Triple"))
        {
            program.classes.push(class);
        }
    }
    let sized_blocks = in_code(source, "List<");
    let keep = |name: &str| {
        (name.starts_with("__")
            && !name.contains("$ext$")
            && (name != "__mapOf" || needs_map)
            && (!(name.starts_with("__fill") || name == "__growList") || sized_blocks))
            || (PRELUDE_FUNCTIONS.contains(&name.split("$default$").next().unwrap_or(name))
                && calls_function(source, name.split("$default$").next().unwrap_or(name)))
            || wanted.iter().any(|wanted| {
                name == *wanted
                    || name
                        .strip_prefix(wanted)
                        .is_some_and(|rest| rest.starts_with("$default$"))
            })
    };
    for mut function in parsed.functions {
        // A map operation over the library's Pair goes when Pair does.
        let wants_pair = function
            .parameters
            .iter()
            .any(|(_, kind)| kind.text.starts_with("Pair<"));
        if keep(function.name.text)
            && !declared.contains(&function.name.text)
            && (!wants_pair || needs_pair)
        {
            function.in_prelude = true;
            program.functions.push(function);
        }
    }
    program.imports.extend(parsed.imports);
}

pub fn parse(source: &str) -> Result<Program<'_>, Error> {
    let mut program = parse_source(source)?;
    add_prelude(&mut program, source);
    Ok(program)
}

fn parse_source(source: &str) -> Result<Program<'_>, Error> {
    let (program, nested, dotted, properties, declared) =
        parse_with(source, Vec::new(), Vec::new(), Vec::new(), Vec::new(), true)?;
    let mut seen = std::collections::HashSet::new();
    let mut overloaded: Vec<&str> = Vec::new();
    for entry in declared {
        if !seen.insert(entry) && !overloaded.contains(&entry.1) {
            overloaded.push(entry.1);
        }
    }
    if nested.is_empty() && !dotted && properties.is_empty() && overloaded.is_empty() {
        return Ok(program);
    }
    // Now that every nested type and function is known, `Outer.Inner` and
    // `::name` can be read.
    let arities = program
        .functions
        .iter()
        .map(|function| {
            let kinds = function
                .parameters
                .iter()
                .map(|(_, kind)| function.type_parameters.is_empty().then_some(*kind))
                .collect();
            (function.name.text, kinds)
        })
        .chain(
            program
                .classes
                .iter()
                .filter(|class| class.type_parameters.is_empty() && !class.parameters.is_empty())
                .map(|class| {
                    let kinds = class
                        .parameters
                        .iter()
                        .map(|(_, kind)| Some(*kind))
                        .collect();
                    (class.name.text, kinds)
                }),
        )
        .collect();
    Ok(parse_with(source, nested, arities, properties, overloaded, false)?.0)
}

#[allow(clippy::type_complexity)]
fn parse_with<'a>(
    source: &'a str,
    nested_names: Vec<(&'a str, &'a str)>,
    arities: Vec<(&'a str, Vec<Option<Name<'a>>>)>,
    interface_properties: Vec<&'a str>,
    overloaded: Vec<&'a str>,
    discovering: bool,
) -> Result<
    (
        Program<'a>,
        Vec<(&'a str, &'a str)>,
        bool,
        Vec<&'a str>,
        Vec<(&'a str, &'a str)>,
    ),
    Error,
> {
    let mut parser = Parser {
        source,
        offset: 0,
        in_header: false,
        pending_defaults: Vec::new(),
        finished_helpers: Vec::new(),
        top_level_function: false,
        extra_statements: Vec::new(),
        nested_names,
        nested_found: Vec::new(),
        discovering,
        saw_dotted_type: false,
        saw_vararg: false,
        local_aliases: Vec::new(),
        hoisted_functions: Vec::new(),
        object_scopes: Vec::new(),
        lambda_prefix: Vec::new(),
        infix_names: infix_names(source),
        nothing_names: nothing_names(source),
        function_start: 0,
        current_type: "",
        declared_functions: Vec::new(),
        overloaded,
        interface_properties,
        properties_found: Vec::new(),
        arities,
        nested_classes: Vec::new(),
        nested_interfaces: Vec::new(),
    };
    let mut program = Program {
        source_name: String::new(),
        line_starts: std::iter::once(0)
            .chain(source.match_indices('\n').map(|(index, _)| index + 1))
            .collect(),
        module: None,
        imports: Vec::new(),
        java: Vec::new(),
        java_fields: Vec::new(),
        java_classes: Vec::new(),
        interfaces: Vec::new(),
        classes: Vec::new(),
        functions: Vec::new(),
        body: Vec::new(),
        aliases: Vec::new(),
    };
    parser.skip_space();
    while !parser.rest().is_empty() {
        let annotations = parser.annotations()?;
        let previous = (
            program.classes.len(),
            program.interfaces.len(),
            program.functions.len(),
        );
        let mut name = parser.name()?;
        // Top-level names are visible to the whole file and its importers.
        while matches!(name.text, "private" | "internal" | "public" | "const")
            && identifier(parser.rest_trimmed(), 0).is_some()
        {
            name = parser.name()?;
        }
        parser.skip_space();
        if name.text == "module" && identifier(parser.rest(), parser.offset).is_some() {
            if program.module.is_some() {
                return Err(Error {
                    offset: name.offset,
                    message: "Declare only one module per source file.",
                });
            }
            program.module = Some(parser.module()?);
        } else if name.text == "import" && identifier(parser.rest(), parser.offset).is_some() {
            program.imports.push(parser.import()?);
        } else if name.text == "export" && parser.take_word("fun") {
            parser.top_level_function = true;
            let mut function = parser.function(true)?;
            parser.top_level_function = false;
            function.exported = true;
            program.functions.push(function);
            for mut helper in parser.finished_helpers.drain(..) {
                helper.exported = true;
                program.functions.push(helper);
            }
        } else if name.text == "java" && parser.take_word("fun") {
            program.java.push(parser.java_binding()?);
        } else if name.text == "java" && parser.take_word("let") {
            let name = parser.binding()?;
            parser.expect(":", "Expected ':' followed by the Java field type.")?;
            let annotation = parser.annotation()?;
            let target = parser.java_target()?;
            program.java_fields.push(JavaField {
                name,
                annotation,
                target,
            });
        } else if name.text == "java" && parser.take_word("class") {
            program.java_classes.push(parser.java_class(false)?);
        } else if name.text == "java" && parser.take_word("interface") {
            program.java_classes.push(parser.java_class(true)?);
        } else if name.text == "typealias" && identifier(parser.rest_trimmed(), 0).is_some() {
            let alias = parser.binding()?;
            if !parser.take_assignment() {
                return Err(parser.error("Expected '=' after the type alias name."));
            }
            program.aliases.push((alias, parser.annotation()?));
        } else if matches!(name.text, "infix" | "inline" | "tailrec" | "operator")
            && parser.rest_trimmed().starts_with("fun ")
        {
            parser.take_word("fun");
            parser.top_level_function = true;
            program.functions.push(parser.function(true)?);
            parser.top_level_function = false;
            program.functions.append(&mut parser.finished_helpers);
        } else if name.text == "fun" {
            parser.top_level_function = true;
            program.functions.push(parser.function(true)?);
            parser.top_level_function = false;
            program.functions.append(&mut parser.finished_helpers);
        } else if name.text == "class" {
            program.classes.push(parser.class(false, false, false)?);
        } else if name.text == "abstract" && parser.take_word("class") {
            program.classes.push(parser.class(true, false, false)?);
        } else if name.text == "object" && identifier(parser.rest_trimmed(), 0).is_some() {
            let class = parser.object_declaration()?;
            program.classes.push(class);
        } else if name.text == "sealed" && parser.take_word("class") {
            let mut class = parser.class(true, false, false)?;
            class.is_sealed = true;
            program.classes.push(class);
        } else if name.text == "sealed" && parser.take_word("interface") {
            let mut interface = parser.interface()?;
            interface.is_sealed = true;
            program.interfaces.push(interface);
        } else if name.text == "open" && parser.take_word("class") {
            program.classes.push(parser.class(false, false, false)?);
        } else if name.text == "data" && parser.take_word("class") {
            let mut class = parser.class(false, true, false)?;
            add_data_members(&mut class);
            program.classes.push(class);
        } else if name.text == "enum" && identifier(parser.rest(), parser.offset).is_some() {
            parser.take_word("class");
            program.classes.push(parser.class(false, false, true)?);
        } else if name.text == "interface" {
            program.interfaces.push(parser.interface()?);
        } else {
            program.body.append(&mut parser.statement_list(name, 0)?);
        }
        if !annotations.is_empty() {
            if program.classes.len() > previous.0 {
                program.classes.last_mut().unwrap().annotations = annotations;
            } else if program.interfaces.len() > previous.1 {
                program.interfaces.last_mut().unwrap().annotations = annotations;
            } else if program.functions.len() > previous.2 {
                program.functions.last_mut().unwrap().annotations = annotations;
            } else {
                return Err(Error {
                    offset: annotations[0].name.offset,
                    message: "Place annotations before a class, interface, or function.",
                });
            }
        }
        parser.skip_space();
    }
    program.functions.append(&mut parser.hoisted_functions);
    program.classes.append(&mut parser.nested_classes);
    program.interfaces.append(&mut parser.nested_interfaces);
    add_delegated_methods(&mut program)?;
    Ok((
        program,
        parser.nested_found,
        parser.saw_dotted_type,
        parser.properties_found,
        parser.declared_functions,
    ))
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
    /// True while reading an `if`, `while`, or `for` header, where a `{`
    /// starts the block rather than a trailing lambda.
    in_header: bool,
    /// Default parameter values read but not yet made into helpers: the
    /// parameter's index, the value, and its source range.
    pending_defaults: Vec<(usize, Expr<'a>, usize, usize)>,
    /// Helpers for the function just parsed, for its caller to file.
    finished_helpers: Vec<Function<'a>>,
    /// Whether the function being parsed may be an extension function.
    top_level_function: bool,
    /// Statements that follow the one just parsed, from destructuring.
    extra_statements: Vec<Statement<'a>>,
    /// Types declared inside other types, as `(outer, inner)`, from an
    /// earlier parse of this file.
    nested_names: Vec<(&'a str, &'a str)>,
    nested_found: Vec<(&'a str, &'a str)>,
    /// The first parse of a file, which accepts `A.B` as a type before
    /// knowing which nested types exist.
    discovering: bool,
    saw_dotted_type: bool,
    saw_vararg: bool,
    /// Local functions in scope: written name, then hidden name.
    local_aliases: Vec<(&'a str, &'a str)>,
    hoisted_functions: Vec<Function<'a>>,
    /// For each `object implements ...` being read: the hidden name of the
    /// object and the members that bare names inside it refer to.
    object_scopes: Vec<(&'a str, Vec<&'a str>)>,
    /// Bindings that unpack `{ (a, b) -> ... }` parameters.
    lambda_prefix: Vec<Statement<'a>>,
    /// Names written as `infix fun`, usable as `a name b`.
    infix_names: Vec<&'a str>,
    /// Functions declared to return `Nothing`.
    nothing_names: Vec<&'a str>,
    /// Where the outermost function being parsed starts.
    function_start: usize,
    /// The type whose members are being parsed.
    current_type: &'a str,
    /// Every function and method seen, with the type it is in.
    declared_functions: Vec<(&'a str, &'a str)>,
    /// Names declared more than once in one scope, from an earlier parse.
    overloaded: Vec<&'a str>,
    /// Property names declared by interfaces, from an earlier parse.
    interface_properties: Vec<&'a str>,
    properties_found: Vec<&'a str>,
    /// Each top-level function's parameter types, from an earlier parse. A
    /// generic function has no entry for its types, only their count.
    arities: Vec<(&'a str, Vec<Option<Name<'a>>>)>,
    nested_classes: Vec<Class<'a>>,
    nested_interfaces: Vec<Interface<'a>>,
}

impl<'a> Parser<'a> {
    fn annotations(&mut self) -> Result<Vec<Annotation<'a>>, Error> {
        let mut annotations = Vec::new();
        while self.take("@") {
            annotations.push(self.annotation_use(0)?);
        }
        Ok(annotations)
    }

    fn annotation_use(&mut self, depth: usize) -> Result<Annotation<'a>, Error> {
        if depth > 64 {
            return Err(self.error("Annotation values can be nested up to 64 levels."));
        }
        let parts = self.qualified_name()?;
        let first = parts.first().unwrap();
        let last = parts.last().unwrap();
        let name = Name {
            text: &self.source[first.offset..last.offset + last.text.len()],
            offset: first.offset,
        };
        let arguments = if self.take("(") {
            self.list(")", |parser| {
                let name = parser.name()?;
                parser.expect("=", "Use named annotation arguments, such as value = 1.")?;
                Ok((name, parser.annotation_argument(depth + 1)?))
            })?
        } else {
            Vec::new()
        };
        Ok(Annotation { name, arguments })
    }

    fn annotation_argument(&mut self, depth: usize) -> Result<AnnotationArgument<'a>, Error> {
        if depth > 64 {
            return Err(self.error("Annotation values can be nested up to 64 levels."));
        }
        self.skip_space();
        let offset = self.offset;
        if self.take("@") {
            return Ok(AnnotationArgument::Nested(Box::new(
                self.annotation_use(depth)?,
            )));
        }
        if self.take("[") {
            return Ok(AnnotationArgument::Array(
                offset,
                self.list("]", |parser| parser.annotation_argument(depth + 1))?,
            ));
        }
        Ok(AnnotationArgument::Constant(self.logical(0, depth)?))
    }
    fn rest(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn skip_space(&mut self) {
        loop {
            self.offset = self.source.len()
                - self
                    .rest()
                    .trim_start_matches(|ch: char| ch.is_whitespace() || ch == ';')
                    .len();
            if self.rest().starts_with("//") {
                self.offset += self.rest().find('\n').unwrap_or(self.rest().len());
                continue;
            }
            if !self.rest().starts_with("/*") {
                break;
            }
            // Block comments nest. One left open stays for the parser to report.
            let bytes = self.rest().as_bytes();
            let mut depth = 0;
            let mut index = 0;
            let mut closed = None;
            while index + 1 < bytes.len() {
                match &bytes[index..index + 2] {
                    b"/*" => {
                        depth += 1;
                        index += 2;
                    }
                    b"*/" => {
                        depth -= 1;
                        index += 2;
                        if depth == 0 {
                            closed = Some(index);
                            break;
                        }
                    }
                    _ => index += 1,
                }
            }
            match closed {
                Some(end) => self.offset += end,
                None => break,
            }
        }
    }

    fn take(&mut self, token: &str) -> bool {
        self.skip_space();
        if self.rest().starts_with(token) {
            self.offset += token.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &str, message: &'static str) -> Result<(), Error> {
        if self.take(token) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn error(&self, message: &'static str) -> Error {
        // `/*` cannot start valid Mammoth, so explain the likely intent.
        let block_comment = self
            .rest()
            .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
            .starts_with("/*")
            || self
                .source
                .get(self.offset.saturating_sub(1)..)
                .is_some_and(|rest| rest.starts_with("/*"));
        Error {
            offset: self.offset,
            message: if block_comment {
                "This block comment is never closed. End it with */."
            } else {
                message
            },
        }
    }

    fn name(&mut self) -> Result<Name<'a>, Error> {
        self.skip_space();
        let name =
            identifier(self.rest(), self.offset).ok_or_else(|| self.error("Expected a name."))?;
        self.offset += name.text.len();
        Ok(name)
    }

    fn take_word(&mut self, word: &str) -> bool {
        self.skip_space();
        if identifier(self.rest(), self.offset).is_some_and(|name| name.text == word) {
            self.offset += word.len();
            return true;
        }
        false
    }

    fn module(&mut self) -> Result<String, Error> {
        let parts = self.qualified_name()?;
        if parts.len() < 2 {
            return Err(Error {
                offset: parts[0].offset,
                message: "Use a package and class name, such as example.Logic.",
            });
        }
        let module = parts
            .iter()
            .map(|part| part.text)
            .collect::<Vec<_>>()
            .join("/");
        if module.len() > 255 {
            return Err(Error {
                offset: parts[0].offset,
                message: "This module name is too long.",
            });
        }
        Ok(module)
    }

    fn import(&mut self) -> Result<Import<'a>, Error> {
        let first = self.name()?;
        let mut parts = vec![first];
        loop {
            if !self.take(".") {
                break;
            }
            self.skip_space();
            if self.rest().starts_with('*') {
                let star = Name {
                    text: "*",
                    offset: self.offset,
                };
                self.offset += 1;
                let module = parts
                    .iter()
                    .map(|part| part.text)
                    .collect::<Vec<_>>()
                    .join("/");
                return Ok(Import {
                    module,
                    name: star,
                    local: star,
                    wildcard: true,
                });
            }
            parts.push(self.name()?);
        }
        if parts.len() < 2 {
            return Err(Error {
                offset: parts[0].offset,
                message: "Import a Java class such as java.util.Scanner, or a function such as example.Logic.double.",
            });
        }
        let name = parts.pop().unwrap();
        let module = parts
            .iter()
            .map(|part| part.text)
            .collect::<Vec<_>>()
            .join("/");
        let local = if self.take_word("as") {
            self.binding()?
        } else {
            name
        };
        Ok(Import {
            module,
            name,
            local,
            wildcard: false,
        })
    }

    fn qualified_name(&mut self) -> Result<Vec<Name<'a>>, Error> {
        let first = self.name()?;
        let mut parts = vec![first];
        while self.take(".") {
            parts.push(self.name()?);
        }
        Ok(parts)
    }

    fn interface(&mut self) -> Result<Interface<'a>, Error> {
        let name = self.binding()?;
        let mut type_parameters = Vec::new();
        let mut variances = Vec::new();
        let mut bounds = Vec::new();
        if self.take("<") {
            loop {
                variances.push(self.variance());
                type_parameters.push(self.name()?);
                bounds.push(self.bound_list()?);
                if self.take(">") {
                    break;
                }
                self.expect(",", "Expected ',' or '>' after a type parameter.")?;
            }
        }
        let mut extends = Vec::new();
        if self.take_word("extends") {
            loop {
                extends.push(self.annotation()?);
                if !self.take(",") {
                    break;
                }
            }
        }
        // An interface without members can leave out its braces.
        let has_body = self.take("{");
        let outer_type = std::mem::replace(&mut self.current_type, name.text);
        let mut methods = Vec::new();
        while has_body && !self.take("}") {
            let annotations = self.annotations()?;
            let is_override = self.take_word("override");
            let mutable_property = self.take_word("var");
            if mutable_property || self.take_word("let") || self.take_word("val") {
                // A property is a method its implementers answer with a field.
                let property = self.binding()?;
                self.expect(":", "Expected ':' followed by the property type.")?;
                let annotation = self.annotation()?;
                self.properties_found.push(property.text);
                if mutable_property {
                    let setter = setter_name(property);
                    self.properties_found.push(setter.text);
                    let value = Name {
                        text: "value",
                        offset: property.offset,
                    };
                    let mut declared =
                        accessor(setter, vec![(value, annotation)], None, Vec::new());
                    declared.is_abstract = true;
                    methods.push(declared);
                }
                let default_body = if !mutable_property && self.take_word("get") {
                    self.expect("(", "Expected '()' after get.")?;
                    self.expect(")", "Expected '()' after get.")?;
                    Some(if self.take_assignment() {
                        vec![Statement::Return(property, Some(self.expression()?))]
                    } else {
                        self.block(1)?
                    })
                } else {
                    None
                };
                let has_default = default_body.is_some();
                methods.push(Function {
                    annotations,
                    type_parameters: Vec::new(),
                    bounds: Vec::new(),
                    exported: false,
                    foreign_name: None,
                    name: property,
                    parameters: Vec::new(),
                    result: Some(annotation),
                    body: default_body.unwrap_or_default(),
                    private: false,
                    protected: false,
                    is_abstract: !has_default,
                    is_override,
                    extension_of: None,
                    in_prelude: false,
                    vararg: false,
                    overload_of: None,
                });
                continue;
            }
            if !self.take_word("fun") {
                return Err(self.error("Expected a method signature or '}' in the interface."));
            }
            let mut method = self.function(false)?;
            method.is_override = is_override;
            method.annotations = annotations;
            self.skip_space();
            if self.rest().starts_with('{') {
                // A default method: implementing classes inherit this body.
                method.body = self.block(0)?;
            } else if self.take_assignment() {
                method.body = vec![Statement::Return(method.name, Some(self.expression()?))];
            } else {
                method.is_abstract = true;
            }
            methods.push(method);
            methods.append(&mut self.finished_helpers);
        }
        self.current_type = outer_type;
        Ok(Interface {
            annotations: Vec::new(),
            type_parameters,
            variances,
            bounds,
            name,
            extends,
            methods,
            is_sealed: false,
        })
    }

    fn class(
        &mut self,
        is_abstract: bool,
        is_data: bool,
        is_enum: bool,
    ) -> Result<Class<'a>, Error> {
        let name = self.binding()?;
        self.class_named(name, is_abstract, is_data, is_enum)
    }

    /// The rest of a class declaration, after its name.
    fn class_named(
        &mut self,
        name: Name<'a>,
        is_abstract: bool,
        is_data: bool,
        is_enum: bool,
    ) -> Result<Class<'a>, Error> {
        let mut type_parameters = Vec::new();
        let mut variances = Vec::new();
        let mut bounds = Vec::new();
        if self.take("<") {
            if is_enum {
                return Err(self.error("Enums cannot declare type parameters."));
            }
            loop {
                variances.push(self.variance());
                type_parameters.push(self.name()?);
                bounds.push(self.bound_list()?);
                if self.take(">") {
                    break;
                }
                self.expect(",", "Expected ',' or '>' after a type parameter.")?;
            }
        }
        let mut parameters = Vec::new();
        let mut fields = Vec::new();
        let mut initializers = Vec::new();
        let mut header_methods = Vec::new();
        let private_primary = self.rest_trimmed().starts_with("private constructor");
        if private_primary {
            self.take_word("private");
            self.take_word("constructor");
        }
        let has_header = self.take("(");
        if has_header {
            let mut index = 0;
            let declared = self.list(")", |parser| {
                let annotations = parser.annotations()?;
                let private = parser.take_word("private");
                let protected = !private && parser.take_word("protected");
                if parser.take_word("private") || parser.take_word("protected") {
                    return Err(parser.error("Choose only one field visibility: private or protected."));
                }
                let overriding = parser.take_word("override");
                let open = !overriding && parser.take_word("open");
                let mutable = if parser.take_word("var") {
                    Some(true)
                } else if parser.take_word("let") || parser.take_word("val") {
                    Some(false)
                } else {
                    None
                };
                if private && mutable.is_none() {
                    return Err(parser.error("Expected let or var after private."));
                }
                if (protected || !annotations.is_empty()) && mutable.is_none() {
                    return Err(parser.error("Field annotations and protected visibility require a let or var constructor property."));
                }
                let parameter = parser.binding()?;
                parser.expect(":", "Expected ':' followed by the parameter type.")?;
                let annotation = parser.annotation()?;
                parser.default_value(index)?;
                index += 1;
                if (overriding || open) && mutable.is_none() {
                    return Err(parser.error("Expected let or var before an overridable constructor property."));
                }
                Ok((parameter, annotation, mutable, private, protected, annotations, overriding, open))
            })?;
            for (
                parameter,
                annotation,
                mutable,
                private,
                protected,
                annotations,
                overriding,
                open,
            ) in declared
            {
                parameters.push((parameter, annotation));
                if let Some(mutable) = mutable
                    && (overriding || open)
                {
                    let (field, methods) = self.finish_property(
                        parameter,
                        Some(annotation),
                        Some(Expr::Variable(parameter)),
                        mutable,
                        Accessors::default(),
                        PropertyAccess {
                            private,
                            protected,
                            is_override: overriding,
                        },
                    )?;
                    initializers.push(Initializer::Field(fields.len()));
                    fields.extend(field);
                    header_methods.extend(methods);
                } else if let Some(mutable) = mutable {
                    initializers.push(Initializer::Field(fields.len()));
                    fields.push(Field {
                        annotations,
                        protected,
                        name: parameter,
                        annotation: Some(annotation),
                        value: Some(Expr::Variable(parameter)),
                        mutable,
                        private,
                        is_lateinit: false,
                    });
                }
            }
        }
        let constructor_helpers = self.default_helpers("new", &parameters, &[], &[]);
        for helper in &constructor_helpers {
            let generic = |annotation: Name<'_>| {
                type_parameters.iter().any(|parameter| {
                    annotation
                        .text
                        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                        .any(|word| word == parameter.text)
                })
            };
            if helper.result.is_some_and(generic)
                || helper.parameters.iter().any(|(_, kind)| generic(*kind))
            {
                return Err(Error {
                    offset: helper.name.offset,
                    message: "A constructor default value cannot use the class's type parameters yet.",
                });
            }
        }
        let extends = if self.take_word("extends") {
            let parent = self.annotation()?;
            let args = if self.take("(") {
                self.list(")", |parser| parser.argument(0))?
            } else {
                Vec::new()
            };
            Some((parent, args))
        } else {
            None
        };
        let mut implements = Vec::new();
        let mut delegates = Vec::new();
        if self.take_word("implements") {
            loop {
                let interface = self.annotation()?;
                implements.push(interface);
                if self.take_word("by") {
                    // The value is kept in a hidden field; methods forward to it.
                    let value = self.header_expression()?;
                    let field = Name {
                        text: Box::leak(format!("__by{}", delegates.len()).into_boxed_str()),
                        offset: interface.offset,
                    };
                    initializers.push(Initializer::Field(fields.len()));
                    fields.push(Field {
                        annotations: Vec::new(),
                        protected: false,
                        name: field,
                        annotation: Some(interface),
                        value: Some(value),
                        mutable: false,
                        private: true,
                        is_lateinit: false,
                    });
                    delegates.push((interface, field));
                }
                if !self.take(",") {
                    break;
                }
            }
        }
        if is_enum && extends.is_some() {
            return Err(self.error("An enum cannot extend a class."));
        }
        if is_data && fields.is_empty() {
            return Err(Error {
                offset: name.offset,
                message: "A data class needs fields, such as data class Point(let x: Int, let y: Int).",
            });
        }
        self.skip_space();
        if !is_enum && !self.rest().starts_with('{') {
            // A class without a body, such as `data class Point(let x: Int, let y: Int)`.
            return Ok(Class {
                annotations: Vec::new(),
                type_parameters,
                bounds,
                name,
                parameters,
                implements,
                extends,
                is_abstract,
                is_sealed: false,
                is_object: false,
                methods: {
                    header_methods.extend(self.property_getters(&fields, &header_methods));
                    header_methods
                },
                fields,
                static_fields: Vec::new(),
                static_methods: constructor_helpers,
                static_initializers: Vec::new(),
                initializers,
                is_data,
                variances,
                enum_values: None,
                secondary: Vec::new(),
                no_primary: false,
                private_primary,
                delegates,
                inner_of: None,
            });
        }
        self.expect("{", "Expected '{' after the class name.")?;
        let mut enum_values = None;
        if is_enum {
            let mut values = Vec::new();
            loop {
                self.skip_space();
                let word = identifier(self.rest(), self.offset);
                if word.is_none_or(|word| {
                    matches!(
                        word.text,
                        "fun"
                            | "let"
                            | "var"
                            | "private"
                            | "init"
                            | "static"
                            | "override"
                            | "abstract"
                            | "open"
                    )
                }) {
                    break;
                }
                let value = self.binding()?;
                let args = if self.take("(") {
                    self.list(")", Self::expression)?
                } else {
                    Vec::new()
                };
                values.push((value, args));
                if !self.take(",") {
                    self.take(";");
                    break;
                }
            }
            if values.is_empty() {
                return Err(
                    self.error("List the enum's values, such as enum Color { Red, Green }.")
                );
            }
            enum_values = Some(values);
        }
        let outer_type = std::mem::replace(&mut self.current_type, name.text);
        let mut methods = header_methods;
        let mut static_fields = Vec::new();
        let mut static_methods = constructor_helpers;
        let mut static_initializers = Vec::new();
        let mut secondary: Vec<SecondaryConstructor<'a>> = Vec::new();
        let mut in_companion = false;
        loop {
            if self.take("}") {
                if in_companion {
                    in_companion = false;
                    continue;
                }
                break;
            }
            if self.rest().is_empty() {
                return Err(self.error("Expected '}' to close the class."));
            }
            let annotations = self.annotations()?;
            let mut member = self.name()?;
            let mut private = false;
            let mut protected = false;
            let mut abstract_member = false;
            let mut is_override = false;
            let mut exported = false;
            let mut is_static = in_companion;
            let mut lateinit = false;
            let mut open_member = false;
            let mut inner_member = false;
            if member.text == "companion" {
                if !self.take_word("object") {
                    return Err(self.error("Expected object after companion."));
                }
                if identifier(self.rest_trimmed(), 0).is_some() {
                    self.name()?;
                }
                self.expect("{", "Expected '{' to open the companion object.")?;
                in_companion = true;
                continue;
            }
            loop {
                match member.text {
                    "static" if !is_static => is_static = true,
                    "private" if !private && !protected => private = true,
                    "protected" if !protected && !private => protected = true,
                    "abstract" if !abstract_member => abstract_member = true,
                    "override" if !is_override => is_override = true,
                    "export" if !exported => exported = true,
                    "lateinit" if !lateinit => lateinit = true,
                    "open" => open_member = true,
                    "inner" if !inner_member => inner_member = true,
                    "operator" | "infix" | "inline" | "tailrec" => {}
                    _ => break,
                }
                member = self.name()?;
            }
            if matches!(
                member.text,
                "class" | "data" | "enum" | "sealed" | "object" | "interface"
            ) {
                if inner_member && (member.text != "class" || !type_parameters.is_empty()) {
                    return Err(Error {
                        offset: member.offset,
                        message: "Only a class inside a class without type parameters can be inner.",
                    });
                }
                self.nested_type(member, abstract_member, name, inner_member)?;
                continue;
            }
            if is_static {
                if protected || abstract_member || is_override || exported {
                    return Err(Error {
                        offset: member.offset,
                        message: "A static member can be public or private, but not protected, abstract, override, or export.",
                    });
                }
                match member.text {
                    "fun" => {
                        let mut function = self.function(true)?;
                        function.annotations = annotations;
                        function.private = private;
                        static_methods.push(function);
                        static_methods.append(&mut self.finished_helpers);
                    }
                    "var" | "let" | "val" => {
                        let mutable = member.text == "var";
                        let name = self.binding()?;
                        let annotation = if self.take(":") {
                            Some(self.annotation()?)
                        } else {
                            None
                        };
                        let value = if self.take_assignment() {
                            Some(self.expression()?)
                        } else if annotation.is_some_and(|name| name.text.ends_with('?')) {
                            None
                        } else {
                            return Err(
                                self.error("Expected '=' before the field's starting value.")
                            );
                        };
                        static_initializers.push(Initializer::Field(static_fields.len()));
                        static_fields.push(Field {
                            annotations,
                            protected: false,
                            name,
                            annotation,
                            value,
                            mutable,
                            private,
                            is_lateinit: false,
                        });
                    }
                    "init" if !private && annotations.is_empty() => {
                        static_initializers.push(Initializer::Block(self.block(0)?));
                    }
                    _ => {
                        return Err(Error {
                            offset: member.offset,
                            message: "Expected fun, var, let, or init after static.",
                        });
                    }
                }
                continue;
            }
            match member.text {
                "fun" => {
                    if protected && exported {
                        return Err(Error {
                            offset: member.offset,
                            message: "An exported class method must be public and have a body.",
                        });
                    }
                    if abstract_member && !is_abstract {
                        return Err(Error {
                            offset: member.offset,
                            message: "Only an abstract class can have abstract methods.",
                        });
                    }
                    let mut function = self.function(!abstract_member)?;
                    function.annotations = annotations;
                    function.private = private;
                    function.protected = protected;
                    function.is_abstract = abstract_member;
                    function.is_override = is_override;
                    if is_override && !self.finished_helpers.is_empty() {
                        return Err(Error {
                            offset: function.name.offset,
                            message: "An override uses the default values of the method it overrides. Remove them here.",
                        });
                    }
                    function.exported = exported;
                    if exported && (private || abstract_member) {
                        return Err(Error {
                            offset: function.name.offset,
                            message: "An exported class method must be public and have a body.",
                        });
                    }
                    if private && abstract_member {
                        return Err(Error {
                            offset: function.name.offset,
                            message: "An abstract method cannot be private.",
                        });
                    }
                    methods.push(function);
                    methods.append(&mut self.finished_helpers);
                }
                "var" | "let" | "val" if !exported => {
                    let mutable = member.text == "var";
                    let name = self.binding()?;
                    let annotation = if self.take(":") {
                        Some(self.annotation()?)
                    } else {
                        None
                    };
                    if abstract_member {
                        if !is_abstract {
                            return Err(Error {
                                offset: member.offset,
                                message: "Only an abstract class can have abstract properties.",
                            });
                        }
                        if private {
                            return Err(Error {
                                offset: name.offset,
                                message: "An abstract property cannot be private.",
                            });
                        }
                        let Some(result) = annotation else {
                            return Err(self.error("Give an abstract property a type."));
                        };
                        let mut getter = accessor(name, Vec::new(), Some(result), Vec::new());
                        getter.is_abstract = true;
                        getter.is_override = is_override;
                        getter.protected = protected;
                        methods.push(getter);
                        if mutable {
                            let value = Name {
                                text: "value",
                                offset: name.offset,
                            };
                            let mut setter = accessor(
                                setter_name(name),
                                vec![(value, result)],
                                None,
                                Vec::new(),
                            );
                            setter.is_abstract = true;
                            setter.protected = protected;
                            methods.push(setter);
                        }
                        continue;
                    }
                    if self.take_word("by") {
                        if !self.take_word("lazy") || !self.take("{") {
                            return Err(self.error("Expected lazy and a block after by."));
                        }
                        let Some(result) = annotation else {
                            return Err(self.error(
                                "Give a lazy field a type, such as let total: Int by lazy { ... }.",
                            ));
                        };
                        let (body, value) = self.value_body(1)?;
                        let Some(value) = value else {
                            return Err(self.error("A lazy block must end with a value."));
                        };
                        let (field, getter) = lazy_field(name, result, body, *value, private);
                        initializers.push(Initializer::Field(fields.len()));
                        fields.push(field);
                        methods.push(getter);
                        continue;
                    }
                    let value = if self.take_assignment() {
                        Some(self.expression()?)
                    } else {
                        None
                    };
                    let backing = backing_name(name);
                    let accessors = self.property_accessors(name, mutable, backing)?;
                    if open_member || is_override || accessors.declared() {
                        if lateinit {
                            return Err(Error {
                                offset: name.offset,
                                message: "A lateinit field cannot have accessors or be overridden.",
                            });
                        }
                        let (field, new_methods) = self.finish_property(
                            name,
                            annotation,
                            value,
                            mutable,
                            accessors,
                            PropertyAccess {
                                private,
                                protected,
                                is_override,
                            },
                        )?;
                        if let Some(field) = field {
                            initializers.push(Initializer::Field(fields.len()));
                            fields.push(field);
                        }
                        methods.extend(new_methods);
                        continue;
                    }
                    if value.is_none()
                        && !lateinit
                        && !annotation.is_some_and(|name| name.text.ends_with('?'))
                    {
                        return Err(self.error("Expected '=' before the field's starting value."));
                    }
                    if lateinit
                        && (!mutable
                            || value.is_some()
                            || annotation.is_none_or(|name| {
                                name.text.ends_with('?')
                                    || matches!(
                                        name.text,
                                        "Int" | "Long" | "Double" | "Bool" | "Char"
                                    )
                            }))
                    {
                        return Err(Error {
                            offset: name.offset,
                            message: "lateinit needs a var with a non-null object type and no starting value.",
                        });
                    }
                    initializers.push(Initializer::Field(fields.len()));
                    fields.push(Field {
                        annotations,
                        protected,
                        name,
                        annotation,
                        value,
                        mutable,
                        private,
                        is_lateinit: lateinit,
                    });
                }
                "constructor"
                    if !private
                        && !protected
                        && !abstract_member
                        && !is_override
                        && !exported
                        && !is_enum
                        && annotations.is_empty() =>
                {
                    secondary.push(self.secondary_constructor(member)?);
                }
                "init"
                    if !private
                        && !protected
                        && !abstract_member
                        && !is_override
                        && !exported
                        && annotations.is_empty() =>
                {
                    initializers.push(Initializer::Block(self.block(0)?));
                }
                _ if private => {
                    return Err(Error {
                        offset: member.offset,
                        message: "Expected var, let, or fun after private.",
                    });
                }
                _ => {
                    return Err(Error {
                        offset: member.offset,
                        message: "Expected a method (fun), a field (var or let), or an init block.",
                    });
                }
            }
        }
        self.current_type = outer_type;
        let getters = self.property_getters(&fields, &methods);
        methods.extend(getters);
        let mut no_primary = !has_header && !secondary.is_empty();
        if no_primary
            && let Some(position) = secondary
                .iter()
                .position(|item| item.parameters.is_empty() && !item.delegates)
        {
            // Its body runs after the field initializers, like an init block.
            let item = secondary.remove(position);
            initializers.push(Initializer::Block(item.body));
            no_primary = false;
        }
        Ok(Class {
            annotations: Vec::new(),
            type_parameters,
            bounds,
            name,
            parameters,
            implements,
            extends,
            is_abstract,
            is_sealed: false,
            is_object: false,
            fields,
            methods,
            static_fields,
            static_methods,
            static_initializers,
            initializers,
            is_data,
            variances,
            enum_values,
            secondary,
            no_primary,
            private_primary,
            delegates,
            inner_of: None,
        })
    }

    /// Any `get()` and `set(value)` written after a property's starting value.
    fn property_accessors(
        &mut self,
        property: Name<'a>,
        mutable: bool,
        backing: &'a str,
    ) -> Result<Accessors<'a>, Error> {
        let mut accessors = Accessors::default();
        let start = self.offset;
        self.local_aliases.push(("field", backing));
        loop {
            self.skip_space();
            let before = self.offset;
            let private = self.take_word("private");
            let protected = !private && self.take_word("protected");
            if !private && !protected && self.take_word("get") {
                self.expect("(", "Expected '()' after get.")?;
                self.expect(")", "Expected '()' after get.")?;
                if accessors.getter.is_some() {
                    return Err(self.error("This property already has a getter."));
                }
                accessors.getter = Some(if self.take_assignment() {
                    vec![Statement::Return(property, Some(self.expression()?))]
                } else {
                    self.block(1)?
                });
                continue;
            }
            if self.take_word("set") {
                if !mutable {
                    return Err(self.error("A let property cannot have a setter. Use var."));
                }
                if accessors.setter_declared {
                    return Err(self.error("This property already has a setter."));
                }
                accessors.setter_declared = true;
                accessors.setter_private = private;
                accessors.setter_protected = protected;
                if self.take("(") {
                    let parameter = self.binding()?;
                    if self.take(":") {
                        self.annotation()?;
                    }
                    self.expect(")", "Expected ')' after the setter's parameter.")?;
                    let body = if self.take_assignment() {
                        vec![Statement::Assign(
                            Name {
                                text: backing,
                                offset: parameter.offset,
                            },
                            self.expression()?,
                        )]
                    } else {
                        self.block(1)?
                    };
                    accessors.setter = Some((parameter, body));
                }
                continue;
            }
            self.offset = before;
            break;
        }
        self.local_aliases.pop();
        let text = &self.source[start..self.offset];
        accessors.uses_field = text
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .any(|word| word == "field");
        Ok(accessors)
    }

    /// Builds a property with accessors, which callers reach through methods
    /// so that subclasses can replace them.
    fn finish_property(
        &self,
        name: Name<'a>,
        annotation: Option<Name<'a>>,
        value: Option<Expr<'a>>,
        mutable: bool,
        accessors: Accessors<'a>,
        access: PropertyAccess,
    ) -> Result<(Option<Field<'a>>, Vec<Function<'a>>), Error> {
        let result =
            annotation.or_else(|| value.as_ref().and_then(|value| literal_type(value, name)));
        let Some(result) = result else {
            return Err(Error {
                offset: name.offset,
                message: "Give this property a type, such as var count: Int = 0.",
            });
        };
        let backing = Name {
            text: backing_name(name),
            offset: name.offset,
        };
        let needs_field = value.is_some()
            || accessors.uses_field
            || accessors.getter.is_none()
            || (mutable && accessors.setter.is_none());
        if needs_field && value.is_none() && !result.text.ends_with('?') {
            return Err(Error {
                offset: name.offset,
                message: "This property stores a value, so give it a starting value.",
            });
        }
        let this = Expr::Variable(Name {
            text: self.object_scopes.last().map_or("this", |scope| scope.0),
            offset: name.offset,
        });
        let getter_body = accessors.getter.unwrap_or_else(|| {
            let field = Expr::Field(Box::new(this.clone()), backing);
            vec![Statement::Return(name, Some(field))]
        });
        let mut getter = accessor(name, Vec::new(), Some(result), getter_body);
        getter.private = access.private;
        getter.protected = access.protected;
        getter.is_override = access.is_override;
        let mut methods = vec![getter];
        if mutable {
            let (parameter, body) = accessors.setter.unwrap_or_else(|| {
                let parameter = Name {
                    text: "value",
                    offset: name.offset,
                };
                let store =
                    Statement::SetField(this.clone(), backing, None, Expr::Variable(parameter));
                (parameter, vec![store])
            });
            let mut setter = accessor(setter_name(name), vec![(parameter, result)], None, body);
            setter.private = access.private || accessors.setter_private;
            setter.protected = access.protected || accessors.setter_protected;
            methods.push(setter);
        }
        let field = needs_field.then_some(Field {
            annotations: Vec::new(),
            protected: false,
            name: backing,
            annotation: Some(result),
            value,
            mutable,
            private: true,
            is_lateinit: false,
        });
        Ok((field, methods))
    }

    /// The hidden name of the innermost object with a member `name`.
    fn object_member(&self, name: &str) -> Option<&'a str> {
        self.object_scopes
            .iter()
            .rev()
            .find(|(_, members)| members.contains(&name))
            .map(|(owner, _)| *owner)
    }

    /// A local or parameter named `name` hides the member until the method ends.
    fn shadow_member(&mut self, name: &str) {
        if let Some(scope) = self.object_scopes.last_mut() {
            scope.1.retain(|member| *member != name);
        }
    }

    /// `object implements Runnable { ... }`. The object becomes a hidden class
    /// whose methods forward to lambdas made where the object is written, so
    /// they can use the local variables around it.
    fn object_expression(&mut self, keyword: Name<'a>) -> Result<Expr<'a>, Error> {
        let hidden: &'a str = Box::leak(format!("__Anon{}", keyword.offset).into_boxed_str());
        let self_name: &'a str = Box::leak(format!("__self{}", keyword.offset).into_boxed_str());
        let name = Name {
            text: hidden,
            offset: keyword.offset,
        };
        // A first reading learns the members, which may be used before they
        // are declared.
        let start = self.offset;
        let marks = (
            self.hoisted_functions.len(),
            self.nested_classes.len(),
            self.finished_helpers.len(),
            self.extra_statements.len(),
            self.declared_functions.len(),
        );
        let first = self.class_named(name, false, false, false)?;
        let mut members: Vec<&'a str> = first.fields.iter().map(|field| field.name.text).collect();
        members.extend(
            first
                .methods
                .iter()
                .map(|method| method.name.text)
                .filter(|name| !name.contains('$')),
        );
        self.hoisted_functions.truncate(marks.0);
        self.nested_classes.truncate(marks.1);
        self.finished_helpers.truncate(marks.2);
        self.extra_statements.truncate(marks.3);
        self.declared_functions.truncate(marks.4);
        self.offset = start;
        self.object_scopes.push((self_name, members));
        self.local_aliases.push(("this", self_name));
        let parsed = self.class_named(name, false, false, false);
        self.local_aliases.pop();
        self.object_scopes.pop();
        let mut class = parsed?;
        let self_type = name;
        let self_variable = Name {
            text: self_name,
            offset: keyword.offset,
        };
        let this = Expr::Variable(Name {
            text: "this",
            offset: keyword.offset,
        });
        let mut arguments = Vec::new();
        let mut holders = Vec::new();
        for (position, method) in class.methods.iter_mut().enumerate() {
            if method.is_abstract || method.name.text.contains('$') {
                continue;
            }
            if !method.type_parameters.is_empty() {
                return Err(Error {
                    offset: method.name.offset,
                    message: "A method of an object expression cannot have type parameters yet.",
                });
            }
            let holder = Name {
                text: Box::leak(format!("__m{position}").into_boxed_str()),
                offset: method.name.offset,
            };
            let mut inputs = vec![hidden];
            inputs.extend(method.parameters.iter().map(|(_, kind)| kind.text));
            let result = method.result.map_or("Unit", |result| result.text);
            let annotation = Name {
                text: Box::leak(format!("({}) -> {result}", inputs.join(", ")).into_boxed_str()),
                offset: method.name.offset,
            };
            let mut parameters = vec![(self_variable, Some(self_type))];
            parameters.extend(
                method
                    .parameters
                    .iter()
                    .map(|&(name, kind)| (name, Some(kind))),
            );
            let body = std::mem::take(&mut method.body);
            arguments.push(Expr::Lambda(Box::new(Lambda {
                offset: method.name.offset,
                parameters,
                explicit: true,
                body,
                value: None,
                receiver: false,
            })));
            let mut forwarded = vec![this.clone()];
            forwarded.extend(
                method
                    .parameters
                    .iter()
                    .map(|&(name, _)| Expr::Variable(name)),
            );
            let call = Expr::Call(holder, forwarded.clone());
            method.body = if method.result.is_some() {
                vec![Statement::Return(method.name, Some(call))]
            } else {
                vec![Statement::Call(holder, forwarded)]
            };
            method.private = false;
            holders.push((holder, annotation));
        }
        for field in &mut class.fields {
            field.private = false;
        }
        // Field starting values refer to the object through this name too.
        class.methods.push(accessor(
            self_variable,
            Vec::new(),
            Some(self_type),
            vec![Statement::Return(self_variable, Some(this))],
        ));
        for (index, &(holder, annotation)) in holders.iter().enumerate() {
            class.parameters.push((holder, annotation));
            class
                .initializers
                .insert(index, Initializer::Field(class.fields.len()));
            class.fields.push(Field {
                annotations: Vec::new(),
                protected: false,
                name: holder,
                annotation: Some(annotation),
                value: Some(Expr::Variable(holder)),
                mutable: false,
                private: true,
                is_lateinit: false,
            });
        }
        self.nested_classes.push(class);
        Ok(Expr::Call(name, arguments))
    }

    /// After the word `constructor`.
    fn secondary_constructor(
        &mut self,
        keyword: Name<'a>,
    ) -> Result<SecondaryConstructor<'a>, Error> {
        self.expect("(", "Expected '(' after constructor.")?;
        let parameters = self.list(")", |parser| {
            let parameter = parser.binding()?;
            parser.expect(":", "Expected ':' followed by the parameter type.")?;
            Ok((parameter, parser.annotation()?))
        })?;
        let mut arguments = Vec::new();
        let delegates = self.take(":");
        if delegates {
            if !self.take_word("this") {
                return Err(self.error("A constructor can only call this(...) of its own class."));
            }
            self.expect("(", "Expected '(' after this.")?;
            arguments = self.list(")", |parser| parser.argument(0))?;
        }
        let body = if self.rest_trimmed().starts_with('{') {
            self.block(0)?
        } else {
            Vec::new()
        };
        Ok(SecondaryConstructor {
            keyword,
            parameters,
            delegates,
            arguments,
            body,
        })
    }

    fn function(&mut self, has_body: bool) -> Result<Function<'a>, Error> {
        self.skip_space();
        let start = self.offset;
        if self.top_level_function || !self.current_type.is_empty() {
            self.function_start = start;
        }
        let receiver = match self.annotation() {
            Ok(annotation) if self.rest().starts_with('.') => {
                self.offset += 1;
                Some(annotation)
            }
            _ => {
                self.offset = start;
                None
            }
        };
        if receiver.is_some() && !self.top_level_function {
            return Err(self.error("Extension functions must be declared at the top level."));
        }
        let name = self.name()?;
        let (name, extension_of) = match receiver {
            Some(receiver) => {
                let label: String = receiver
                    .text
                    .chars()
                    .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
                    .collect();
                let hidden = Box::leak(format!("{label}$ext${}", name.text).into_boxed_str());
                (
                    Name {
                        text: hidden,
                        offset: name.offset,
                    },
                    Some(name.text),
                )
            }
            None => (name, None),
        };
        let mut type_parameters = Vec::new();
        let mut bounds = Vec::new();
        if self.take("<") {
            loop {
                type_parameters.push(self.name()?);
                bounds.push(self.bound_list()?);
                if self.take(">") {
                    break;
                }
                self.expect(",", "Expected ',' or '>' after a type parameter.")?;
            }
        }
        self.expect("(", "Expected '(' after the function name.")?;
        let mut parameters = self.parameters()?;
        let members = self.object_scopes.last().map(|scope| scope.1.clone());
        for (parameter, _) in &parameters {
            self.shadow_member(parameter.text);
        }
        let vararg = std::mem::take(&mut self.saw_vararg);
        self.declared_functions.push((self.current_type, name.text));
        let mut overload_of = None;
        let mut name = name;
        if self.overloaded.contains(&name.text) {
            overload_of = Some(name.text);
            let label: String = parameters
                .iter()
                .map(|(_, annotation)| annotation.text)
                .collect::<Vec<_>>()
                .join("_")
                .chars()
                .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
                .collect();
            name.text = Box::leak(format!("{}$ov${label}", name.text).into_boxed_str());
        }
        if let Some(receiver) = receiver {
            parameters.insert(
                0,
                (
                    Name {
                        text: "this",
                        offset: name.offset,
                    },
                    receiver,
                ),
            );
            // Default values count the receiver as parameter zero.
            for default in &mut self.pending_defaults {
                default.0 += 1;
            }
        }
        let mut wrote_unit = false;
        let result = if self.take(":") {
            let written = self.annotation()?;
            wrote_unit = written.text == "Unit";
            // A function that never returns is declared to give an exception,
            // and each call throws it, so code after a call is unreachable.
            (!wrote_unit).then_some(if written.text == "Nothing" {
                Name {
                    text: "RuntimeException",
                    offset: written.offset,
                }
            } else {
                written
            })
        } else {
            None
        };
        if self.take_word("where") {
            loop {
                let parameter = self.name()?;
                self.expect(":", "Expected ':' after the type parameter.")?;
                let bound = self.annotation()?;
                let Some(index) = type_parameters
                    .iter()
                    .position(|name| name.text == parameter.text)
                else {
                    return Err(Error {
                        offset: parameter.offset,
                        message: "This type parameter is not declared.",
                    });
                };
                bounds[index].push(bound);
                if !self.take(",") {
                    break;
                }
            }
        }
        let helpers = self.default_helpers(name.text, &parameters, &type_parameters, &bounds);
        self.finished_helpers.extend(helpers);
        if let (Some(members), Some(scope)) = (members, self.object_scopes.last_mut()) {
            scope.1 = members;
        }
        let body = if has_body && self.take_assignment() {
            let start = self.offset;
            match self.expression()? {
                Expr::Jump(statement) => vec![*statement],
                Expr::Call(name, args) if result.is_none() && wrote_unit => {
                    vec![Statement::Call(name, args)]
                }
                call @ Expr::MethodCall(..) if result.is_none() && wrote_unit => {
                    vec![Statement::MethodCall(call)]
                }
                value => {
                    if result.is_none() {
                        self.offset = start;
                        return Err(self.error("Give this function a result type, such as fun twice(x: Int): Int = x * 2."));
                    }
                    vec![Statement::Return(name, Some(value))]
                }
            }
        } else if has_body {
            self.block(0)?
        } else {
            Vec::new()
        };
        Ok(Function {
            annotations: Vec::new(),
            type_parameters,
            bounds,
            exported: false,
            foreign_name: None,
            name,
            parameters,
            result,
            body,
            private: false,
            protected: false,
            is_abstract: false,
            is_override: false,
            extension_of,
            in_prelude: false,
            vararg,
            overload_of,
        })
    }

    fn java_binding(&mut self) -> Result<JavaBinding<'a>, Error> {
        let function = self.function(false)?;
        let target = self.java_target()?;
        Ok(JavaBinding { function, target })
    }

    fn java_class(&mut self, interface: bool) -> Result<JavaClass<'a>, Error> {
        let name = self.binding()?;
        let target = self.java_target()?;
        self.expect("{", "Expected '{' after the Java class name.")?;
        let mut constructor = None;
        let mut methods = Vec::new();
        while !self.take("}") {
            let member = self.name()?;
            match member.text {
                "new" if interface => {
                    return Err(Error {
                        offset: member.offset,
                        message: "A Java interface cannot have a constructor.",
                    });
                }
                "new" if constructor.is_none() => {
                    self.expect("(", "Expected '(' after new.")?;
                    constructor = Some(Function {
                        annotations: Vec::new(),
                        type_parameters: Vec::new(),
                        bounds: Vec::new(),
                        exported: false,
                        foreign_name: None,
                        name: member,
                        parameters: self.parameters()?,
                        result: None,
                        body: Vec::new(),
                        private: false,
                        protected: false,
                        is_abstract: false,
                        is_override: false,
                        extension_of: None,
                        in_prelude: false,
                        vararg: false,
                        overload_of: None,
                    });
                }
                "new" => {
                    return Err(Error {
                        offset: member.offset,
                        message: "Declare only one constructor for this Java class.",
                    });
                }
                "fun" => {
                    let mut method = self.function(false)?;
                    if !interface && self.take("=") {
                        method.foreign_name = Some(self.java_quoted_name()?);
                    }
                    methods.push(method);
                }
                _ => {
                    return Err(Error {
                        offset: member.offset,
                        message: "Expected a constructor (new) or method (fun).",
                    });
                }
            }
        }
        Ok(JavaClass {
            interface,
            name,
            target,
            constructor,
            methods,
        })
    }

    fn java_target(&mut self) -> Result<Name<'a>, Error> {
        self.expect("=", "Expected '=' before the Java name.")?;
        self.java_quoted_name()
    }

    fn java_quoted_name(&mut self) -> Result<Name<'a>, Error> {
        self.expect("\"", "Put the full Java name in double quotes.")?;
        let offset = self.offset;
        let length = self
            .rest()
            .bytes()
            .take_while(|&byte| !matches!(byte, b'"' | b'\n' | b'\r'))
            .count();
        if self.rest().as_bytes().get(length) != Some(&b'"') {
            return Err(self.error("Expected a closing quote after the Java name."));
        }
        let target = Name {
            text: &self.rest()[..length],
            offset,
        };
        self.offset += length + 1;
        Ok(target)
    }

    fn parameters(&mut self) -> Result<Vec<(Name<'a>, Name<'a>)>, Error> {
        let mut index = 0;
        self.saw_vararg = false;
        self.list(")", |parser| {
            let is_vararg = parser.take_word("vararg");
            if parser.saw_vararg {
                return Err(parser.error("A vararg parameter must be the last one."));
            }
            parser.saw_vararg = is_vararg;
            let parameter = parser.binding()?;
            parser.expect(":", "Expected ':' followed by the parameter type.")?;
            let mut annotation = parser.annotation()?;
            if is_vararg {
                annotation.text = Box::leak(format!("List<{}>", annotation.text).into_boxed_str());
            }
            parser.default_value(index)?;
            index += 1;
            Ok((parameter, annotation))
        })
    }

    /// Reads `= value` after parameter number `index`, if it is there.
    fn default_value(&mut self, index: usize) -> Result<(), Error> {
        if self.take_assignment() {
            self.skip_space();
            let start = self.offset;
            let value = self.expression()?;
            self.pending_defaults
                .push((index, value, start, self.offset));
        }
        Ok(())
    }

    /// Turns the default values just read into hidden helper functions named
    /// `base$default$index`, each receiving the earlier parameters it mentions.
    fn default_helpers(
        &mut self,
        base: &str,
        parameters: &[(Name<'a>, Name<'a>)],
        type_parameters: &[Name<'a>],
        bounds: &[Vec<Name<'a>>],
    ) -> Vec<Function<'a>> {
        std::mem::take(&mut self.pending_defaults)
            .into_iter()
            .map(|(index, value, start, end)| {
                let text = &self.source[start..end];
                let mentions = |name: &str| {
                    text.match_indices(name).any(|(at, _)| {
                        let before = text[..at].chars().next_back();
                        let after = text[at + name.len()..].chars().next();
                        let word = |ch: Option<char>| {
                            ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                        };
                        !word(before) && !word(after)
                    })
                };
                let name = Name {
                    text: Box::leak(format!("{base}$default${index}").into_boxed_str()),
                    offset: value.offset(),
                };
                let inputs: Vec<(Name<'a>, Name<'a>)> = parameters[..index]
                    .iter()
                    .filter(|(parameter, _)| mentions(parameter.text))
                    .copied()
                    .collect();
                let result = parameters[index].1;
                // Keep only the type parameters this helper can see, so a
                // call can always infer them.
                let in_annotation = |annotation: Name<'_>, parameter: &str| {
                    annotation
                        .text
                        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                        .any(|word| word == parameter)
                };
                let kept: Vec<usize> = (0..type_parameters.len())
                    .filter(|&position| {
                        let parameter = type_parameters[position].text;
                        in_annotation(result, parameter)
                            || mentions(parameter)
                            || inputs
                                .iter()
                                .any(|(_, kind)| in_annotation(*kind, parameter))
                    })
                    .collect();
                Function {
                    annotations: Vec::new(),
                    type_parameters: kept.iter().map(|&at| type_parameters[at]).collect(),
                    bounds: kept.iter().map(|&at| bounds[at].clone()).collect(),
                    exported: false,
                    foreign_name: None,
                    name,
                    parameters: inputs,
                    result: Some(result),
                    body: vec![Statement::Return(name, Some(value))],
                    private: false,
                    protected: false,
                    is_abstract: false,
                    is_override: false,
                    extension_of: None,
                    in_prelude: false,
                    vararg: false,
                    overload_of: None,
                }
            })
            .collect()
    }

    /// A call argument, which may be written `parameter = value`.
    fn argument(&mut self, depth: usize) -> Result<Expr<'a>, Error> {
        self.skip_space();
        let start = self.offset;
        if self.rest().starts_with('*') {
            self.offset += 1;
            return Ok(Expr::Spread(Box::new(self.logical(0, depth)?)));
        }
        if let Some(label) = identifier(self.rest(), self.offset) {
            self.offset += label.text.len();
            if self.take_assignment() {
                let value = self.logical(0, depth)?;
                return Ok(Expr::Named(label, Box::new(value)));
            }
            self.offset = start;
        }
        let first = self.logical(0, depth)?;
        let first = self.range_value(first, depth)?;
        if !self.newline_ahead() && self.take_word("to") {
            let second = self.logical(0, depth)?;
            let pair = Name {
                text: "Pair",
                offset: first.offset(),
            };
            return Ok(Expr::Call(pair, vec![first, second]));
        }
        Ok(first)
    }

    /// Reads an optional `out` or `in` before an interface type parameter.
    fn variance(&mut self) -> Variance {
        let start = self.offset;
        let variance = if self.take_word("out") {
            Variance::Out
        } else if self.take_word("in") {
            Variance::In
        } else {
            return Variance::Invariant;
        };
        self.skip_space();
        if identifier(self.rest(), self.offset).is_some() {
            variance
        } else {
            // A parameter that is itself named `out` or `in`.
            self.offset = start;
            Variance::Invariant
        }
    }

    /// Reads the bounds after a type parameter, such as `: Animal & Named`.
    fn bound_list(&mut self) -> Result<Vec<Name<'a>>, Error> {
        let mut bounds = Vec::new();
        if self.take(":") {
            loop {
                bounds.push(self.annotation()?);
                if !self.take("&") {
                    break;
                }
            }
        }
        Ok(bounds)
    }

    fn annotation(&mut self) -> Result<Name<'a>, Error> {
        self.skip_space();
        let start = self.offset;
        if self.take("(") {
            let mut parts = Vec::new();
            if !self.take(")") {
                loop {
                    parts.push(self.annotation()?);
                    if self.take(")") {
                        break;
                    }
                    self.expect(",", "Expected ',' or ')' in the function type.")?;
                }
            }
            // `((Int) -> Text)?` groups a function type so it can be nullable.
            if let [inner] = parts.as_slice()
                && inner.text.starts_with('(')
                && !self.rest().trim_start().starts_with("->")
            {
                if self.rest().starts_with('?') {
                    self.offset += 1;
                }
                return Ok(Name {
                    text: &self.source[start..self.offset],
                    offset: start,
                });
            }
            self.expect(
                "->",
                "Expected '->' and a result type, such as (Int) -> Text.",
            )?;
            self.annotation()?;
            return Ok(Name {
                text: &self.source[start..self.offset],
                offset: start,
            });
        }
        let mut name = self.name()?;
        while self.rest().starts_with('.') {
            let Some(inner) = identifier(&self.rest()[1..], self.offset + 1) else {
                break;
            };
            let guessed =
                self.discovering && inner.text.chars().next().is_some_and(char::is_uppercase);
            if !guessed && !self.is_nested(name.text, inner.text) {
                break;
            }
            self.saw_dotted_type |= guessed;
            self.offset += 1 + inner.text.len();
            name = inner;
        }
        let mut end = self.offset;
        if self.rest().starts_with('<') {
            self.offset += 1;
            loop {
                // `*` is a star projection: an unknown type argument.
                if !self.take("*") {
                    self.annotation()?;
                }
                if self.take(">") {
                    break;
                }
                self.expect(",", "Expected ',' or '>' in the type arguments.")?;
            }
            end = self.offset;
        }
        if self.source[end..].starts_with('?') {
            self.offset += 1;
            end += 1;
        }
        Ok(Name {
            text: &self.source[name.offset..end],
            offset: name.offset,
        })
    }

    fn list<T>(
        &mut self,
        end: &str,
        mut read: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        // Inside brackets, `{` can only start a lambda, even in an `if` header.
        let outer = std::mem::replace(&mut self.in_header, false);
        let mut items = Vec::new();
        while !self.take(end) {
            items.push(read(self)?);
            if self.take(end) {
                break;
            }
            self.expect(",", "Expected a comma or the end of the list.")?;
        }
        self.in_header = outer;
        Ok(items)
    }

    fn block(&mut self, depth: usize) -> Result<Vec<Statement<'a>>, Error> {
        self.expect("{", "Expected '{' to open the block.")?;
        let mut body = Vec::new();
        while !self.take("}") {
            if self.rest().is_empty() {
                return Err(self.error("Expected '}' to close the block."));
            }
            let name = self.name()?;
            body.append(&mut self.statement_list(name, depth)?);
        }
        Ok(body)
    }

    fn statement(&mut self, mut name: Name<'a>, depth: usize) -> Result<Statement<'a>, Error> {
        if matches!(name.text, "field" | "this")
            && let Some(&(_, hidden)) = self
                .local_aliases
                .iter()
                .rev()
                .find(|(plain, _)| *plain == name.text)
        {
            name.text = hidden;
        }
        if name.text == "return" {
            // `return@forEach` leaves the lambda, as a plain return in one does.
            if self.rest().starts_with('@') {
                self.offset += 1;
                self.name()?;
                name.text = "return@";
            }
            let line = self.rest().split('\n').next().unwrap_or("");
            let bare = line.trim().is_empty()
                || line.trim_start().starts_with('}')
                || line.trim_start().starts_with("//");
            if bare {
                return Ok(Statement::Return(name, None));
            }
            return Ok(Statement::Return(name, Some(self.expression()?)));
        }
        if name.text == "throw" {
            return Ok(Statement::Throw(name, self.expression()?));
        }
        if name.text == "synchronized" && self.rest_trimmed().starts_with('(') {
            return self.synchronized(name, depth);
        }
        if name.text == "try" {
            if depth >= 63 {
                return Err(self.error("Blocks can be nested up to 64 levels."));
            }
            let body = self.block(depth + 1)?;
            let mut catches = Vec::new();
            while self.take_word("catch") {
                let parenthesized = self.take("(");
                let (variable, kind) = if !parenthesized && self.rest().starts_with('{') {
                    (
                        Name {
                            text: "error",
                            offset: self.offset,
                        },
                        None,
                    )
                } else {
                    let variable = self.binding()?;
                    let kind = if self.take(":") {
                        Some(self.name()?)
                    } else {
                        None
                    };
                    if parenthesized {
                        self.expect(")", "Expected ')' after the caught exception.")?;
                    }
                    (variable, kind)
                };
                catches.push(Catch {
                    name: variable,
                    kind,
                    body: self.block(depth + 1)?,
                });
            }
            let finally = if self.take_word("finally") {
                Some(self.block(depth + 1)?)
            } else {
                None
            };
            if catches.is_empty() && finally.is_none() {
                return Err(self.error("Expected catch or finally after the try block."));
            }
            return Ok(Statement::Try(name, body, catches, finally));
        }
        if depth == 64 && matches!(name.text, "for" | "if" | "while") {
            return Err(self.error("Blocks can be nested up to 64 levels."));
        }
        let name = if name.text == "val" {
            Name {
                text: "let",
                offset: name.offset,
            }
        } else {
            name
        };
        if name.text == "fun" && identifier(self.rest_trimmed(), 0).is_some() {
            return self.local_function(name);
        }
        if name.text == "do" && self.rest_trimmed().starts_with('{') {
            let body = self.block(depth + 1)?;
            if !self.take_word("while") {
                return Err(self.error("Expected while after the do block."));
            }
            let condition = self.expression()?;
            return Ok(Statement::DoWhile(name, body, condition));
        }
        if self.rest().starts_with('@')
            && let Some(following) = identifier(self.rest()[1..].trim_start(), self.offset + 1)
            && matches!(following.text, "for" | "while" | "do")
        {
            self.offset += 1;
            let keyword = self.name()?;
            let inner = self.statement(keyword, depth)?;
            return Ok(Statement::Labeled(name, Box::new(inner)));
        }
        if matches!(name.text, "break" | "continue") && self.rest().starts_with('@') {
            self.offset += 1;
            let label = self.name()?;
            let text = &self.source[name.offset..label.offset + label.text.len()];
            let labeled = Name {
                text,
                offset: name.offset,
            };
            return Ok(if name.text == "break" {
                Statement::Break(labeled)
            } else {
                Statement::Continue(labeled)
            });
        }
        if matches!(name.text, "let" | "var") && self.take("(") {
            let mutable = name.text == "var";
            let names = self.destructured_names()?;
            if !self.take_assignment() {
                return Err(self.error("Expected '=' after the names."));
            }
            let value = self.expression()?;
            return Ok(self.destructure(name, names, value, mutable));
        }
        if matches!(name.text, "let" | "var") {
            let variable = self.binding()?;
            let annotation = if self.take(":") {
                Some(self.annotation()?)
            } else {
                None
            };
            self.expect("=", "Expected '=' after the binding name.")?;
            let value = self.expression()?;
            self.shadow_member(variable.text);
            return Ok(Statement::Binding(
                variable,
                value,
                name.text == "var",
                annotation,
            ));
        }
        if name.text == "when" {
            let (subject, branches) = self.when_parts(depth, false)?;
            return Ok(Statement::When(name, subject, branches));
        }
        if name.text == "break" {
            return Ok(Statement::Break(name));
        }
        if name.text == "continue" {
            return Ok(Statement::Continue(name));
        }
        if name.text == "if" {
            let condition = self.header_expression()?;
            let then = self.branch_body(depth + 1)?;
            let otherwise = if self.take_word("else") {
                self.skip_space();
                let offset = self.offset;
                if self.take_word("if") {
                    let keyword = Name { text: "if", offset };
                    if depth + 1 >= 64 {
                        return Err(self.error("Blocks can be nested up to 64 levels."));
                    }
                    Some(self.statement_list(keyword, depth + 1)?)
                } else {
                    Some(self.branch_body(depth + 1)?)
                }
            } else {
                None
            };
            return Ok(Statement::If(name, condition, then, otherwise));
        }
        if name.text == "while" {
            return Ok(Statement::While(
                name,
                self.header_expression()?,
                self.block(depth + 1)?,
            ));
        }
        if name.text == "for" {
            let wrapped = self.rest_trimmed().starts_with('(')
                && identifier(self.rest()[1..].trim_start(), 0).is_some_and(|first| {
                    self.rest()[1..]
                        .trim_start()
                        .get(first.text.len()..)
                        .is_some_and(|tail| tail.trim_start().starts_with("in "))
                });
            if wrapped {
                self.offset += 1;
            }
            let names = if !wrapped && self.take("(") {
                Some(self.destructured_names()?)
            } else {
                None
            };
            let variable = match &names {
                Some(_) => Name {
                    text: Box::leak(format!("__item{}", name.offset).into_boxed_str()),
                    offset: name.offset,
                },
                None => self.binding()?,
            };
            let keyword = self.name()?;
            if keyword.text != "in" {
                return Err(Error {
                    offset: keyword.offset,
                    message: "Expected 'in' after the loop variable.",
                });
            }
            let mut values = self.header_expression()?;
            if self.take("..") {
                values = Expr::Range(Box::new(values), Box::new(self.header_expression()?), true);
            } else if self.take_word("until") {
                values = Expr::Range(Box::new(values), Box::new(self.header_expression()?), false);
            } else if self.take_word("downTo") {
                let end = self.header_expression()?;
                values = Expr::Progression(Box::new(values), Box::new(end), true, true, None);
            }
            if self.take_word("step") {
                let step = Box::new(self.header_expression()?);
                values = match values {
                    Expr::Range(start, end, inclusive) => {
                        Expr::Progression(start, end, inclusive, false, Some(step))
                    }
                    Expr::Progression(start, end, inclusive, down, None) => {
                        Expr::Progression(start, end, inclusive, down, Some(step))
                    }
                    other => {
                        return Err(Error {
                            offset: other.offset(),
                            message: "Use step after a range, such as 0..10 step 2.",
                        });
                    }
                };
            }
            if wrapped {
                self.expect(")", "Expected ')' after the loop's range.")?;
            }
            let mut body = self.branch_body(depth + 1)?;
            if let Some(names) = names {
                let prefix = self.component_bindings(variable, &names, false);
                body.splice(0..0, prefix);
            }
            return Ok(Statement::For(variable, values, body));
        }
        if let Some(owner) = self.object_member(name.text) {
            let this = Expr::Variable(Name {
                text: owner,
                offset: name.offset,
            });
            let target = if !self.newline_ahead() && self.take("(") {
                let mut args = self.list(")", |parser| parser.argument(0))?;
                if let Some(lambda) = self.trailing_lambda(0)? {
                    args.push(lambda);
                }
                Expr::MethodCall(Box::new(this), name, args)
            } else {
                Expr::Field(Box::new(this), name)
            };
            let target = self.postfix(target, 0)?;
            return match target {
                Expr::MethodCall(..) => Ok(Statement::MethodCall(target)),
                Expr::Field(..) | Expr::Index(..) => self.member_assignment(target),
                _ => Err(self.error("Expected a function or method call.")),
            };
        }
        if let Some(operator) = self.step_operator() {
            return Ok(Statement::Assign(
                name,
                Expr::Arithmetic(
                    Box::new(Expr::Variable(name)),
                    vec![(operator, Expr::Int(name.offset, 1))],
                ),
            ));
        }
        if let Some(operator) = self.compound_operator() {
            let value = self.expression()?;
            return Ok(Statement::Assign(
                name,
                Expr::Arithmetic(Box::new(Expr::Variable(name)), vec![(operator, value)]),
            ));
        }
        if self.take_assignment() {
            return Ok(Statement::Assign(name, self.expression()?));
        }
        let name = if self.rest().starts_with('<') {
            self.offset = name.offset;
            self.annotation()?
        } else {
            name
        };
        let receiver = self.postfix(Expr::Variable(name), 0)?;
        if matches!(receiver, Expr::MethodCall(..))
            || matches!(&receiver, Expr::Safe(inner) if matches!(inner.as_ref(), Expr::MethodCall(..)))
        {
            return Ok(Statement::MethodCall(receiver));
        }
        if matches!(receiver, Expr::Field(..) | Expr::Index(..)) {
            return self.member_assignment(receiver);
        }
        let args = if let Expr::Variable(_) = receiver
            && let Some(lambda) = self.trailing_lambda(0)?
        {
            vec![lambda]
        } else {
            self.expect("(", "Expected '(' for a call or '=' for an assignment.")?;
            let mut args = self.list(")", |parser| parser.argument(0))?;
            if let Some(lambda) = self.trailing_lambda(0)? {
                args.push(lambda);
            }
            args
        };
        match self.postfix(Expr::Call(name, args), 0)? {
            Expr::Call(name, args) if self.nothing_names.contains(&name.text) => {
                Ok(Statement::Throw(name, Expr::Call(name, args)))
            }
            Expr::Call(name, args) => Ok(Statement::Call(name, args)),
            call @ Expr::MethodCall(..) => Ok(Statement::MethodCall(call)),
            target @ (Expr::Field(..) | Expr::Index(..)) => self.member_assignment(target),
            _ => Err(self.error("Expected a function or method call.")),
        }
    }

    /// `synchronized(lock) { ... }` holds the lock in a hidden variable, takes
    /// it, and releases it on every way out of the block.
    fn synchronized(&mut self, keyword: Name<'a>, depth: usize) -> Result<Statement<'a>, Error> {
        if depth >= 63 {
            return Err(self.error("Blocks can be nested up to 64 levels."));
        }
        self.expect("(", "Expected '(' after synchronized.")?;
        let lock = self.expression()?;
        self.expect(")", "Expected ')' after the lock.")?;
        let body = self.block(depth + 1)?;
        let holder = Name {
            text: Box::leak(format!("__lock{}", keyword.offset).into_boxed_str()),
            offset: keyword.offset,
        };
        let monitor = |text: &'a str| {
            Statement::Call(
                Name {
                    text,
                    offset: keyword.offset,
                },
                vec![Expr::Variable(holder)],
            )
        };
        self.extra_statements.push(monitor("__monitorEnter"));
        self.extra_statements.push(Statement::Try(
            keyword,
            body,
            Vec::new(),
            Some(vec![monitor("__monitorExit")]),
        ));
        Ok(Statement::Binding(holder, lock, false, None))
    }

    /// `object.field = value` or `list[index] += value`, after the target.
    fn member_assignment(&mut self, target: Expr<'a>) -> Result<Statement<'a>, Error> {
        if let Some(operator) = self.step_operator() {
            let one = Expr::Int(target.offset(), 1);
            return Ok(match target {
                Expr::Field(object, field) => {
                    Statement::SetField(*object, field, Some(operator), one)
                }
                Expr::Index(list, index) => Statement::SetIndex(*list, *index, Some(operator), one),
                _ => unreachable!("only members are assigned here"),
            });
        }
        let operator = self.compound_operator();
        if operator.is_none() && !self.take_assignment() {
            return Err(self.error(if matches!(target, Expr::Field(..)) {
                "Expected '=' to change this field, or '(' to call a method."
            } else {
                "Expected '=' to change this list item."
            }));
        }
        let value = self.expression()?;
        Ok(match target {
            Expr::Field(object, field) => Statement::SetField(*object, field, operator, value),
            Expr::Index(list, index) => Statement::SetIndex(*list, *index, operator, value),
            _ => unreachable!("only members are assigned here"),
        })
    }

    /// The names in `(first, _, third)`, after its opening parenthesis.
    fn destructured_names(&mut self) -> Result<Vec<Name<'a>>, Error> {
        self.list(")", Self::binding)
    }

    fn component_bindings(
        &self,
        holder: Name<'a>,
        names: &[Name<'a>],
        mutable: bool,
    ) -> Vec<Statement<'a>> {
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.text != "_")
            .map(|(index, name)| {
                let component = Name {
                    text: Box::leak(format!("component{}", index + 1).into_boxed_str()),
                    offset: name.offset,
                };
                let call =
                    Expr::MethodCall(Box::new(Expr::Variable(holder)), component, Vec::new());
                Statement::Binding(*name, call, mutable, None)
            })
            .collect()
    }

    /// `let (a, b) = value` becomes a hidden binding and one per name.
    fn destructure(
        &mut self,
        keyword: Name<'a>,
        names: Vec<Name<'a>>,
        value: Expr<'a>,
        mutable: bool,
    ) -> Statement<'a> {
        let holder = Name {
            text: Box::leak(format!("__pair{}", keyword.offset).into_boxed_str()),
            offset: keyword.offset,
        };
        let bindings = self.component_bindings(holder, &names, mutable);
        self.extra_statements.extend(bindings);
        Statement::Binding(holder, value, false, None)
    }

    /// Reads one statement and any others it expands into.
    fn statement_list(
        &mut self,
        name: Name<'a>,
        depth: usize,
    ) -> Result<Vec<Statement<'a>>, Error> {
        let first = self.statement(name, depth)?;
        let mut statements = vec![first];
        statements.append(&mut self.extra_statements);
        Ok(statements)
    }

    /// `try { value } catch (e: Kind) { value }`, used as a value. Each branch
    /// assigns a hidden variable, and the statement form does the rest.
    fn try_value(&mut self, keyword: Name<'a>, depth: usize) -> Result<Expr<'a>, Error> {
        let holder = Name {
            text: Box::leak(format!("__try{}", keyword.offset).into_boxed_str()),
            offset: keyword.offset,
        };
        let finish = |mut body: Vec<Statement<'a>>, value: Option<Box<Expr<'a>>>| {
            let first = value
                .as_deref()
                .filter(|value| !matches!(value, Expr::Jump(_)))
                .cloned();
            match value.map(|value| *value) {
                Some(Expr::Jump(statement)) => body.push(*statement),
                Some(value) => body.push(Statement::Assign(holder, value)),
                None => {}
            }
            (body, first)
        };
        self.expect("{", "Expected '{' to open the block.")?;
        let (body, value) = self.value_body(depth)?;
        let (body, mut probe) = finish(body, value);
        let mut catches = Vec::new();
        while self.take_word("catch") {
            let parenthesized = self.take("(");
            let variable = self.binding()?;
            let kind = if self.take(":") {
                Some(self.name()?)
            } else {
                None
            };
            if parenthesized {
                self.expect(")", "Expected ')' after the caught exception.")?;
            }
            self.expect("{", "Expected '{' to open the block.")?;
            let (handler, value) = self.value_body(depth)?;
            let (handler, first) = finish(handler, value);
            probe = probe.or(first);
            catches.push(Catch {
                name: variable,
                kind,
                body: handler,
            });
        }
        let finally = if self.take_word("finally") {
            Some(self.block(depth + 1)?)
        } else {
            None
        };
        let Some(probe) = probe else {
            return Err(Error {
                offset: keyword.offset,
                message: "A try used as a value must end with a value.",
            });
        };
        Ok(Expr::TryValue(
            holder,
            Box::new(probe),
            Box::new(Statement::Try(keyword, body, catches, finally)),
        ))
    }

    /// A block, or a single statement such as `if (x) print(1)`.
    fn branch_body(&mut self, depth: usize) -> Result<Vec<Statement<'a>>, Error> {
        if self.rest_trimmed().starts_with('{') {
            return self.block(depth);
        }
        let name = self.name()?;
        self.statement_list(name, depth)
    }

    /// A block's value, or a single expression such as `if (x) 1 else 2`.
    #[allow(clippy::type_complexity)]
    fn branch_value(
        &mut self,
        depth: usize,
    ) -> Result<(Vec<Statement<'a>>, Option<Box<Expr<'a>>>), Error> {
        if self.take("{") {
            return self.value_body(depth);
        }
        let outer = std::mem::replace(&mut self.in_header, false);
        let value = self.logical(0, depth);
        self.in_header = outer;
        Ok((Vec::new(), Some(Box::new(value?))))
    }

    /// A function declared inside another. One that uses nothing from around
    /// it becomes a hidden top-level function, so it can call itself. One that
    /// does becomes a local variable holding a lambda.
    fn local_function(&mut self, keyword: Name<'a>) -> Result<Statement<'a>, Error> {
        let plain = identifier(self.rest_trimmed(), self.offset).expect("checked by the caller");
        let start = self.offset;
        let hidden: &'a str =
            Box::leak(format!("__local{}_{}", plain.offset, plain.text).into_boxed_str());
        self.local_aliases.push((plain.text, hidden));
        let aliases = self.local_aliases.len();
        let enclosing = std::mem::replace(&mut self.top_level_function, false);
        let mut function = self.function(true)?;
        self.top_level_function = enclosing;
        self.local_aliases.truncate(aliases);
        let helpers = std::mem::take(&mut self.finished_helpers);
        if !helpers.is_empty() {
            return Err(self.error("A local function cannot have default arguments yet."));
        }
        let text = &self.source[start..self.offset];
        if !self.uses_enclosing_names(text, &function) {
            function.name.text = hidden;
            self.hoisted_functions.push(function);
            return Ok(Statement::If(
                keyword,
                Expr::Bool(keyword.offset, false),
                Vec::new(),
                None,
            ));
        }
        // Recursion needs the hidden name, which only the hoisted form has.
        self.local_aliases.pop();
        if text.matches(plain.text).count() > 1 && recursive_call(text, plain.text) {
            return Err(Error {
                offset: plain.offset,
                message: "A local function that uses outer variables cannot call itself yet.",
            });
        }
        let inputs: Vec<&str> = function
            .parameters
            .iter()
            .map(|(_, kind)| kind.text)
            .collect();
        let result = function.result.map_or("Unit", |result| result.text);
        let annotation = Name {
            text: Box::leak(format!("({}) -> {result}", inputs.join(", ")).into_boxed_str()),
            offset: plain.offset,
        };
        let lambda = Lambda {
            offset: plain.offset,
            parameters: function
                .parameters
                .iter()
                .map(|(name, kind)| (*name, Some(*kind)))
                .collect(),
            explicit: true,
            body: function.body,
            value: None,
            receiver: false,
        };
        Ok(Statement::Binding(
            plain,
            Expr::Lambda(Box::new(lambda)),
            false,
            Some(annotation),
        ))
    }

    /// Whether the text of a local function mentions a variable, parameter or
    /// field declared earlier in the function around it.
    fn uses_enclosing_names(&self, text: &str, function: &Function<'a>) -> bool {
        if !self.current_type.is_empty() {
            return true;
        }
        let before = &self.source[self.function_start..self.source.len().min(self.offset)];
        let before = before.strip_suffix(text).unwrap_or(before);
        text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|word| !word.is_empty())
            .filter(|word| {
                !function
                    .parameters
                    .iter()
                    .any(|(name, _)| name.text == *word)
            })
            .any(|word| {
                ["let ", "var ", "val "]
                    .iter()
                    .any(|keyword| before.contains(&format!("{keyword}{word}")))
                    || before.contains(&format!("({word}:"))
                    || before.contains(&format!(", {word}:"))
            })
    }

    /// `object Name ...`, after the word `object`.
    fn object_declaration(&mut self) -> Result<Class<'a>, Error> {
        let mut class = self.class(false, false, false)?;
        class.is_object = true;
        let instance = Name {
            text: "INSTANCE",
            offset: class.name.offset,
        };
        class
            .static_initializers
            .insert(0, Initializer::Field(class.static_fields.len()));
        class.static_fields.push(Field {
            annotations: Vec::new(),
            protected: false,
            name: instance,
            annotation: Some(class.name),
            value: Some(Expr::Call(class.name, Vec::new())),
            mutable: false,
            private: false,
            is_lateinit: false,
        });
        Ok(class)
    }

    /// A class, enum, object or interface declared inside `outer`. It is
    /// compiled as a top-level type and can be named with or without `outer`.
    fn nested_type(
        &mut self,
        word: Name<'a>,
        is_abstract: bool,
        outer: Name<'a>,
        inner: bool,
    ) -> Result<(), Error> {
        let mut interface = None;
        let mut class = None;
        match word.text {
            "class" => class = Some(self.class(is_abstract, false, false)?),
            "data" => {
                self.take_word("class");
                let mut declared = self.class(false, true, false)?;
                add_data_members(&mut declared);
                class = Some(declared);
            }
            "enum" => {
                self.take_word("class");
                class = Some(self.class(false, false, true)?);
            }
            "sealed" => {
                if self.take_word("interface") {
                    let mut declared = self.interface()?;
                    declared.is_sealed = true;
                    interface = Some(declared);
                } else {
                    self.take_word("class");
                    let mut declared = self.class(true, false, false)?;
                    declared.is_sealed = true;
                    class = Some(declared);
                }
            }
            "object" => class = Some(self.object_declaration()?),
            _ => interface = Some(self.interface()?),
        }
        if let Some(mut class) = class {
            if inner {
                if !class.secondary.is_empty() || !class.static_methods.is_empty() {
                    return Err(Error {
                        offset: class.name.offset,
                        message: "An inner class cannot have default values, more constructors or static members yet.",
                    });
                }
                class.inner_of = Some(outer.text);
                let holder = Name {
                    text: "__outer",
                    offset: class.name.offset,
                };
                class.parameters.insert(0, (holder, outer));
                class
                    .initializers
                    .insert(0, Initializer::Field(class.fields.len()));
                class.fields.push(Field {
                    annotations: Vec::new(),
                    protected: false,
                    name: holder,
                    annotation: Some(outer),
                    value: Some(Expr::Variable(holder)),
                    mutable: false,
                    private: false,
                    is_lateinit: false,
                });
            }
            self.nested_found.push((outer.text, class.name.text));
            self.nested_classes.push(class);
        }
        if let Some(interface) = interface {
            self.nested_found.push((outer.text, interface.name.text));
            self.nested_interfaces.push(interface);
        }
        Ok(())
    }

    fn is_nested(&self, outer: &str, inner: &str) -> bool {
        let inner = inner.split('<').next().unwrap_or(inner);
        self.nested_names
            .iter()
            .any(|(o, i)| *o == outer && *i == inner)
    }

    /// Methods that answer interface properties with this class's fields.
    fn property_getters(
        &self,
        fields: &[Field<'a>],
        methods: &[Function<'a>],
    ) -> Vec<Function<'a>> {
        fields
            .iter()
            .filter(|field| {
                !field.private
                    && self.interface_properties.contains(&field.name.text)
                    && !methods
                        .iter()
                        .any(|method| method.name.text == field.name.text)
            })
            .filter_map(|field| {
                let result = field.annotation.or_else(|| {
                    let text = match field.value.as_ref()? {
                        Expr::Int(..) => "Int",
                        Expr::Long(..) => "Long",
                        Expr::Double(..) => "Double",
                        Expr::Bool(..) => "Bool",
                        Expr::Text(..) => "Text",
                        _ => return None,
                    };
                    Some(Name {
                        text,
                        offset: field.name.offset,
                    })
                })?;
                let this = Name {
                    text: self.object_scopes.last().map_or("this", |scope| scope.0),
                    offset: field.name.offset,
                };
                let value = Expr::Field(Box::new(Expr::Variable(this)), field.name);
                Some(Function {
                    annotations: Vec::new(),
                    type_parameters: Vec::new(),
                    bounds: Vec::new(),
                    exported: false,
                    foreign_name: None,
                    name: field.name,
                    parameters: Vec::new(),
                    result: Some(result),
                    body: vec![Statement::Return(field.name, Some(value))],
                    private: false,
                    protected: false,
                    is_abstract: false,
                    is_override: false,
                    extension_of: None,
                    in_prelude: false,
                    vararg: false,
                    overload_of: None,
                })
            })
            .chain(fields.iter().filter_map(|field| {
                let setter = setter_name(field.name);
                if field.private
                    || !field.mutable
                    || !self.interface_properties.contains(&setter.text)
                    || methods.iter().any(|method| method.name.text == setter.text)
                {
                    return None;
                }
                let this = Name {
                    text: self.object_scopes.last().map_or("this", |scope| scope.0),
                    offset: field.name.offset,
                };
                let value = Name {
                    text: "value",
                    offset: field.name.offset,
                };
                let store = Statement::SetField(
                    Expr::Variable(this),
                    field.name,
                    None,
                    Expr::Variable(value),
                );
                Some(accessor(
                    setter,
                    vec![(value, field.annotation?)],
                    None,
                    vec![store],
                ))
            }))
            .collect()
    }

    fn rest_trimmed(&mut self) -> &'a str {
        self.skip_space();
        self.rest()
    }

    /// `++` or `--` after a variable or member.
    fn step_operator(&mut self) -> Option<Arithmetic> {
        self.skip_space();
        if self.rest().starts_with("++") {
            self.offset += 2;
            Some(Arithmetic::Add)
        } else if self.rest().starts_with("--") {
            self.offset += 2;
            Some(Arithmetic::Sub)
        } else {
            None
        }
    }

    fn compound_operator(&mut self) -> Option<Arithmetic> {
        self.skip_space();
        for (token, operator) in [
            ("+=", Arithmetic::Add),
            ("-=", Arithmetic::Sub),
            ("*=", Arithmetic::Mul),
            ("/=", Arithmetic::Div),
            ("%=", Arithmetic::Rem),
        ] {
            if self.rest().starts_with(token) {
                self.offset += 2;
                return Some(operator);
            }
        }
        None
    }

    /// Takes a single `=` but not the `==` comparison operator.
    fn take_assignment(&mut self) -> bool {
        self.skip_space();
        if self.rest().starts_with('=') && !self.rest().starts_with("==") {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn elvis(&mut self, depth: usize) -> Result<Expr<'a>, Error> {
        let mut left = self.bit_operations(depth)?;
        loop {
            self.skip_space();
            if !self.rest().starts_with("?:") {
                return Ok(left);
            }
            self.offset += 2;
            let right = self.bit_operations(depth)?;
            left = Expr::Elvis(Box::new(left), Box::new(right));
        }
    }

    /// Reads a `when` after its keyword. With `value`, each branch gives a value.
    fn when_parts(
        &mut self,
        depth: usize,
        value: bool,
    ) -> Result<(Option<Expr<'a>>, Vec<WhenBranch<'a>>), Error> {
        if depth >= 62 {
            return Err(self.error("Blocks can be nested up to 64 levels."));
        }
        self.skip_space();
        let wrapped = self.rest().starts_with("(let ") || self.rest().starts_with("(val ");
        let subject = if self.rest().starts_with('{') {
            None
        } else if wrapped || self.rest().starts_with("let ") || self.rest().starts_with("val ") {
            // `when (let v = next()) { ... }` names the subject for its branches.
            if wrapped {
                self.offset += 1;
            }
            self.name()?;
            let variable = self.binding()?;
            self.expect("=", "Expected '=' after the subject's name.")?;
            let value = if wrapped {
                let value = self.expression()?;
                self.expect(")", "Expected ')' after the subject.")?;
                value
            } else {
                self.header_expression()?
            };
            Some(Expr::Named(variable, Box::new(value)))
        } else {
            Some(self.header_expression()?)
        };
        self.expect("{", "Expected '{' to start the when branches.")?;
        let outer = std::mem::replace(&mut self.in_header, false);
        let mut branches = Vec::new();
        let mut has_else = false;
        loop {
            self.skip_space();
            if self.take("}") {
                break;
            }
            if self.rest().is_empty() {
                return Err(self.error("Expected '}' to close the when."));
            }
            if has_else {
                return Err(self.error("The else branch must be the last one."));
            }
            let mut conditions = Vec::new();
            if self.take_word("else") {
                has_else = true;
            } else {
                loop {
                    self.skip_space();
                    let negated = subject.is_some()
                        && (self.rest().starts_with("!is") || self.rest().starts_with("!in"));
                    if negated {
                        self.offset += 1;
                    }
                    let start = self.offset;
                    let negated = negated
                        || (subject.is_some()
                            && self.take_word("not")
                            && (self.rest_trimmed().starts_with("in ")
                                || self.rest_trimmed().starts_with("is ")
                                || {
                                    self.offset = start;
                                    false
                                }));
                    let condition = if self.take_word("is") {
                        if subject.is_none() {
                            return Err(self.error(
                                "Use is in a when that has a subject, such as when value { ... }.",
                            ));
                        }
                        WhenCondition::Is(self.annotation()?)
                    } else if subject.is_some() && self.take_word("in") {
                        let start = self.arithmetic(0, depth + 1)?;
                        let inclusive = if self.take("..") {
                            true
                        } else if self.take_word("until") {
                            false
                        } else {
                            return Err(self.error("Expected .. or until in the range."));
                        };
                        let end = self.arithmetic(0, depth + 1)?;
                        WhenCondition::In(start, end, inclusive)
                    } else {
                        WhenCondition::Value(self.logical(0, depth + 1)?)
                    };
                    let condition = if negated {
                        WhenCondition::Not(Box::new(condition))
                    } else {
                        condition
                    };
                    conditions.push(condition);
                    if !self.take(",") {
                        break;
                    }
                }
            }
            self.skip_space();
            if !self.rest().starts_with("->") {
                return Err(self.error("Expected '->' after the when condition."));
            }
            self.offset += 2;
            self.skip_space();
            let (body, result) = if value {
                if self.take("{") {
                    self.value_body(depth + 2)?
                } else {
                    (Vec::new(), Some(Box::new(self.logical(0, depth + 1)?)))
                }
            } else if self.rest().starts_with('{') {
                (self.block(depth + 1)?, None)
            } else {
                let name = self.name()?;
                (self.statement_list(name, depth + 1)?, None)
            };
            branches.push(WhenBranch {
                conditions,
                body,
                value: result,
            });
        }
        self.in_header = outer;
        Ok((subject, branches))
    }

    /// Reads `if condition { ... } else { ... }` used as a value, after `if`.
    fn if_value(&mut self, offset: usize, depth: usize) -> Result<Expr<'a>, Error> {
        if depth >= 62 {
            return Err(self.error("Expressions can be nested up to 64 levels."));
        }
        let condition = self.header_expression()?;
        let (then_body, then_value) = self.branch_value(depth + 1)?;
        if !self.take_word("else") {
            return Err(self.error("An if used as a value needs an else branch."));
        }
        self.skip_space();
        let nested_offset = self.offset;
        let (else_body, else_value) = if self.take_word("if") {
            (
                Vec::new(),
                Some(Box::new(self.if_value(nested_offset, depth + 1)?)),
            )
        } else {
            self.branch_value(depth + 1)?
        };
        Ok(Expr::When(
            offset,
            None,
            vec![
                WhenBranch {
                    conditions: vec![WhenCondition::Value(condition)],
                    body: then_body,
                    value: then_value,
                },
                WhenBranch {
                    conditions: Vec::new(),
                    body: else_body,
                    value: else_value,
                },
            ],
        ))
    }

    /// Reads statements up to `}` (already past `{`). A final expression, if
    /// any, is returned separately as the block's value.
    fn value_body(
        &mut self,
        depth: usize,
    ) -> Result<(Vec<Statement<'a>>, Option<Box<Expr<'a>>>), Error> {
        let outer = std::mem::replace(&mut self.in_header, false);
        let mut body = Vec::new();
        let mut value = None;
        loop {
            self.skip_space();
            if self.take("}") {
                break;
            }
            if self.rest().is_empty() {
                self.in_header = outer;
                return Err(self.error("Expected '}' to close the block."));
            }
            let start = self.offset;
            if let Ok(expr) = self.logical(0, depth + 1) {
                self.skip_space();
                if self.rest().starts_with('}') {
                    self.offset += 1;
                    value = Some(Box::new(expr));
                    break;
                }
            }
            self.offset = start;
            let name = self.name()?;
            body.append(&mut self.statement_list(name, depth + 1)?);
        }
        self.in_header = outer;
        Ok((body, value))
    }

    /// Whether the next token starts on a later line.
    fn newline_ahead(&self) -> bool {
        // Whitespace before the current position may already have been skipped.
        let before = &self.source[..self.offset];
        let skipped = before.trim_end_matches(|ch: char| ch.is_ascii_whitespace());
        if before[skipped.len()..].contains('\n') {
            return true;
        }
        let mut rest = self.rest();
        loop {
            let trimmed = rest.trim_start_matches([' ', '\t', '\r']);
            if trimmed.starts_with('\n') {
                return true;
            }
            if let Some(comment) = trimmed.strip_prefix("//") {
                rest = comment.find('\n').map_or("", |index| &comment[index..]);
                continue;
            }
            return false;
        }
    }

    fn comparison(&mut self, depth: usize) -> Result<Expr<'a>, Error> {
        let mut left = self.elvis(depth)?;
        if !self.newline_ahead() && self.take_word("to") {
            let pair = Name {
                text: "Pair",
                offset: left.offset(),
            };
            left = Expr::Call(pair, vec![left, self.elvis(depth)?]);
        }
        while !self.newline_ahead() {
            let Some(word) = identifier(self.rest_trimmed(), self.offset) else {
                break;
            };
            if !self.infix_names.contains(&word.text) {
                break;
            }
            self.offset += word.text.len();
            let right = self.elvis(depth)?;
            left = Expr::MethodCall(Box::new(left), word, vec![right]);
        }
        if !self.newline_ahead() && self.take_word("is") {
            return Ok(Expr::Is(Box::new(left), self.annotation()?));
        }
        if !self.newline_ahead() {
            self.skip_space();
            if self.rest().starts_with("!is")
                && !self.rest()[3..].starts_with(|ch: char| ch.is_alphanumeric() || ch == '_')
            {
                self.offset += 3;
                let offset = left.offset();
                let check = Expr::Is(Box::new(left), self.annotation()?);
                return Ok(Expr::Not(offset, true, Box::new(check)));
            }
        }
        if !self.newline_ahead() {
            let start = self.offset;
            self.skip_space();
            let negated = if self.rest().starts_with("!in")
                && !self.rest()[3..].starts_with(|ch: char| ch.is_alphanumeric() || ch == '_')
            {
                self.offset += 1;
                true
            } else {
                self.take_word("not")
            };
            if self.take_word("in") {
                let values = self.elvis(depth)?;
                let range = if self.take("..") {
                    Some(Expr::Range(
                        Box::new(values),
                        Box::new(self.elvis(depth)?),
                        true,
                    ))
                } else if self.take_word("until") {
                    Some(Expr::Range(
                        Box::new(values),
                        Box::new(self.elvis(depth)?),
                        false,
                    ))
                } else {
                    return Ok(Self::contains_check(left, values, negated));
                };
                return Ok(Expr::InRange(
                    Box::new(left),
                    Box::new(range.unwrap()),
                    negated,
                ));
            }
            self.offset = start;
        }
        for (token, operator) in [
            ("==", Comparison::Eq),
            ("!=", Comparison::Ne),
            ("<=", Comparison::Le),
            (">=", Comparison::Ge),
            ("<", Comparison::Lt),
            (">", Comparison::Gt),
        ] {
            if self.take(token) {
                return Ok(Expr::Compare(
                    Box::new(left),
                    operator,
                    Box::new(self.elvis(depth)?),
                ));
            }
        }
        Ok(left)
    }

    /// `item in values` asks `values.contains(item)`.
    fn contains_check(item: Expr<'a>, values: Expr<'a>, negated: bool) -> Expr<'a> {
        let offset = item.offset();
        let call = Expr::MethodCall(
            Box::new(values),
            Name {
                text: "contains",
                offset,
            },
            vec![item],
        );
        if negated {
            Expr::Not(offset, true, Box::new(call))
        } else {
            call
        }
    }

    fn expression(&mut self) -> Result<Expr<'a>, Error> {
        let first = self.logical(0, 0)?;
        self.range_value(first, 0)
    }

    /// `1..3` used as a value holds its numbers in a list.
    fn range_value(&mut self, first: Expr<'a>, depth: usize) -> Result<Expr<'a>, Error> {
        if self.in_header || self.newline_ahead() {
            return Ok(first);
        }
        let function = if self.take("..") {
            "__rangeTo"
        } else if self.take_word("until") {
            "__rangeUntil"
        } else if self.take_word("downTo") {
            "__rangeDown"
        } else {
            return Ok(first);
        };
        let name = Name {
            text: function,
            offset: first.offset(),
        };
        let last = self.logical(0, depth)?;
        Ok(Expr::Call(name, vec![first, last]))
    }

    /// An expression directly followed by a block, as in `if ready { ... }`.
    fn header_expression(&mut self) -> Result<Expr<'a>, Error> {
        let outer = std::mem::replace(&mut self.in_header, true);
        let result = self.logical(0, 0);
        self.in_header = outer;
        result
    }

    /// Takes a `{` that starts a trailing lambda after a call.
    fn trailing_lambda(&mut self, depth: usize) -> Result<Option<Expr<'a>>, Error> {
        self.skip_space();
        if self.in_header || !self.rest().starts_with('{') {
            return Ok(None);
        }
        let offset = self.offset;
        self.offset += 1;
        Ok(Some(self.lambda(offset, depth)?))
    }

    fn lambda(&mut self, offset: usize, depth: usize) -> Result<Expr<'a>, Error> {
        if depth >= 63 {
            return Err(self.error("Expressions can be nested up to 64 levels."));
        }
        let parameters = self.lambda_parameters();
        let prefix = std::mem::take(&mut self.lambda_prefix);
        let explicit = parameters.is_some();
        let parameters = parameters.unwrap_or_default();
        let (mut body, value) = self.value_body(depth)?;
        body.splice(0..0, prefix);
        Ok(Expr::Lambda(Box::new(Lambda {
            offset,
            parameters,
            explicit,
            body,
            value,
            receiver: false,
        })))
    }

    /// Reads `x, y: Int ->` at the start of a lambda. Restores the position
    /// and returns `None` when the lambda has no parameter list.
    fn lambda_parameters(&mut self) -> Option<Vec<(Name<'a>, Option<Name<'a>>)>> {
        let start = self.offset;
        let mut parameters = Vec::new();
        self.skip_space();
        if self.rest().starts_with("->") {
            self.offset += 2;
            return Some(parameters);
        }
        loop {
            self.skip_space();
            let at = self.offset;
            let destructured = if self.take("(") {
                match self.list(")", Self::binding) {
                    Ok(names) => Some(names),
                    Err(_) => {
                        self.offset = start;
                        self.lambda_prefix.clear();
                        return None;
                    }
                }
            } else {
                None
            };
            let binding = match &destructured {
                Some(_) => Ok(Name {
                    text: Box::leak(format!("__pair{at}").into_boxed_str()),
                    offset: at,
                }),
                None => self.binding(),
            };
            let Ok(name) = binding else {
                self.offset = start;
                self.lambda_prefix.clear();
                return None;
            };
            if let Some(names) = destructured {
                let bindings = self.component_bindings(name, &names, false);
                self.lambda_prefix.extend(bindings);
            }
            let annotation = if self.take(":") {
                match self.annotation() {
                    Ok(annotation) => Some(annotation),
                    Err(_) => {
                        self.offset = start;
                        self.lambda_prefix.clear();
                        return None;
                    }
                }
            } else {
                None
            };
            parameters.push((name, annotation));
            self.skip_space();
            if self.rest().starts_with("->") {
                self.offset += 2;
                return Some(parameters);
            }
            if !self.take(",") {
                self.offset = start;
                self.lambda_prefix.clear();
                return None;
            }
        }
    }

    fn logical(&mut self, precedence: u8, depth: usize) -> Result<Expr<'a>, Error> {
        let (word, logic) = match precedence {
            0 => ("or", Logic::Or),
            1 => ("and", Logic::And),
            _ => {
                self.skip_space();
                let offset = self.offset;
                if !self.take_word("not") {
                    return self.comparison(depth);
                }
                let mut odd = true;
                while self.take_word("not") {
                    odd = !odd;
                }
                return Ok(Expr::Not(offset, odd, Box::new(self.comparison(depth)?)));
            }
        };
        let first = self.logical(precedence + 1, depth)?;
        if !self.take_word(word) {
            return Ok(first);
        }
        let mut terms = vec![first];
        loop {
            terms.push(self.logical(precedence + 1, depth)?);
            if !self.take_word(word) {
                return Ok(Expr::Logic(logic, terms));
            }
        }
    }

    fn binding(&mut self) -> Result<Name<'a>, Error> {
        let name = self.name()?;
        if matches!(
            name.text,
            "true"
                | "false"
                | "null"
                | "and"
                | "or"
                | "not"
                | "fun"
                | "let"
                | "var"
                | "if"
                | "for"
                | "while"
                | "return"
                | "class"
                | "interface"
                | "this"
                | "break"
                | "continue"
                | "else"
                | "try"
                | "catch"
                | "finally"
                | "throw"
                | "as"
                | "is"
                | "when"
        ) {
            return Err(Error {
                offset: name.offset,
                message: "Choose a name that is not a Mammoth keyword.",
            });
        }
        Ok(name)
    }

    /// `a shl 2`, `a & b`, `a | b` and `a ^ b`, below arithmetic.
    fn bit_operations(&mut self, depth: usize) -> Result<Expr<'a>, Error> {
        let mut left = self.arithmetic(0, depth)?;
        loop {
            if self.newline_ahead() {
                return Ok(left);
            }
            self.skip_space();
            let rest = self.rest();
            let operator = if self.take_word("shl") {
                Arithmetic::Shl
            } else if self.take_word("shr") {
                Arithmetic::Shr
            } else if self.take_word("ushr") {
                Arithmetic::Ushr
            } else if rest.starts_with('&') && !rest.starts_with("&&") {
                self.offset += 1;
                Arithmetic::BitAnd
            } else if rest.starts_with('|') && !rest.starts_with("||") {
                self.offset += 1;
                Arithmetic::BitOr
            } else if rest.starts_with('^') {
                self.offset += 1;
                Arithmetic::BitXor
            } else {
                return Ok(left);
            };
            let right = self.arithmetic(0, depth)?;
            left = Expr::Arithmetic(Box::new(left), vec![(operator, right)]);
        }
    }

    fn arithmetic(&mut self, precedence: u8, depth: usize) -> Result<Expr<'a>, Error> {
        let operators: &[(&str, Arithmetic)] = match precedence {
            0 => &[("+", Arithmetic::Add), ("-", Arithmetic::Sub)],
            1 => &[
                ("*", Arithmetic::Mul),
                ("/", Arithmetic::Div),
                ("%", Arithmetic::Rem),
            ],
            _ => {
                let receiver = self.atom(depth)?;
                let mut value = self.postfix(receiver, depth)?;
                while self.take_word("as") {
                    let safe = self.rest().starts_with('?');
                    if safe {
                        self.offset += 1;
                    }
                    value = Expr::Cast(Box::new(value), self.annotation()?, safe);
                }
                return Ok(value);
            }
        };
        let first = self.arithmetic(precedence + 1, depth)?;
        let mut rest = Vec::new();
        while let Some((_, operator)) = operators.iter().find(|(token, _)| {
            // A line starting with `-` begins a new expression, such as a
            // negative `when` value, rather than continuing a subtraction.
            if *token == "-" && self.newline_ahead() {
                return false;
            }
            self.skip_space();
            !(*token == "-" && self.rest().starts_with("->")) && self.take(token)
        }) {
            rest.push((*operator, self.arithmetic(precedence + 1, depth)?));
        }
        if rest.is_empty() {
            Ok(first)
        } else {
            Ok(Expr::Arithmetic(Box::new(first), rest))
        }
    }

    fn postfix(&mut self, mut receiver: Expr<'a>, depth: usize) -> Result<Expr<'a>, Error> {
        for nested in depth..=64 {
            // `make()(3)` or `handlers[0](3)` calls the function value that
            // the call or index gives. The `(` must follow directly, so a
            // parenthesized expression on the next line stays separate.
            if self.rest().starts_with('(')
                && self.source[..self.offset].ends_with([')', ']'])
                && matches!(
                    receiver,
                    Expr::Call(..) | Expr::MethodCall(..) | Expr::Index(..)
                )
            {
                if nested == 64 {
                    return Err(self.error("Expressions can be nested up to 64 levels."));
                }
                let invoke = Name {
                    text: "invoke",
                    offset: self.offset,
                };
                self.offset += 1;
                let args = self.list(")", |parser| parser.logical(0, nested + 1))?;
                receiver = Expr::MethodCall(Box::new(receiver), invoke, args);
                continue;
            }
            if self.rest().starts_with("!!") {
                self.offset += 2;
                let require = Name {
                    text: "requireValue",
                    offset: receiver.offset(),
                };
                receiver = Expr::Call(require, vec![receiver]);
                continue;
            }
            self.skip_space();
            let safe = self.rest().starts_with("?.");
            if safe {
                self.offset += 2;
            }
            let method = safe || (!self.rest().starts_with("..") && self.take("."));
            if !method && (self.newline_ahead() || !self.take("[")) {
                return Ok(receiver);
            }
            if nested == 64 {
                return Err(self.error("Expressions can be nested up to 64 levels."));
            }
            if method {
                let name = self.name()?;
                let name = if self.rest().starts_with('<') {
                    self.offset = name.offset;
                    self.annotation()?
                } else {
                    name
                };
                // `Outer.Inner` names a nested type.
                if let Expr::Variable(outer) = &receiver
                    && self.is_nested(outer.text, name.text)
                {
                    receiver = if !self.newline_ahead() && self.take("(") {
                        let mut args = self.list(")", |parser| parser.argument(nested + 1))?;
                        if let Some(lambda) = self.trailing_lambda(nested + 1)? {
                            args.push(lambda);
                        }
                        Expr::Call(name, args)
                    } else {
                        Expr::Variable(name)
                    };
                    continue;
                }
                if !self.newline_ahead() && self.take("(") {
                    let mut args = self.list(")", |parser| parser.argument(nested + 1))?;
                    if let Some(lambda) = self.trailing_lambda(nested + 1)? {
                        args.push(lambda);
                    }
                    receiver = Expr::MethodCall(Box::new(receiver), name, args);
                } else if let Some(lambda) = self.trailing_lambda(nested + 1)? {
                    let lambda = if matches!(name.text, "apply" | "run") {
                        with_receiver(lambda)
                    } else {
                        lambda
                    };
                    receiver = Expr::MethodCall(Box::new(receiver), name, vec![lambda]);
                } else {
                    receiver = Expr::Field(Box::new(receiver), name);
                }
                if safe {
                    receiver = Expr::Safe(Box::new(receiver));
                }
            } else {
                let index = self.logical(0, nested + 1)?;
                self.expect("]", "Expected ']' after the list index.")?;
                receiver = Expr::Index(Box::new(receiver), Box::new(index));
            }
        }
        unreachable!()
    }

    fn atom(&mut self, depth: usize) -> Result<Expr<'a>, Error> {
        self.skip_space();
        let offset = self.offset;
        if self.take("[") {
            if depth == 64 {
                return Err(self.error("Expressions can be nested up to 64 levels."));
            }
            let values = self.list("]", |parser| parser.logical(0, depth + 1))?;
            return Ok(Expr::List(offset, values));
        }
        if self.take("(") {
            if depth == 64 {
                return Err(self.error("Expressions can be nested up to 64 levels."));
            }
            let outer = std::mem::replace(&mut self.in_header, false);
            let inner = self.logical(0, depth + 1)?;
            let inner = self.range_value(inner, depth + 1)?;
            self.in_header = outer;
            self.expect(")", "Expected ')' to close the expression.")?;
            return Ok(inner);
        }
        if self.rest().starts_with("::") {
            self.offset += 2;
            let target = self.name()?;
            if self.discovering {
                // The second parse knows how many parameters it has.
                self.saw_dotted_type = true;
                return Ok(Expr::Null(offset));
            }
            let Some((_, kinds)) = self
                .arities
                .iter()
                .find(|(name, _)| *name == target.text)
                .cloned()
            else {
                return Err(at_name(
                    target,
                    "Use :: with a function declared in this file.",
                ));
            };
            let names: Vec<Name<'a>> = (0..kinds.len())
                .map(|index| Name {
                    text: Box::leak(format!("__arg{index}").into_boxed_str()),
                    offset,
                })
                .collect();
            let arguments = names.iter().map(|name| Expr::Variable(*name)).collect();
            return Ok(Expr::Lambda(Box::new(Lambda {
                offset,
                parameters: names.into_iter().zip(kinds).collect(),
                explicit: true,
                body: Vec::new(),
                value: Some(Box::new(Expr::Call(target, arguments))),
                receiver: false,
            })));
        }
        if self.take("\"") {
            if self.rest().starts_with("\"\"") {
                self.offset += 2;
                let Some(length) = self.rest().find("\"\"\"") else {
                    return Err(self.error("Expected three quotes to close this text."));
                };
                let text = self.rest()[..length].replace("\r\n", "\n");
                self.offset += length + 3;
                return Ok(Expr::Text(offset, vec![Part::Literal(text)]));
            }
            return Ok(Expr::Text(offset, self.read_text()?));
        }
        if self.take("{") {
            return self.lambda(offset, depth + 1);
        }
        if self.take("'") {
            return self.read_char(offset);
        }
        if self.rest().starts_with('-')
            && !self.rest()[1..].starts_with(|ch: char| ch.is_ascii_digit())
        {
            self.offset += 1;
            if depth == 64 {
                return Err(self.error("Expressions can be nested up to 64 levels."));
            }
            let inner = self.atom(depth + 1)?;
            let inner = self.postfix(inner, depth + 1)?;
            return Ok(Expr::Negate(offset, Box::new(inner)));
        }
        let unsigned = self.rest().strip_prefix('-').unwrap_or(self.rest());
        let negative = unsigned.len() != self.rest().len();
        for (prefix, radix) in [("0x", 16), ("0X", 16), ("0b", 2), ("0B", 2)] {
            if !unsigned.starts_with(prefix) {
                continue;
            }
            let body = &unsigned[2..];
            let length = body
                .bytes()
                .take_while(|byte| (*byte as char).is_digit(radix) || *byte == b'_')
                .count();
            let text: String = body[..length].chars().filter(|ch| *ch != '_').collect();
            let value = i128::from_str_radix(&text, radix).map_err(|_| Error {
                offset,
                message: "Expected digits after the number's prefix.",
            })?;
            let value = if negative { -value } else { value };
            self.offset += usize::from(negative) + 2 + length;
            let long = self.rest().starts_with('L');
            if long {
                self.offset += 1;
            }
            if !long && let Ok(value) = i32::try_from(value) {
                return Ok(Expr::Int(offset, value));
            }
            let value = i64::try_from(value).map_err(|_| Error {
                offset,
                message: "This integer is outside the range of Long.",
            })?;
            return Ok(Expr::Long(offset, value));
        }
        let length = unsigned
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || *byte == b'_')
            .count();
        if unsigned.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && unsigned[..length].contains('_')
        {
            let text: String = unsigned[..length].chars().filter(|ch| *ch != '_').collect();
            let value: i64 = text.parse().map_err(|_| Error {
                offset,
                message: "This integer is outside the range of Long.",
            })?;
            let value = if negative { -value } else { value };
            self.offset += usize::from(negative) + length;
            let long = self.rest().starts_with('L');
            if long {
                self.offset += 1;
            }
            if !long && let Ok(value) = i32::try_from(value) {
                return Ok(Expr::Int(offset, value));
            }
            return Ok(Expr::Long(offset, value));
        }
        let digits = unsigned.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            self.offset += digits + usize::from(unsigned.len() != self.rest().len());
            let mut decimal = false;
            if self.rest().starts_with('.')
                && self
                    .rest()
                    .as_bytes()
                    .get(1)
                    .is_some_and(u8::is_ascii_digit)
            {
                decimal = true;
                self.offset += 1;
                self.offset += self.rest().bytes().take_while(u8::is_ascii_digit).count();
            }
            if self.rest().starts_with(['e', 'E']) {
                decimal = true;
                self.offset += 1;
                if self.rest().starts_with(['+', '-']) {
                    self.offset += 1;
                }
                let digits = self.rest().bytes().take_while(u8::is_ascii_digit).count();
                if digits == 0 {
                    return Err(self.error("Expected digits in the exponent."));
                }
                self.offset += digits;
            }
            if decimal {
                let value: f64 = self.source[offset..self.offset]
                    .parse()
                    .map_err(|_| Error {
                        offset,
                        message: "Expected a decimal number.",
                    })?;
                if !value.is_finite() {
                    return Err(Error {
                        offset,
                        message: "This decimal literal is outside the range of Double.",
                    });
                }
                return Ok(Expr::Double(offset, value));
            }
            let digits = &self.source[offset..self.offset];
            if self.rest().starts_with('L')
                && !self.rest()[1..].starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
            {
                self.offset += 1;
                let value = digits.parse().map_err(|_| Error {
                    offset,
                    message: "This integer is outside the range of Long.",
                })?;
                return Ok(Expr::Long(offset, value));
            }
            if let Ok(value) = digits.parse() {
                return Ok(Expr::Int(offset, value));
            }
            let value = digits.parse().map_err(|_| Error {
                offset,
                message: "This integer is outside the range of Long.",
            })?;
            return Ok(Expr::Long(offset, value));
        }
        if let Some(name) = identifier(self.rest(), offset) {
            self.offset += name.text.len();
            let mut name = name;
            // Borrow the qualified receiver just like a generic call name.
            if name.text == "super" && self.take("<") {
                self.name()?;
                self.expect(">", "Expected '>' after the parent interface name.")?;
                name.text = &self.source[offset..self.offset];
                return Ok(Expr::Variable(name));
            }
            // `ArrayList<Text>()` creates a Java object with type arguments.
            if self.rest().starts_with('<') {
                let start = self.offset;
                let parsed = (|| -> Result<(), Error> {
                    self.offset += 1;
                    loop {
                        self.annotation()?;
                        if self.take(">") {
                            return Ok(());
                        }
                        self.expect(",", "")?;
                    }
                })();
                let lambda_follows =
                    !self.in_header && self.rest().trim_start_matches(' ').starts_with('{');
                if parsed.is_ok() && (self.rest().starts_with('(') || lambda_follows) {
                    name = Name {
                        text: &self.source[offset..self.offset],
                        offset,
                    };
                } else {
                    self.offset = start;
                }
            }
            if name.text == "if" {
                return self.if_value(offset, depth + 1);
            }
            if name.text == "when" {
                let (subject, branches) = self.when_parts(depth + 1, true)?;
                return Ok(Expr::When(offset, subject.map(Box::new), branches));
            }
            if matches!(name.text, "return" | "throw" | "break" | "continue") {
                return Ok(Expr::Jump(Box::new(self.statement(name, depth + 1)?)));
            }
            if name.text == "try" && self.rest_trimmed().starts_with('{') {
                return self.try_value(name, depth + 1);
            }
            if name.text == "object"
                && (self.rest_trimmed().starts_with('{')
                    || self.rest().starts_with("implements ")
                    || self.rest().starts_with("extends "))
            {
                return self.object_expression(name);
            }
            if let Some(&(_, hidden)) = self
                .local_aliases
                .iter()
                .rev()
                .find(|(plain, _)| *plain == name.text)
            {
                name.text = hidden;
            }
            if let Some(owner) = self.object_member(name.text) {
                let this = Expr::Variable(Name {
                    text: owner,
                    offset,
                });
                let target = if !self.newline_ahead() && self.take("(") {
                    let mut args = self.list(")", |parser| parser.argument(depth + 1))?;
                    if let Some(lambda) = self.trailing_lambda(depth + 1)? {
                        args.push(lambda);
                    }
                    Expr::MethodCall(Box::new(this), name, args)
                } else {
                    Expr::Field(Box::new(this), name)
                };
                return Ok(target);
            }
            if name.text == "this" && self.rest().starts_with('@') {
                self.offset += 1;
                let label = self.name()?;
                name.text = Box::leak(format!("this@{}", label.text).into_boxed_str());
                return Ok(Expr::Variable(name));
            }
            if self.rest().starts_with("::")
                && name.text.starts_with(|ch: char| ch.is_ascii_uppercase())
            {
                self.offset += 2;
                let member = self.name()?;
                if member.text != "class" {
                    let input = Name {
                        text: "__input",
                        offset,
                    };
                    let read = Expr::Field(Box::new(Expr::Variable(input)), member);
                    return Ok(Expr::Lambda(Box::new(Lambda {
                        offset,
                        parameters: vec![(input, Some(name))],
                        explicit: true,
                        body: Vec::new(),
                        value: Some(Box::new(read)),
                        receiver: false,
                    })));
                }
                self.offset -= member.text.len() + 2;
            }
            if !matches!(name.text, "true" | "false" | "null")
                && !self.newline_ahead()
                && self.take("(")
            {
                if depth == 64 {
                    return Err(self.error("Expressions can be nested up to 64 levels."));
                }
                let mut args = self.list(")", |parser| parser.argument(depth + 1))?;
                if let Some(lambda) = self.trailing_lambda(depth + 1)? {
                    args.push(if name.text == "with" {
                        with_receiver(lambda)
                    } else {
                        lambda
                    });
                }
                if self.nothing_names.contains(&name.text) {
                    let call = Expr::Call(name, args);
                    return Ok(Expr::Jump(Box::new(Statement::Throw(name, call))));
                }
                return Ok(Expr::Call(name, args));
            }
            if !matches!(name.text, "true" | "false" | "null" | "this")
                && let Some(lambda) = self.trailing_lambda(depth + 1)?
            {
                let lambda = if matches!(
                    name.text.split('<').next(),
                    Some("buildString" | "buildList")
                ) {
                    with_receiver(lambda)
                } else {
                    lambda
                };
                return Ok(Expr::Call(name, vec![lambda]));
            }
            return Ok(match name.text {
                "true" => Expr::Bool(offset, true),
                "false" => Expr::Bool(offset, false),
                "null" => Expr::Null(offset),
                _ => Expr::Variable(name),
            });
        }
        Err(self.error("Expected a value such as text, an integer, or a variable."))
    }

    fn read_text(&mut self) -> Result<Vec<Part<'a>>, Error> {
        let mut parts = Vec::new();
        let mut text = String::new();
        while let Some(ch) = self.rest().chars().next() {
            let offset = self.offset;
            self.offset += ch.len_utf8();
            match ch {
                '"' => {
                    if !text.is_empty() || parts.is_empty() {
                        parts.push(Part::Literal(text));
                    }
                    return Ok(parts);
                }
                '\n' | '\r' => {
                    return Err(Error {
                        offset,
                        message: "Expected a closing quote before the end of the line.",
                    });
                }
                '\\' => {
                    let escaped = match self.rest().as_bytes().first() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'{') => '{',
                        Some(b'}') => '}',
                        _ => {
                            return Err(Error {
                                offset,
                                message: "Unknown text escape.",
                            });
                        }
                    };
                    self.offset += 1;
                    text.push(escaped);
                }
                '{' => {
                    if let Some(expr) = self.interpolation() {
                        if !text.is_empty() {
                            parts.push(Part::Literal(std::mem::take(&mut text)));
                        }
                        parts.push(Part::Value(expr));
                    } else {
                        text.push(ch);
                    }
                }
                _ => text.push(ch),
            }
        }
        Err(self.error("Expected a closing quote."))
    }

    /// Parses `{expression}` inside text, starting just after the `{`.
    /// Returns `None` (leaving the offset unchanged) when the braces do not
    /// contain a complete expression, so the brace is kept as literal text.
    fn interpolation(&mut self) -> Option<Expr<'a>> {
        let rest = self.rest().as_bytes();
        let mut depth = 0usize;
        let mut index = 0;
        let mut end = None;
        while index < rest.len() {
            match rest[index] {
                b'\n' | b'\r' | b'\\' => return None,
                b'{' => depth += 1,
                b'}' if depth == 0 => {
                    end = Some(index);
                    break;
                }
                b'}' => depth -= 1,
                b'"' => {
                    index += 1;
                    while index < rest.len() && rest[index] != b'"' {
                        if matches!(rest[index], b'\n' | b'\r' | b'\\') {
                            return None;
                        }
                        index += 1;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        let end = self.offset + end?;
        if self.source[self.offset..end].trim().is_empty() {
            return None;
        }
        let mut inner = Parser {
            source: &self.source[..end],
            offset: self.offset,
            in_header: false,
            pending_defaults: Vec::new(),
            finished_helpers: Vec::new(),
            top_level_function: false,
            extra_statements: Vec::new(),
            nested_names: self.nested_names.clone(),
            nested_found: Vec::new(),
            discovering: self.discovering,
            saw_dotted_type: false,
            saw_vararg: false,
            local_aliases: self.local_aliases.clone(),
            hoisted_functions: Vec::new(),
            object_scopes: self.object_scopes.clone(),
            lambda_prefix: Vec::new(),
            infix_names: self.infix_names.clone(),
            nothing_names: self.nothing_names.clone(),
            function_start: self.function_start,
            current_type: "",
            declared_functions: Vec::new(),
            overloaded: self.overloaded.clone(),
            interface_properties: self.interface_properties.clone(),
            properties_found: Vec::new(),
            arities: self.arities.clone(),
            nested_classes: Vec::new(),
            nested_interfaces: Vec::new(),
        };
        let expr = inner.logical(0, 1).ok()?;
        inner.skip_space();
        if inner.offset != end {
            return None;
        }
        self.offset = end + 1;
        Some(expr)
    }

    fn read_char(&mut self, offset: usize) -> Result<Expr<'a>, Error> {
        let invalid = Error {
            offset,
            message: "Write one character between single quotes, such as 'a'.",
        };
        let ch = self.rest().chars().next().ok_or(invalid)?;
        self.offset += ch.len_utf8();
        let value = match ch {
            '\'' | '\n' | '\r' => return Err(invalid),
            '\\' => {
                let escaped = match self.rest().as_bytes().first() {
                    Some(b'\'') => '\'',
                    Some(b'"') => '"',
                    Some(b'\\') => '\\',
                    Some(b'n') => '\n',
                    Some(b'r') => '\r',
                    Some(b't') => '\t',
                    Some(b'0') => '\0',
                    _ => {
                        return Err(Error {
                            offset,
                            message: "Unknown character escape.",
                        });
                    }
                };
                self.offset += 1;
                escaped
            }
            ch => ch,
        };
        let mut units = [0u16; 2];
        let encoded = value.encode_utf16(&mut units);
        if encoded.len() != 1 || !self.rest().starts_with('\'') {
            return Err(invalid);
        }
        self.offset += 1;
        Ok(Expr::Char(offset, units[0]))
    }
}

fn at_name(name: Name<'_>, message: &'static str) -> Error {
    Error {
        offset: name.offset,
        message,
    }
}

fn identifier(source: &str, offset: usize) -> Option<Name<'_>> {
    let mut bytes = source.bytes();
    if !bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
    {
        return None;
    }
    let length = 1 + bytes
        .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .count();
    Some(Name {
        text: &source[..length],
        offset,
    })
}
