//! Top-level variables that functions and classes use. They become public static
//! fields of the entry class so every function and method can reach them.

use std::collections::{HashMap, HashSet};

use super::{
    Emitter, Error, Expr, Kind, Lambda, Method, Name, Part, Program, Statement, Symbols, at,
    instruction, resolve_type,
};

#[derive(Clone)]
pub(super) struct Global<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub mutable: bool,
    /// Source offset of the declaring `let` or `var` name.
    pub offset: usize,
}

/// Finds the top-level bindings that are used by name inside a function, method,
/// field initializer, or init block, and works out their types.
pub(super) fn resolve_globals<'a>(
    program: &'a Program<'a>,
    symbols: &Symbols<'a>,
) -> Result<Vec<Global<'a>>, Error> {
    let used = collect_program(program, HashSet::new());
    // A name only called matters when the variable holds a function.
    let called = collect_program(program, HashSet::from([RECORD_CALLS]));
    let mut known: HashMap<&str, Kind> = HashMap::new();
    let mut globals = Vec::new();
    let mut declared = HashSet::new();
    for statement in &program.body {
        let Statement::Binding(name, value, mutable, annotation) = statement else {
            continue;
        };
        let kind = match annotation {
            Some(annotation) => Some(resolve_type(&symbols.types, *annotation)?),
            None => symbols.static_kind(value, &|variable| known.get(variable).copied()),
        };
        if let Some(kind) = kind {
            known.insert(name.text, kind);
        } else {
            known.remove(name.text);
        }
        let holds_function = matches!(kind, Some(Kind::Function(_)));
        if !used.contains(name.text) && !(holds_function && called.contains(name.text)) {
            continue;
        }
        if !declared.insert(name.text) {
            return Err(at(
                *name,
                "A top-level variable used by functions can only be declared once.",
            ));
        }
        let kind = kind.filter(|kind| *kind != Kind::Null).ok_or_else(|| {
            at(
                *name,
                "Functions use this variable, so give it a type, such as `var score: Int = 0`.",
            )
        })?;
        if name.text.len() > u16::MAX as usize {
            return Err(at(*name, "This variable name is too long for the JVM."));
        }
        globals.push(Global {
            name: name.text,
            kind,
            mutable: *mutable,
            offset: name.offset,
        });
    }
    Ok(globals)
}

fn collect_program<'a>(program: &'a Program<'a>, mut used: HashSet<&'a str>) -> HashSet<&'a str> {
    for function in &program.functions {
        collect_statements(&function.body, &mut used);
    }
    for class in &program.classes {
        for method in &class.methods {
            collect_statements(&method.body, &mut used);
        }
        for field in &class.fields {
            if let Some(value) = &field.value {
                collect_expr(value, &mut used);
            }
        }
        for initializer in &class.initializers {
            if let super::Initializer::Block(statements) = initializer {
                collect_statements(statements, &mut used);
            }
        }
        if let Some((_, args)) = &class.extends {
            for arg in args {
                collect_expr(arg, &mut used);
            }
        }
    }
    collect_lambdas(&program.body, &mut used);
    used
}

/// Present in a name set to record the names of calls too.
pub(super) const RECORD_CALLS: &str = "\0record-calls";

/// Present in a name set while walking only the code that runs immediately,
/// since a lambda's body runs later, when it is called.
const SKIP_LAMBDAS: &str = "\0skip-lambdas";

/// Rejects a top-level statement that, through the functions and constructors
/// it calls, reads a top-level variable declared further down. That variable
/// would still hold null or zero, whatever its type says. Method calls are
/// followed by name, and lambdas are not followed.
pub(super) fn check_global_order<'a>(
    program: &'a Program<'a>,
    globals: &[Global<'a>],
) -> Result<(), Error> {
    if globals.is_empty() {
        return Ok(());
    }
    let mut reads: HashMap<&str, HashSet<&str>> = HashMap::new();
    for function in &program.functions {
        let mut used = HashSet::from([SKIP_LAMBDAS]);
        collect_statements(&function.body, &mut used);
        let mut local = HashSet::new();
        collect_bound(&function.body, &mut local);
        local.extend(function.parameters.iter().map(|(name, _)| name.text));
        used.retain(|name| !local.contains(name));
        reads.entry(function.name.text).or_default().extend(used);
    }
    for class in &program.classes {
        let mut used = HashSet::from([SKIP_LAMBDAS]);
        let mut local: HashSet<&str> = class.parameters.iter().map(|(name, _)| name.text).collect();
        local.extend(class.fields.iter().map(|field| field.name.text));
        for field in &class.fields {
            if let Some(value) = &field.value {
                collect_expr(value, &mut used);
            }
        }
        for initializer in &class.initializers {
            if let super::Initializer::Block(statements) = initializer {
                collect_statements(statements, &mut used);
                collect_bound(statements, &mut local);
            }
        }
        used.retain(|name| !local.contains(name));
        reads.entry(class.name.text).or_default().extend(used);
        // A method call is followed by name, into every method so named.
        for method in &class.methods {
            let mut used = HashSet::from([SKIP_LAMBDAS]);
            collect_statements(&method.body, &mut used);
            let mut bound = local.clone();
            collect_bound(&method.body, &mut bound);
            bound.extend(method.parameters.iter().map(|(name, _)| name.text));
            used.retain(|name| !bound.contains(name));
            reads.entry(method.name.text).or_default().extend(used);
        }
    }
    let later = |name: &str, declared: &HashSet<&str>| {
        !declared.contains(name) && globals.iter().any(|global| global.name == name)
    };
    let mut declared: HashSet<&str> = HashSet::new();
    for statement in &program.body {
        let mut pending = HashSet::from([SKIP_LAMBDAS]);
        collect_statements(std::slice::from_ref(statement), &mut pending);
        let mut seen: HashSet<&str> = HashSet::new();
        let mut work: Vec<&str> = pending
            .into_iter()
            .filter(|name| reads.contains_key(name))
            .collect();
        while let Some(callee) = work.pop() {
            if !seen.insert(callee) {
                continue;
            }
            let body = &reads[callee];
            if body.iter().any(|name| later(name, &declared)) {
                return Err(Error {
                    offset: statement.offset(),
                    message: "This line uses a top-level variable that is declared further down, through a function or constructor it calls, so the variable would not have its value yet. Move the variable above this line.",
                });
            }
            work.extend(body.iter().copied().filter(|name| reads.contains_key(name)));
        }
        if let Statement::Binding(name, ..) = statement {
            declared.insert(name.text);
        }
    }
    Ok(())
}

/// The names of the local variables declared anywhere in these statements.
fn collect_bound<'a>(statements: &'a [Statement<'a>], bound: &mut HashSet<&'a str>) {
    for statement in statements {
        match statement {
            Statement::Binding(name, ..) => {
                bound.insert(name.text);
            }
            Statement::For(name, _, body) => {
                bound.insert(name.text);
                collect_bound(body, bound);
            }
            Statement::While(_, _, body) => collect_bound(body, bound),
            Statement::If(_, _, then, otherwise) => {
                collect_bound(then, bound);
                collect_bound(otherwise.as_deref().unwrap_or_default(), bound);
            }
            Statement::Try(_, body, catches, finally) => {
                collect_bound(body, bound);
                for catch in catches {
                    bound.insert(catch.name.text);
                    collect_bound(&catch.body, bound);
                }
                collect_bound(finally.as_deref().unwrap_or_default(), bound);
            }
            Statement::When(_, _, branches) => {
                for branch in branches {
                    collect_bound(&branch.body, bound);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn collect_statements<'a>(statements: &'a [Statement<'a>], used: &mut HashSet<&'a str>) {
    for statement in statements {
        match statement {
            Statement::Return(_, expr) => {
                if let Some(expr) = expr {
                    collect_expr(expr, used);
                }
            }
            Statement::Binding(_, expr, _, _) | Statement::Throw(_, expr) => {
                collect_expr(expr, used)
            }
            Statement::Try(_, body, catches, finally) => {
                collect_statements(body, used);
                for catch in catches {
                    collect_statements(&catch.body, used);
                }
                if let Some(finally) = finally {
                    collect_statements(finally, used);
                }
            }
            Statement::Assign(name, expr) => {
                used.insert(name.text);
                collect_expr(expr, used);
            }
            Statement::Call(name, args) => {
                used.insert(name.text);
                for arg in args {
                    collect_expr(arg, used);
                }
            }
            Statement::MethodCall(expr) => collect_expr(expr, used),
            Statement::For(_, values, body) => {
                collect_expr(values, used);
                collect_statements(body, used);
            }
            Statement::While(_, condition, body) => {
                collect_expr(condition, used);
                collect_statements(body, used);
            }
            Statement::If(_, condition, then, otherwise) => {
                collect_expr(condition, used);
                collect_statements(then, used);
                if let Some(otherwise) = otherwise {
                    collect_statements(otherwise, used);
                }
            }
            Statement::SetIndex(list, index, _, value) => {
                collect_expr(list, used);
                collect_expr(index, used);
                collect_expr(value, used);
            }
            Statement::SetField(receiver, _, _, value) => {
                collect_expr(receiver, used);
                collect_expr(value, used);
            }
            Statement::Break(_) | Statement::Continue(_) => {}
            Statement::When(_, subject, branches) => {
                if let Some(subject) = subject {
                    collect_expr(subject, used);
                }
                collect_branches(branches, used);
            }
        }
    }
}

fn collect_branches<'a>(branches: &'a [super::WhenBranch<'a>], used: &mut HashSet<&'a str>) {
    for branch in branches {
        for condition in &branch.conditions {
            match condition {
                super::WhenCondition::Value(expr) => collect_expr(expr, used),
                super::WhenCondition::In(start, end, _) => {
                    collect_expr(start, used);
                    collect_expr(end, used);
                }
                super::WhenCondition::Is(_) => {}
            }
        }
        collect_statements(&branch.body, used);
        if let Some(value) = &branch.value {
            collect_expr(value, used);
        }
    }
}

/// Collects the names used inside lambdas that appear in `statements`.
fn collect_lambdas<'a>(statements: &'a [Statement<'a>], used: &mut HashSet<&'a str>) {
    for_each_lambda(statements, &mut |lambda| {
        collect_statements(&lambda.body, used);
        if let Some(value) = &lambda.value {
            collect_expr(value, used);
        }
    });
}

/// Calls `visit` for each lambda written in `statements`, outside other lambdas.
fn for_each_lambda<'a>(statements: &'a [Statement<'a>], visit: &mut dyn FnMut(&'a Lambda<'a>)) {
    for statement in statements {
        lambda_statement(statement, visit);
    }
}

/// Whether a lambda in `statements` assigns a new value to the variable `name`.
pub(super) fn lambda_assigns(statements: &[Statement<'_>], name: &str) -> bool {
    let mut found = false;
    for_each_lambda(statements, &mut |lambda| {
        found |= assigns(&lambda.body, name)
            || lambda
                .value
                .as_ref()
                .is_some_and(|value| expr_assigns(value, name));
    });
    found
}

/// Whether `statements` (including nested blocks and lambdas) assign `name`.
fn assigns(statements: &[Statement<'_>], name: &str) -> bool {
    let mut names = HashSet::new();
    collect_statements(statements, &mut names);
    if !names.contains(name) {
        return false;
    }
    statements
        .iter()
        .any(|statement| statement_assigns(statement, name))
}

fn statement_assigns(statement: &Statement<'_>, name: &str) -> bool {
    match statement {
        Statement::Assign(target, value) => target.text == name || expr_assigns(value, name),
        Statement::For(_, values, body) | Statement::While(_, values, body) => {
            expr_assigns(values, name) || assigns(body, name)
        }
        Statement::If(_, condition, then, otherwise) => {
            expr_assigns(condition, name)
                || assigns(then, name)
                || otherwise.as_ref().is_some_and(|block| assigns(block, name))
        }
        Statement::Try(_, body, catches, finally) => {
            assigns(body, name)
                || catches.iter().any(|catch| assigns(&catch.body, name))
                || finally.as_ref().is_some_and(|block| assigns(block, name))
        }
        Statement::When(_, subject, branches) => {
            subject
                .as_ref()
                .is_some_and(|subject| expr_assigns(subject, name))
                || branches.iter().any(|branch| {
                    assigns(&branch.body, name)
                        || branch
                            .value
                            .as_ref()
                            .is_some_and(|value| expr_assigns(value, name))
                })
        }
        other => {
            let mut found = false;
            lambda_statement(other, &mut |lambda| {
                found |= assigns(&lambda.body, name)
                    || lambda
                        .value
                        .as_ref()
                        .is_some_and(|value| expr_assigns(value, name));
            });
            found
        }
    }
}

fn expr_assigns(expr: &Expr<'_>, name: &str) -> bool {
    if let Expr::When(_, _, branches) = expr
        && branches.iter().any(|branch| {
            assigns(&branch.body, name)
                || branch
                    .value
                    .as_ref()
                    .is_some_and(|value| expr_assigns(value, name))
        })
    {
        return true;
    }
    let mut found = false;
    lambda_expr(expr, &mut |lambda| {
        found |= assigns(&lambda.body, name)
            || lambda
                .value
                .as_ref()
                .is_some_and(|value| expr_assigns(value, name));
    });
    found
}

fn lambda_statement<'a>(statement: &'a Statement<'a>, used: &mut dyn FnMut(&'a Lambda<'a>)) {
    let mut exprs: Vec<&'a Expr<'a>> = Vec::new();
    let mut blocks: Vec<&'a [Statement<'a>]> = Vec::new();
    match statement {
        Statement::Return(_, Some(expr))
        | Statement::Binding(_, expr, _, _)
        | Statement::Assign(_, expr)
        | Statement::Throw(_, expr)
        | Statement::MethodCall(expr) => exprs.push(expr),
        Statement::Call(_, args) => exprs.extend(args.iter()),
        Statement::For(_, values, body) => {
            exprs.push(values);
            blocks.push(body);
        }
        Statement::While(_, condition, body) => {
            exprs.push(condition);
            blocks.push(body);
        }
        Statement::If(_, condition, then, otherwise) => {
            exprs.push(condition);
            blocks.push(then);
            if let Some(otherwise) = otherwise {
                blocks.push(otherwise);
            }
        }
        Statement::SetIndex(a, b, _, c) => exprs.extend([a, b, c]),
        Statement::SetField(a, _, _, b) => exprs.extend([a, b]),
        Statement::Try(_, body, catches, finally) => {
            blocks.push(body);
            blocks.extend(catches.iter().map(|catch| catch.body.as_slice()));
            if let Some(finally) = finally {
                blocks.push(finally);
            }
        }
        Statement::Return(_, None) | Statement::Break(_) | Statement::Continue(_) => {}
        Statement::When(_, subject, branches) => {
            exprs.extend(subject.iter());
            for branch in branches {
                blocks.push(&branch.body);
                exprs.extend(branch.value.iter().map(|value| value.as_ref()));
                for condition in &branch.conditions {
                    match condition {
                        super::WhenCondition::Value(expr) => exprs.push(expr),
                        super::WhenCondition::In(start, end, _) => exprs.extend([start, end]),
                        super::WhenCondition::Is(_) => {}
                    }
                }
            }
        }
    }
    for block in blocks {
        for statement in block {
            lambda_statement(statement, used);
        }
    }
    for expr in exprs {
        lambda_expr(expr, used);
    }
}

fn lambda_expr<'a>(expr: &'a Expr<'a>, used: &mut dyn FnMut(&'a Lambda<'a>)) {
    match expr {
        Expr::When(_, subject, branches) => {
            if let Some(subject) = subject {
                lambda_expr(subject, used);
            }
            for branch in branches {
                for statement in &branch.body {
                    lambda_statement(statement, used);
                }
                if let Some(value) = &branch.value {
                    lambda_expr(value, used);
                }
            }
        }
        Expr::Safe(inner) => lambda_expr(inner, used),
        Expr::Elvis(left, right) => {
            lambda_expr(left, used);
            lambda_expr(right, used);
        }
        Expr::Lambda(lambda) => used(lambda),
        Expr::List(_, values) | Expr::Logic(_, values) | Expr::Call(_, values) => {
            for value in values {
                lambda_expr(value, used);
            }
        }
        Expr::MethodCall(receiver, _, args) => {
            lambda_expr(receiver, used);
            for arg in args {
                lambda_expr(arg, used);
            }
        }
        Expr::Index(left, right) | Expr::Compare(left, _, right) | Expr::Range(left, right, _) => {
            lambda_expr(left, used);
            lambda_expr(right, used);
        }
        Expr::Not(_, _, inner)
        | Expr::Negate(_, inner)
        | Expr::Field(inner, _)
        | Expr::Cast(inner, _, _)
        | Expr::Is(inner, _) => lambda_expr(inner, used),
        Expr::Arithmetic(left, rest) => {
            lambda_expr(left, used);
            for (_, right) in rest {
                lambda_expr(right, used);
            }
        }
        Expr::Text(_, parts) => {
            for part in parts {
                if let Part::Value(value) = part {
                    lambda_expr(value, used);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn collect_expr<'a>(expr: &'a Expr<'a>, used: &mut HashSet<&'a str>) {
    match expr {
        Expr::Named(_, value) => collect_expr(value, used),
        Expr::Progression(start, end, _, _, step) => {
            collect_expr(start, used);
            collect_expr(end, used);
            if let Some(step) = step {
                collect_expr(step, used);
            }
        }
        Expr::InRange(value, range, _) => {
            collect_expr(value, used);
            collect_expr(range, used);
        }
        Expr::Jump(statement) => collect_statements(std::slice::from_ref(statement), used),
        Expr::Temp(..) => {}
        Expr::StaticHelper(_, _, args) => {
            for arg in args {
                collect_expr(arg, used);
            }
        }
        Expr::When(_, subject, branches) => {
            if let Some(subject) = subject {
                collect_expr(subject, used);
            }
            collect_branches(branches, used);
        }
        Expr::Safe(inner) => collect_expr(inner, used),
        Expr::Elvis(left, right) => {
            collect_expr(left, used);
            collect_expr(right, used);
        }
        Expr::Lambda(lambda) => {
            if used.contains(SKIP_LAMBDAS) {
                return;
            }
            collect_statements(&lambda.body, used);
            if let Some(value) = &lambda.value {
                collect_expr(value, used);
            }
        }
        Expr::Variable(name) => {
            used.insert(name.text);
        }
        Expr::List(_, values) | Expr::Logic(_, values) => {
            for value in values {
                collect_expr(value, used);
            }
        }
        Expr::Call(name, args) => {
            if used.contains(SKIP_LAMBDAS) || used.contains(RECORD_CALLS) {
                used.insert(name.text);
            }
            for arg in args {
                collect_expr(arg, used);
            }
        }
        Expr::MethodCall(receiver, name, args) => {
            if used.contains(SKIP_LAMBDAS) {
                used.insert(name.text.split('<').next().unwrap_or(name.text));
            }
            collect_expr(receiver, used);
            for arg in args {
                collect_expr(arg, used);
            }
        }
        Expr::Index(left, right) | Expr::Compare(left, _, right) | Expr::Range(left, right, _) => {
            collect_expr(left, used);
            collect_expr(right, used);
        }
        Expr::Not(_, _, inner)
        | Expr::Negate(_, inner)
        | Expr::Field(inner, _)
        | Expr::Cast(inner, _, _)
        | Expr::Is(inner, _) => collect_expr(inner, used),
        Expr::Arithmetic(left, rest) => {
            collect_expr(left, used);
            for (_, right) in rest {
                collect_expr(right, used);
            }
        }
        Expr::Text(_, parts) => {
            for part in parts {
                if let Part::Value(value) = part {
                    collect_expr(value, used);
                }
            }
        }
        Expr::Null(_)
        | Expr::Int(..)
        | Expr::Long(..)
        | Expr::Char(..)
        | Expr::Double(..)
        | Expr::Bool(..) => {}
    }
}

impl<'a> Symbols<'a> {
    pub(super) fn global(&self, name: &str) -> Option<&Global<'a>> {
        self.globals.iter().find(|global| global.name == name)
    }
}

impl<'a> Emitter<'a> {
    fn global_member(&mut self, global: &Global<'a>) -> u16 {
        let name = global.name;
        if let Some(&index) = self.global_members.get(name) {
            return index;
        }
        let class = *self
            .main_class
            .get_or_insert_with(|| self.pool.class(self.symbols.types.entry_class()));
        let descriptor = global.kind.descriptor(&self.symbols.types);
        let index = self.pool.member(9, class, name, &descriptor);
        self.global_members.insert(name, index);
        index
    }

    /// Loads a top-level variable. Returns `None` when `name` is not one.
    pub(super) fn load_global(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        base: u16,
    ) -> Option<Kind> {
        let symbols = self.symbols;
        let global = symbols.global(name.text)?;
        let reference = self.global_member(global);
        instruction(&mut method.code, 0xb2, reference); // getstatic
        method.max_stack = method.max_stack.max(base + global.kind.slots());
        Some(global.kind)
    }

    /// Stores the value on top of the stack in a top-level variable.
    pub(super) fn store_global(&mut self, method: &mut Method<'a>, name: &str) {
        let symbols = self.symbols;
        let global = symbols.global(name).expect("checked by the caller");
        let reference = self.global_member(global);
        instruction(&mut method.code, 0xb3, reference); // putstatic
    }

    /// For a top-level declaration that functions use, stores the value on top
    /// of the stack in its static field. A `let` keeps a copy in a local
    /// variable so null checks in the main code can narrow it.
    /// Returns true when no local variable is needed.
    pub(super) fn declare_global(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        kind: Kind,
    ) -> bool {
        let symbols = self.symbols;
        let Some(global) = symbols
            .globals
            .iter()
            .find(|global| global.offset == name.offset)
        else {
            return false;
        };
        if !global.mutable {
            method
                .code
                .push(if kind.slots() == 2 { 0x5c } else { 0x59 }); // dup2 or dup
            method.max_stack = method.max_stack.max(2 * kind.slots());
        }
        let reference = self.global_member(global);
        instruction(&mut method.code, 0xb3, reference); // putstatic
        global.mutable
    }
}
