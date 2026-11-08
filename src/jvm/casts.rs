//! `as`, `as?`, and the conversions used by `is` smart casts.

use super::{Emitter, Error, Expr, Kind, Method, Name, at, instruction, resolve_type};

impl<'a> Emitter<'a> {
    /// The class `checkcast` and `instanceof` use for a type. Primitive types
    /// use their boxed class, since only boxed values can be checked.
    pub(super) fn instance_class(&mut self, kind: Kind) -> u16 {
        match kind {
            Kind::Int => self.runtime_class("java/lang/Integer"),
            Kind::Long => self.runtime_class("java/lang/Long"),
            Kind::Double => self.runtime_class("java/lang/Double"),
            Kind::Bool => self.runtime_class("java/lang/Boolean"),
            Kind::Char => self.runtime_class("java/lang/Character"),
            Kind::Text => self.refs.string,
            other => self.object_class(other),
        }
    }

    /// Converts the value on top of the stack, of type `from`, to the more
    /// specific type `to`. Raises a `ClassCastException` at runtime if the
    /// value has another type.
    pub(super) fn cast_top(&mut self, method: &mut Method<'a>, from: Kind, to: Kind, base: u16) {
        if !from.is_reference() {
            self.box_value(method, from);
        }
        if to.is_reference() {
            if to.non_null() != Kind::Any {
                let class = self.instance_class(to.non_null());
                instruction(&mut method.code, 0xc0, class); // checkcast
            }
        } else {
            self.unbox_element(method, to, base);
        }
        method.max_stack = method.max_stack.max(base + to.slots().max(from.slots()));
    }

    pub(super) fn cast(
        &mut self,
        method: &mut Method<'a>,
        value: &'a Expr<'a>,
        annotation: Name<'a>,
        safe: bool,
        base: u16,
    ) -> Result<Kind, Error> {
        let target = resolve_type(&self.type_scope, annotation)?;
        let kind = self.expression(method, value, base)?;
        self.symbols
            .check_runtime_target(target, kind, annotation)?;
        if kind == target {
            return Ok(kind);
        }
        if !kind.is_reference() && !target.is_reference() {
            return Err(at(
                annotation,
                "Convert numbers with toInt(), toLong(), toDouble(), or toChar() instead of as.",
            ));
        }
        if !safe {
            if !matches!(target, Kind::Nullable(_))
                && (matches!(kind, Kind::Nullable(_) | Kind::Null))
            {
                // `checkcast` lets null through, so a non-null target checks it.
                if !kind.is_reference() {
                    self.box_value(method, kind);
                }
                self.guard_non_null(method, target, base);
                return Ok(target);
            }
            self.cast_top(method, kind, target, base);
            return Ok(target);
        }
        let target = target.non_null();
        let Some(result) = target.nullable() else {
            return Err(at(
                annotation,
                "as? needs a type that can be null, such as Text or a class.",
            ));
        };
        if !kind.is_reference() {
            self.box_value(method, kind);
        }
        let source = if kind.is_reference() && kind != Kind::Null {
            self.local_type(kind)
        } else {
            self.local_type(Kind::Any)
        };
        let class = self.instance_class(target);
        method.code.push(0x59); // dup
        instruction(&mut method.code, 0xc1, class); // instanceof
        let matched = method.code.len();
        instruction(&mut method.code, 0x9a, 0); // ifne
        method.code.extend([0x57, 0x01]); // pop, aconst_null
        method.patch_branch(matched, method.code.len(), annotation.offset)?;
        method.stack.push(source);
        method.frame(method.locals.len());
        method.stack.pop();
        if target != Kind::Any {
            instruction(&mut method.code, 0xc0, class); // checkcast
        }
        method.max_stack = method.max_stack.max(base + 2);
        Ok(result)
    }
}
