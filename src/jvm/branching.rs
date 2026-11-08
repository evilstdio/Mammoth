//! `when`, `if` used as a value, safe calls (`?.`), and the elvis operator (`?:`).

use super::{
    Comparison, Emitter, Error, Expr, Kind, Method, Name, WhenBranch, WhenCondition, at,
    instruction, resolve_type,
};

/// The evaluated subject of a `when`.
#[derive(Clone, Copy)]
struct Subject {
    slot: u16,
    kind: Kind,
    /// The slot of the read-only variable written as the subject, which
    /// `is` branches can narrow.
    variable: Option<u16>,
}

impl<'a> Emitter<'a> {
    fn when_subject(
        &mut self,
        method: &mut Method<'a>,
        subject: Option<&'a Expr<'a>>,
    ) -> Result<Option<Subject>, Error> {
        let Some(expr) = subject else {
            return Ok(None);
        };
        let (expr, named) = match expr {
            Expr::Named(label, value) => (&**value, Some(*label)),
            other => (other, None),
        };
        let base = method.stack.len() as u16;
        let kind = self.expression(method, expr, base)?;
        if kind == Kind::Null {
            return Err(Error {
                offset: expr.offset(),
                message: "A when subject needs a value with a known type.",
            });
        }
        let variable = match expr {
            Expr::Variable(name) => method
                .binding(*name)
                .filter(|binding| !binding.mutable)
                .map(|binding| binding.slot),
            _ => None,
        };
        let slot = method.local(self.local_type(kind));
        method.access(kind.store(), slot);
        let variable = if let Some(label) = named {
            method.variables.push(super::Binding {
                name: label.text,
                slot,
                kind,
                mutable: false,
            });
            Some(slot)
        } else {
            variable
        };
        Ok(Some(Subject {
            slot,
            kind,
            variable,
        }))
    }

    fn load_subject(&mut self, method: &mut Method<'a>, subject: Subject) {
        method.access(subject.kind.load(), subject.slot);
        let depth = method.stack.len() as u16;
        method.max_stack = method.max_stack.max(depth + subject.kind.slots());
    }

    /// Emits a branch condition and returns the jumps taken when it equals `when`.
    fn when_test(
        &mut self,
        method: &mut Method<'a>,
        subject: Option<Subject>,
        condition: &'a WhenCondition<'a>,
        when: bool,
    ) -> Result<Vec<usize>, Error> {
        let Some(subject) = subject else {
            let WhenCondition::Value(expr) = condition else {
                unreachable!("the parser only allows values without a subject")
            };
            return self.branch_when(method, expr, when);
        };
        let base = method.stack.len() as u16;
        match condition {
            WhenCondition::Not(inner) => self.when_test(method, Some(subject), inner, !when),
            WhenCondition::Value(Expr::Null(_)) => {
                if !subject.kind.is_reference() {
                    return Err(Error {
                        offset: 0,
                        message: "This value can never be null.",
                    });
                }
                self.load_subject(method, subject);
                let branch = method.code.len();
                instruction(&mut method.code, if when { 0xc6 } else { 0xc7 }, 0); // ifnull / ifnonnull
                Ok(vec![branch])
            }
            WhenCondition::Value(expr) => {
                self.load_subject(method, subject);
                let local = self.local_type(subject.kind);
                method.push_stack(local);
                let kind = self.expression(method, expr, base + subject.kind.slots())?;
                method.pop_stack();
                let (yes, no) = self.compare_kinds(
                    method,
                    expr,
                    subject.kind,
                    expr,
                    kind,
                    Comparison::Eq,
                    base,
                )?;
                let branch = method.code.len();
                instruction(&mut method.code, if when { yes } else { no }, 0);
                Ok(vec![branch])
            }
            WhenCondition::Is(annotation) => {
                if !subject.kind.is_reference() {
                    return Err(at(
                        *annotation,
                        "Use is with values whose type is not known exactly, such as Any.",
                    ));
                }
                let target = resolve_type(&self.type_scope, *annotation)?.non_null();
                self.symbols
                    .check_runtime_target(target, subject.kind, *annotation)?;
                self.load_subject(method, subject);
                let class = self.instance_class(target);
                instruction(&mut method.code, 0xc1, class); // instanceof
                let branch = method.code.len();
                instruction(&mut method.code, if when { 0x9a } else { 0x99 }, 0);
                Ok(vec![branch])
            }
            WhenCondition::In(start, end, inclusive) => {
                let upper = if *inclusive {
                    Comparison::Le
                } else {
                    Comparison::Lt
                };
                // subject >= start
                self.load_subject(method, subject);
                let local = self.local_type(subject.kind);
                method.push_stack(local);
                let low = self.expression(method, start, base + subject.kind.slots())?;
                method.pop_stack();
                let (_, below) = self.compare_kinds(
                    method,
                    start,
                    subject.kind,
                    start,
                    low,
                    Comparison::Ge,
                    base,
                )?;
                let first = method.code.len();
                instruction(&mut method.code, below, 0);
                // subject <= end (or < end)
                self.load_subject(method, subject);
                method.push_stack(local);
                let high = self.expression(method, end, base + subject.kind.slots())?;
                method.pop_stack();
                let (inside, outside) =
                    self.compare_kinds(method, end, subject.kind, end, high, upper, base)?;
                let second = method.code.len();
                instruction(&mut method.code, if when { inside } else { outside }, 0);
                if when {
                    // Below the range: fall through to the code after this test.
                    method.patch_branch(first, method.code.len(), start.offset())?;
                    method.frame(method.locals.len());
                    Ok(vec![second])
                } else {
                    Ok(vec![first, second])
                }
            }
        }
    }

    /// Emits the conditions of a branch, falling through when it matches and
    /// returning the jumps taken when it does not.
    fn branch_conditions(
        &mut self,
        method: &mut Method<'a>,
        subject: Option<Subject>,
        branch: &'a WhenBranch<'a>,
        offset: usize,
    ) -> Result<Vec<usize>, Error> {
        let Some((last, others)) = branch.conditions.split_last() else {
            return Ok(Vec::new());
        };
        let mut matched = Vec::new();
        for condition in others {
            matched.extend(self.when_test(method, subject, condition, true)?);
        }
        let next = self.when_test(method, subject, last, false)?;
        if !matched.is_empty() {
            method.patch_branches(&matched, offset)?;
            method.frame(method.locals.len());
        }
        Ok(next)
    }

    /// Facts a branch may rely on: (non-null slots, narrowed types).
    fn branch_facts(
        &self,
        method: &Method<'a>,
        subject: Option<Subject>,
        branches: &'a [WhenBranch<'a>],
        index: usize,
    ) -> (Vec<u16>, Vec<(u16, Kind)>) {
        let branch = &branches[index];
        match (subject, branch.conditions.as_slice()) {
            (Some(subject), [WhenCondition::Is(annotation)]) => {
                let facts = subject
                    .variable
                    .and_then(|slot| {
                        let kind = resolve_type(&self.type_scope, *annotation).ok()?;
                        Some(vec![(slot, kind.non_null())])
                    })
                    .unwrap_or_default();
                (Vec::new(), facts)
            }
            (None, [WhenCondition::Value(condition)]) => (
                method.non_null_facts(condition, true),
                self.type_facts(method, condition, true),
            ),
            (None, []) if index == 1 => match branches[0].conditions.as_slice() {
                [WhenCondition::Value(condition)] => (
                    method.non_null_facts(condition, false),
                    self.type_facts(method, condition, false),
                ),
                _ => (Vec::new(), Vec::new()),
            },
            _ => (Vec::new(), Vec::new()),
        }
    }

    /// A `when` statement. Returns whether code after it can run.
    pub(super) fn when_statement(
        &mut self,
        method: &mut Method<'a>,
        keyword: Name<'a>,
        subject: Option<&'a Expr<'a>>,
        branches: &'a [WhenBranch<'a>],
    ) -> Result<bool, Error> {
        let locals = method.locals.len();
        let subject = self.when_subject(method, subject)?;
        let mut ends = Vec::new();
        let mut has_else = false;
        for (index, branch) in branches.iter().enumerate() {
            has_else |= branch.conditions.is_empty();
            let next = self.branch_conditions(method, subject, branch, keyword.offset)?;
            let (non_null, types) = self.branch_facts(method, subject, branches, index);
            let type_count = method.type_facts.len();
            method.type_facts.extend(types);
            let continues = self.scoped_block(method, &branch.body, &non_null);
            method.type_facts.truncate(type_count);
            if continues? {
                ends.push(method.jump());
            }
            if !next.is_empty() {
                method.patch_branches(&next, keyword.offset)?;
                method.frame(method.locals.len());
            }
        }
        let covered =
            !has_else && subject.is_some_and(|subject| self.exhaustive(subject.kind, branches));
        if !has_else
            && !covered
            && subject.is_some_and(|subject| self.sealed_type(subject.kind.non_null()).is_some())
        {
            return Err(at(
                keyword,
                "This when must handle every subclass of the sealed type, or have an else branch.",
            ));
        }
        if covered {
            self.throw_unmatched(method, method.stack.len() as u16);
        }
        let falls_through = !ends.is_empty() || !(has_else || covered);
        if !ends.is_empty() {
            method.patch_branches(&ends, keyword.offset)?;
            method.frame(method.locals.len());
        }
        method.locals.truncate(locals);
        Ok(falls_through)
    }

    /// Reached only if a subclass or constant was added after compiling.
    fn throw_unmatched(&mut self, method: &mut Method<'a>, base: u16) {
        let error = self.pool_class("java/lang/IncompatibleClassChangeError");
        instruction(&mut method.code, 0xbb, error); // new
        method.code.push(0x59); // dup
        let init = self.pool.member_for(10, error, "<init>", "()V");
        instruction(&mut method.code, 0xb7, init); // invokespecial
        method.code.push(0xbf); // athrow
        method.max_stack = method.max_stack.max(base + 2);
    }

    /// Whether the branches cover every value of a subject of type `kind`.
    /// A nullable subject also needs a `null` branch.
    fn exhaustive(&self, kind: Kind, branches: &[WhenBranch<'_>]) -> bool {
        if matches!(kind, Kind::Nullable(_))
            && !branches
                .iter()
                .flat_map(|branch| &branch.conditions)
                .any(|condition| matches!(condition, WhenCondition::Value(Expr::Null(_))))
        {
            return false;
        }
        let kind = kind.non_null();
        self.covers_enum(kind, branches) || self.covers_sealed(kind, branches)
    }

    /// The class or interface of a sealed type, as `(is_interface, index)`.
    fn sealed_type(&self, kind: Kind) -> Option<(bool, usize)> {
        let symbols = self.symbols;
        if let Some(class) = symbols.types.native_class(kind) {
            return symbols.class_declarations[class]
                .is_sealed
                .then_some((false, class));
        }
        let interface = symbols.types.interface_root(kind)?;
        symbols.interface_declarations[interface]
            .is_sealed
            .then_some((true, interface))
    }

    fn covers_sealed(&self, kind: Kind, branches: &[WhenBranch<'_>]) -> bool {
        let Some(root) = self.sealed_type(kind) else {
            return false;
        };
        let types = &self.symbols.types;
        let named: Vec<(bool, usize)> = branches
            .iter()
            .flat_map(|branch| &branch.conditions)
            .filter_map(|condition| match condition {
                WhenCondition::Is(annotation) => resolve_type(&self.type_scope, *annotation).ok(),
                // An object is named without `is`.
                WhenCondition::Value(Expr::Variable(name)) => self
                    .type_scope
                    .names
                    .get(name.text)
                    .copied()
                    .filter(|kind| {
                        matches!(kind, Kind::Object(class)
                            if self.symbols.class_declarations[*class].is_object)
                    }),
                _ => None,
            })
            .filter_map(|kind| {
                let kind = kind.non_null();
                types
                    .native_class(kind)
                    .map(|class| (false, class))
                    .or_else(|| {
                        types
                            .interface_root(kind)
                            .map(|interface| (true, interface))
                    })
            })
            .collect();
        self.sealed_covered(root, &named)
    }

    fn sealed_covered(&self, target: (bool, usize), named: &[(bool, usize)]) -> bool {
        if named.contains(&target) {
            return true;
        }
        let symbols = self.symbols;
        let sealed = match target {
            (false, class) => symbols.class_declarations[class].is_sealed,
            (true, interface) => symbols.interface_declarations[interface].is_sealed,
        };
        if !sealed {
            return false;
        }
        let mut children = Vec::new();
        match target {
            (false, class) => {
                for (child, parent) in symbols.superclasses.iter().enumerate() {
                    if *parent == Some(class) {
                        children.push((false, child));
                    }
                }
            }
            (true, interface) => {
                let extends = |kinds: &[Kind]| {
                    kinds
                        .iter()
                        .any(|&kind| symbols.types.interface_root(kind) == Some(interface))
                };
                for (child, kinds) in symbols.implementations.iter().enumerate() {
                    if extends(kinds) {
                        children.push((false, child));
                    }
                }
                for (child, kinds) in symbols.interface_parents.iter().enumerate() {
                    if extends(kinds) {
                        children.push((true, child));
                    }
                }
            }
        }
        !children.is_empty()
            && children
                .iter()
                .all(|&child| self.sealed_covered(child, named))
    }

    /// Whether the branches name every constant of the enum type `kind`, a
    /// Mammoth enum or a Java enum, as `Color.RED` style values. A nullable
    /// subject is never covered, since null matches no constant.
    fn covers_enum(&self, kind: Kind, branches: &[WhenBranch<'_>]) -> bool {
        let constants: Vec<String> = match kind {
            Kind::Object(class) => match &self.symbols.enum_values[class] {
                Some(values) => values.iter().map(|value| (*value).to_owned()).collect(),
                None => return false,
            },
            Kind::Java(_) => {
                let Some(info) = self
                    .symbols
                    .types
                    .runtime_class_name(kind)
                    .and_then(|binary| self.symbols.types.java_class(&binary))
                else {
                    return false;
                };
                if info.access & 0x4000 == 0 {
                    return false;
                }
                info.fields
                    .iter()
                    .filter(|field| field.access & 0x4000 != 0)
                    .map(|field| field.name.clone())
                    .collect()
            }
            _ => return false,
        };
        let named: Vec<&str> = branches
            .iter()
            .flat_map(|branch| &branch.conditions)
            .filter_map(|condition| match condition {
                crate::syntax::WhenCondition::Value(Expr::Field(owner, constant)) => {
                    let Expr::Variable(type_name) = owner.as_ref() else {
                        return None;
                    };
                    resolve_type(&self.type_scope, *type_name)
                        .ok()
                        .filter(|owner| self.symbols.types.same_kind(*owner, kind))
                        .map(|_| constant.text)
                }
                _ => None,
            })
            .collect();
        !constants.is_empty()
            && constants
                .iter()
                .all(|constant| named.contains(&constant.as_str()))
    }

    /// A `when` (or `if`) used as a value. Returns `None` when its branches
    /// end in calls that give no value, which a lambda body allows.
    pub(super) fn when_value(
        &mut self,
        method: &mut Method<'a>,
        offset: usize,
        subject: Option<&'a Expr<'a>>,
        branches: &'a [WhenBranch<'a>],
        expected: Option<Kind>,
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let location = Name {
            text: "when",
            offset,
        };
        let has_else = branches.iter().any(|branch| branch.conditions.is_empty());
        let locals = method.locals.len();
        let subject = self.when_subject(method, subject)?;
        // Branches naming every constant of an enum subject need no else.
        if !has_else && !subject.is_some_and(|subject| self.exhaustive(subject.kind, branches)) {
            return Err(at(
                location,
                "A when used as a value needs an else branch, unless it covers every constant of an enum or every subclass of a sealed type.",
            ));
        }
        // `if c { 1 } else { null }` is an Int? even with no destination.
        let expected = match expected {
            None if branches
                .iter()
                .any(|branch| matches!(branch.value.as_deref(), Some(Expr::Null(_)))) =>
            {
                self.branch_type_with_null(method, branches)
            }
            other => other,
        };
        let mut result: Option<Option<Kind>> = expected.map(Some);
        let mut ends = Vec::new();
        for (index, branch) in branches.iter().enumerate() {
            let next = self.branch_conditions(method, subject, branch, offset)?;
            let (non_null, types) = self.branch_facts(method, subject, branches, index);
            let variables = method.variables.len();
            let branch_locals = method.locals.len();
            let facts = method.non_null_slots.len();
            let type_count = method.type_facts.len();
            method.non_null_slots.extend(non_null);
            method.type_facts.extend(types);
            let continues = self.block(method, &branch.body);
            let produced = match continues {
                Ok(true) => match &branch.value {
                    None => Err(at(
                        location,
                        "Each branch of this when must end with a value.",
                    )),
                    Some(value) if matches!(value.as_ref(), Expr::Jump(_)) => {
                        let Expr::Jump(statement) = value.as_ref() else {
                            unreachable!()
                        };
                        self.jump_value(method, statement, offset).map(|()| None)
                    }
                    Some(value) => match result {
                        Some(Some(kind)) => self
                            .expression_as(method, value, base, Some(kind))
                            .and_then(|actual| {
                                self.coerce(method, value, actual, kind, base)
                                    .map(|()| Some(()))
                            }),
                        Some(None) => self.value_or_nothing(method, value).map(|kind| {
                            if let Some(kind) = kind {
                                method.code.push(kind.pop_opcode());
                            }
                            Some(())
                        }),
                        None => self.value_or_nothing(method, value).and_then(|kind| {
                            if kind == Some(Kind::Null) {
                                return Err(at(
                                    location,
                                    "Give this value a type, such as `let name: Text? = ...`.",
                                ));
                            }
                            result = Some(kind);
                            Ok(Some(()))
                        }),
                    },
                },
                Ok(false) => Ok(None),
                Err(error) => Err(error),
            };
            method.non_null_slots.truncate(facts);
            method.type_facts.truncate(type_count);
            method.truncate_variables(variables);
            method.locals.truncate(branch_locals);
            self.sync_locals(method);
            if produced?.is_some() {
                ends.push(method.jump());
            }
            if !next.is_empty() {
                method.patch_branches(&next, offset)?;
                method.frame(method.locals.len());
            }
        }
        if !has_else {
            self.throw_unmatched(method, base);
        }
        method.locals.truncate(locals);
        let result = result.flatten();
        if !ends.is_empty() {
            method.patch_branches(&ends, offset)?;
            if let Some(kind) = result {
                let local = self.local_type(kind);
                method.push_stack(local);
                method.frame(method.locals.len());
                method.pop_stack();
                method.max_stack = method.max_stack.max(base + kind.slots());
            } else {
                method.frame(method.locals.len());
            }
        }
        Ok(result)
    }

    /// `receiver?.member` produces a nullable value when the receiver is null.
    fn safe_member(
        &mut self,
        method: &mut Method<'a>,
        receiver: Kind,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        if let Some(result) = self.generic_method(
            method,
            super::generic_methods::MemberCall {
                receiver,
                name,
                args,
                base,
                expected,
                special: false,
            },
        )? {
            return Ok(result);
        }
        self.call_method_on_expected(method, receiver, name, args, base, true, expected)
    }

    pub(super) fn safe_access_expected(
        &mut self,
        method: &mut Method<'a>,
        inner: &'a Expr<'a>,
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let receiver = match inner {
            Expr::MethodCall(receiver, ..) | Expr::Field(receiver, _) => receiver,
            _ => unreachable!("the parser only wraps member access"),
        };
        self.allow_null = true;
        let kind = self.expression(method, receiver, base)?;
        if !kind.is_reference() || kind == Kind::Null {
            return Err(Error {
                offset: receiver.offset(),
                message: "Use ?. only on a value that can be null.",
            });
        }
        let receiver_local = self.local_type(kind);
        method.code.push(0x59); // dup
        method.max_stack = method.max_stack.max(base + 2);
        let null_jump = method.code.len();
        instruction(&mut method.code, 0xc6, 0); // ifnull
        let present_kind = kind.non_null();
        if !present_kind.is_reference() {
            self.unbox_element(method, present_kind, base);
        }
        let live = method.locals.len();
        let result = match inner {
            Expr::MethodCall(_, name, args) => {
                let expected = expected.map(Kind::non_null);
                let attempt = method.snapshot();
                match self.safe_member(method, present_kind, *name, args, base, expected) {
                    Ok(result) => result,
                    Err(error) => {
                        method.restore(attempt);
                        self.extension_on_stack(method, present_kind, *name, args, base, expected)
                            .map_err(|_| error)?
                            .ok_or(error)?
                    }
                }
            }
            Expr::Field(_, name) => Some(self.field_on(method, present_kind, *name, base, true)?),
            _ => unreachable!(),
        };
        // The null path joins here without any local this call made.
        method.locals.truncate(live);
        let nullable = result.map(|kind| kind.nullable().unwrap_or(kind));
        if let Some(kind) = result
            && !kind.is_reference()
        {
            self.box_value(method, kind);
        }
        let offset = inner.offset();
        let end = method.jump();
        method.patch_branch(null_jump, method.code.len(), offset)?;
        method.push_stack(receiver_local);
        method.frame(method.locals.len());
        method.pop_stack();
        method.code.push(0x57); // discard null receiver
        if result.is_some() {
            method.code.push(0x01); // aconst_null
        }
        method.patch_branch(end, method.code.len(), offset)?;
        if let Some(kind) = nullable {
            let local = self.local_type(kind);
            method.push_stack(local);
            method.frame(method.locals.len());
            method.pop_stack();
        } else {
            method.frame(method.locals.len());
        }
        Ok(nullable)
    }

    pub(super) fn safe_access(
        &mut self,
        method: &mut Method<'a>,
        inner: &'a Expr<'a>,
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        self.safe_access_expected(method, inner, base, None)
    }

    /// The fallback of an elvis expression is evaluated only for a null value.
    /// The nullable type of the first branch whose value is not `null`.
    fn branch_type_with_null(
        &mut self,
        method: &mut Method<'a>,
        branches: &'a [WhenBranch<'a>],
    ) -> Option<Kind> {
        for branch in branches {
            let Some(value) = branch.value.as_deref() else {
                continue;
            };
            if matches!(value, Expr::Null(_) | Expr::Jump(_)) {
                continue;
            }
            let saved = method.snapshot();
            let variables = method.variables.len();
            let base = method.stack.len() as u16;
            let kind = self
                .block(method, &branch.body)
                .and_then(|_| self.expression(method, value, base));
            method.truncate_variables(variables);
            method.restore(saved);
            return kind.ok().and_then(Kind::nullable);
        }
        None
    }

    /// Emits a `return`, `throw`, `break` or `continue` used as a value.
    pub(super) fn jump_value(
        &mut self,
        method: &mut Method<'a>,
        statement: &'a crate::syntax::Statement<'a>,
        offset: usize,
    ) -> Result<(), Error> {
        use crate::syntax::Statement;
        if matches!(statement, Statement::Break(_) | Statement::Continue(_))
            && !method.stack.is_empty()
        {
            return Err(Error {
                offset,
                message: "break and continue cannot leave the middle of a call. Move this to its own line.",
            });
        }
        // The jump leaves operands behind, which return and throw discard.
        let operands = std::mem::take(&mut method.stack);
        let live = method.locals.len();
        let result = self.block(method, std::slice::from_ref(statement));
        method.locals.truncate(live);
        method.stack = operands;
        result.map(|_| ())
    }

    pub(super) fn elvis(
        &mut self,
        method: &mut Method<'a>,
        value: &'a Expr<'a>,
        fallback: &'a Expr<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        self.allow_null = true;
        let kind = self.expression(method, value, base)?;
        if !kind.is_reference() || kind == Kind::Null {
            return Err(Error {
                offset: value.offset(),
                message: "The left side of ?: must be a value that can be null.",
            });
        }
        let target = kind.non_null();
        method.code.push(0x59);
        method.max_stack = method.max_stack.max(base + 2);
        let present = method.code.len();
        instruction(&mut method.code, 0xc7, 0); // ifnonnull
        method.code.push(0x57);
        if let Expr::Jump(statement) = fallback {
            // Only a present value continues past this point.
            self.jump_value(method, statement, value.offset())?;
            method.patch_branch(present, method.code.len(), value.offset())?;
            method.push_stack(self.local_type(kind));
            method.frame(method.locals.len());
            method.pop_stack();
            if !target.is_reference() {
                self.unbox_element(method, target, base);
            }
            return Ok(target);
        }
        let actual = self.expression_as(method, fallback, base, Some(target))?;
        let result = if actual == Kind::Null || actual == kind {
            kind
        } else if actual.non_null() == target && matches!(actual, Kind::Nullable(_)) {
            actual
        } else {
            self.coerce(method, fallback, actual, target, base)?;
            target
        };
        let end = method.jump();
        method.patch_branch(present, method.code.len(), value.offset())?;
        let original = self.local_type(kind);
        method.push_stack(original);
        method.frame(method.locals.len());
        method.pop_stack();
        if !result.is_reference() {
            self.unbox_element(method, result, base);
        }
        method.patch_branch(end, method.code.len(), value.offset())?;
        let local = self.local_type(result);
        method.push_stack(local);
        method.frame(method.locals.len());
        method.pop_stack();
        Ok(result)
    }
}
