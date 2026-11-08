//! `try`, `catch`, `finally`, and `throw`.
//!
//! A `finally` block is copied onto every way out of its `try`: falling off
//! the end, each `catch`, `return`, `break`, `continue`, and a catch-all
//! handler that runs it and rethrows. The copies are left out of the ranges
//! the `try` protects, so an exception inside a `finally` is not caught by
//! the same `try`.

use super::{
    Catch, Emitter, Error, Expr, Kind, Local, Method, Name, Statement, at, instruction,
    resolve_type,
};

#[derive(Clone)]
pub(super) struct TryContext<'a> {
    finally: Option<&'a [Statement<'a>]>,
    /// Completed protected code ranges.
    ranges: Vec<(usize, usize)>,
    /// Start of the protected range currently being emitted.
    open: Option<usize>,
    /// The number of enclosing loops when the `try` started.
    loop_depth: usize,
}

impl TryContext<'_> {
    pub(super) fn has_finally(&self) -> bool {
        self.finally.is_some()
    }

    fn close(&mut self, at: usize) {
        if let Some(start) = self.open.take()
            && start < at
        {
            self.ranges.push((start, at));
        }
    }
}

impl<'a> Emitter<'a> {
    /// Runs the `finally` blocks of the innermost `try` statements, down to
    /// `depth`, before a jump out of them. Returns the contexts, which the
    /// caller passes to `resume_tries` after emitting the jump.
    pub(super) fn exit_tries(
        &mut self,
        method: &mut Method<'a>,
        depth: usize,
    ) -> Result<Vec<TryContext<'a>>, Error> {
        let mut exiting = Vec::new();
        while method.tries.len() > depth {
            let mut context = method.tries.pop().unwrap();
            context.close(method.code.len());
            if let Some(finally) = context.finally {
                self.finally_block(method, finally)?;
            }
            exiting.push(context);
        }
        Ok(exiting)
    }

    pub(super) fn resume_tries(&mut self, method: &mut Method<'a>, exiting: Vec<TryContext<'a>>) {
        let at = method.code.len();
        for mut context in exiting.into_iter().rev() {
            context.open = Some(at);
            method.tries.push(context);
        }
    }

    /// The number of `try` statements a `break` or `continue` stays inside.
    pub(super) fn loop_try_depth(&self, method: &Method<'a>) -> usize {
        let loops = method.loops.len();
        method
            .tries
            .iter()
            .take_while(|context| context.loop_depth < loops)
            .count()
    }

    fn finally_block(
        &mut self,
        method: &mut Method<'a>,
        finally: &'a [Statement<'a>],
    ) -> Result<(), Error> {
        if !self.scoped_block(method, finally, &[])? {
            return Err(Error {
                offset: finally.last().map_or(0, Statement::offset),
                message: "A finally block cannot return, break, continue, or throw.",
            });
        }
        Ok(())
    }

    fn throwable_class(&mut self, name: Name<'a>) -> Result<(Kind, u16), Error> {
        let kind = resolve_type(&self.type_scope, name)?;
        if !self.is_throwable(kind) {
            return Err(at(name, "Only exception types can be caught."));
        }
        let class = self.object_class(kind);
        Ok((kind, class))
    }

    /// Whether values of `kind` can be thrown and caught.
    fn is_throwable(&self, kind: Kind) -> bool {
        let types = &self.symbols.types;
        match kind {
            Kind::Object(class) => self
                .symbols
                .java_root(class)
                .is_some_and(|root| types.is_java_subclass(&root, "java/lang/Throwable")),
            other => types
                .runtime_class_name(other)
                .is_some_and(|binary| types.is_java_subclass(&binary, "java/lang/Throwable")),
        }
    }

    pub(super) fn try_statement(
        &mut self,
        method: &mut Method<'a>,
        keyword: Name<'a>,
        body: &'a [Statement<'a>],
        catches: &'a [Catch<'a>],
        finally: Option<&'a [Statement<'a>]>,
    ) -> Result<bool, Error> {
        let live = method.locals.len();
        let start = method.code.len();
        method.tries.push(TryContext {
            finally,
            ranges: Vec::new(),
            open: Some(start),
            loop_depth: method.loops.len(),
        });
        let mut exits = Vec::new();
        let mut continues = self.scoped_block(method, body, &[])?;
        let body_end = method.code.len();
        method.tries.last_mut().unwrap().close(body_end);
        let body_ranges = method.tries.last().unwrap().ranges.clone();
        if continues {
            if let Some(finally) = finally {
                self.finally_block(method, finally)?;
            }
            exits.push(method.jump());
        }
        let mut entries = Vec::new();
        for catch in catches {
            let (kind, class) = match catch.kind {
                Some(name) => self.throwable_class(name)?,
                None => {
                    let kind = self.symbols.types.java_kind("java/lang/Exception");
                    (kind, self.object_class(kind))
                }
            };
            let handler = method.code.len();
            for &(from, to) in &body_ranges {
                entries.push([from, to, handler, class as usize]);
            }
            method.stack.push(Local::Object(class));
            method.frame(live);
            method.stack.pop();
            method.max_stack = method.max_stack.max(1);
            method.tries.last_mut().unwrap().open = Some(handler);
            let slot = method.local(Local::Object(class));
            method.access(0x3a, slot); // astore
            method.variables.push(super::Binding {
                name: catch.name.text,
                slot,
                kind,
                mutable: false,
            });
            let catch_continues = self.scoped_block(method, &catch.body, &[])?;
            method.pop_variable();
            method.locals.truncate(live);
            method.tries.last_mut().unwrap().close(method.code.len());
            if catch_continues {
                if let Some(finally) = finally {
                    self.finally_block(method, finally)?;
                }
                exits.push(method.jump());
            }
            continues |= catch_continues;
        }
        let context = method.tries.pop().unwrap();
        if let Some(finally) = finally {
            let handler = method.code.len();
            let throwable = self.symbols.types.java_kind("java/lang/Throwable");
            let class = self.object_class(throwable);
            for &(from, to) in &context.ranges {
                entries.push([from, to, handler, 0]);
            }
            method.stack.push(Local::Object(class));
            method.frame(live);
            method.stack.pop();
            let slot = method.local(Local::Object(class));
            method.access(0x3a, slot); // astore
            self.finally_block(method, finally)?;
            method.access(0x19, slot); // aload
            method.code.push(0xbf); // athrow
            method.locals.truncate(live);
            method.max_stack = method.max_stack.max(1);
        }
        for entry in entries {
            let [from, to, handler, class] = entry;
            method
                .exceptions
                .push([from as u16, to as u16, handler as u16, class as u16]);
        }
        if !exits.is_empty() {
            method.patch_branches(&exits, keyword.offset)?;
            method.frame(live);
        }
        Ok(continues)
    }

    pub(super) fn throw(
        &mut self,
        method: &mut Method<'a>,
        keyword: Name<'a>,
        value: &'a Expr<'a>,
    ) -> Result<(), Error> {
        let kind = self.expression(method, value, 0)?;
        if !self.is_throwable(kind) {
            return Err(at(
                keyword,
                "Only exceptions can be thrown, such as IllegalArgumentException(\"message\").",
            ));
        }
        method.code.push(0xbf); // athrow
        Ok(())
    }

    /// `error(message)` throws an `IllegalStateException` with the message.
    pub(super) fn error_call(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
    ) -> Result<(), Error> {
        let [message] = args else {
            return Err(at(name, "error expects one message."));
        };
        let class = self.runtime_class("java/lang/IllegalStateException");
        let init = self.runtime_member(
            "java/lang/IllegalStateException",
            "<init>",
            "(Ljava/lang/String;)V",
            10,
        );
        let uninitialized = Local::Uninitialized(method.code.len() as u16);
        instruction(&mut method.code, 0xbb, class); // new
        method.code.push(0x59); // dup
        method.stack.extend([uninitialized, uninitialized]);
        let kind = self.expression(method, message, 2)?;
        method.stack.clear();
        self.symbols.require(message, kind, Kind::Text)?;
        method.max_stack = method.max_stack.max(3);
        instruction(&mut method.code, 0xb7, init); // invokespecial
        method.code.push(0xbf); // athrow
        Ok(())
    }
}
