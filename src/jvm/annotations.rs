//! Java declaration annotations. Values are constants, never executed code.
use super::*;
use crate::classfile::AnnotationValue;
use crate::syntax::{Annotation, AnnotationArgument};

impl<'a> Emitter<'a> {
    pub(super) fn annotate_method(
        &mut self,
        output: &mut Vec<u8>,
        start: usize,
        annotations: &[Annotation<'a>],
    ) -> Result<(), Error> {
        self.annotate_member(output, start, annotations, "METHOD")
    }

    pub(super) fn annotate_field(
        &mut self,
        output: &mut Vec<u8>,
        start: usize,
        annotations: &[Annotation<'a>],
    ) -> Result<(), Error> {
        self.annotate_member(output, start, annotations, "FIELD")
    }

    fn annotate_member(
        &mut self,
        output: &mut Vec<u8>,
        start: usize,
        annotations: &[Annotation<'a>],
        target: &str,
    ) -> Result<(), Error> {
        let (count, bytes) = self.annotation_attributes(annotations, target)?;
        let old = u16::from_be_bytes([output[start + 6], output[start + 7]]);
        output[start + 6..start + 8].copy_from_slice(&(old + count).to_be_bytes());
        output.extend(bytes);
        Ok(())
    }

    pub(super) fn annotation_attributes(
        &mut self,
        annotations: &[Annotation<'a>],
        target: &str,
    ) -> Result<(u16, Vec<u8>), Error> {
        let mut visible = Vec::new();
        let mut invisible = Vec::new();
        let mut seen = HashSet::new();
        for annotation in annotations {
            if annotations.len() > u16::MAX as usize
                || annotation.arguments.len() > u16::MAX as usize
            {
                return Err(at(
                    annotation.name,
                    "Too many annotations or annotation arguments for the JVM.",
                ));
            }
            let info = self.annotation_info(annotation.name)?;
            let binary = info.name.clone();
            if !seen.insert(binary.clone()) {
                return Err(at(
                    annotation.name,
                    "Declare an annotation only once. Repeatable annotations are not supported yet.",
                ));
            }
            let mut retention = "CLASS";
            for meta in &info.annotations {
                if meta.descriptor == "Ljava/lang/annotation/Retention;"
                    && let Some((_, AnnotationValue::Enum(owner, value))) =
                        meta.elements.iter().find(|(name, _)| name == "value")
                    && owner == "Ljava/lang/annotation/RetentionPolicy;"
                {
                    retention = value;
                }
                if meta.descriptor == "Ljava/lang/annotation/Target;" {
                    let allowed = meta.elements.iter().find(|(name, _)| name == "value")
                        .is_some_and(|(_, value)| match value {
                            AnnotationValue::Array(values) => values.iter().any(|value| matches!(value, AnnotationValue::Enum(owner, value) if owner == "Ljava/lang/annotation/ElementType;" && value == target)),
                            _ => false,
                        });
                    if !allowed {
                        return Err(at(
                            annotation.name,
                            "This Java annotation is not allowed on this declaration.",
                        ));
                    }
                }
            }
            let bytes = self.annotation_bytes(annotation, &info, 0)?;
            match retention {
                "SOURCE" => {}
                "RUNTIME" => visible.push(bytes),
                _ => invisible.push(bytes),
            }
        }
        let mut output = Vec::new();
        let mut count = 0;
        for (name, entries) in [
            ("RuntimeVisibleAnnotations", visible),
            ("RuntimeInvisibleAnnotations", invisible),
        ] {
            if entries.is_empty() {
                continue;
            }
            count += 1;
            let attribute = self.pool.ascii(name);
            shorts(&mut output, &[attribute]);
            word(
                &mut output,
                (2 + entries.iter().map(Vec::len).sum::<usize>()) as u32,
            );
            shorts(&mut output, &[entries.len() as u16]);
            for entry in entries {
                output.extend(entry);
            }
        }
        Ok((count, output))
    }

    fn annotation_info(&self, name: Name<'_>) -> Result<Rc<crate::classfile::ClassInfo>, Error> {
        let kind = if name.text.contains('.') {
            self.symbols.types.java_kind(
                &name
                    .text
                    .split('.')
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("/"),
            )
        } else {
            resolve_type(&self.type_scope, name)?
        };
        let Kind::Java(index) = kind else {
            return Err(at(name, "Use a public Java annotation type after '@'."));
        };
        let binary = self.symbols.types.java_name(index);
        let info = self
            .symbols
            .types
            .java_class(&binary)
            .ok_or_else(|| at(name, "This annotation type could not be loaded."))?;
        if info.access & 0x2000 == 0 || !info.is_public() {
            return Err(at(name, "Use a public Java annotation type after '@'."));
        }
        Ok(info)
    }

    fn annotation_bytes(
        &mut self,
        annotation: &Annotation<'a>,
        info: &crate::classfile::ClassInfo,
        depth: usize,
    ) -> Result<Vec<u8>, Error> {
        if depth > 64 || annotation.arguments.len() > u16::MAX as usize {
            return Err(at(
                annotation.name,
                "Too many nested annotation values for the JVM.",
            ));
        }
        let mut bytes = Vec::new();
        let descriptor = format!("L{};", info.name);
        let descriptor = self.pool.utf8(&modified_utf8(&descriptor));
        shorts(&mut bytes, &[descriptor, annotation.arguments.len() as u16]);
        let mut arguments = HashSet::new();
        for (name, value) in &annotation.arguments {
            if !arguments.insert(name.text) {
                return Err(at(*name, "Give each annotation argument only once."));
            }
            let member = info
                .methods
                .iter()
                .find(|method| method.name == name.text)
                .ok_or_else(|| at(*name, "This annotation has no element with that name."))?;
            let descriptor = member
                .descriptor
                .strip_prefix("()")
                .ok_or_else(|| at(*name, "Invalid Java annotation element."))?;
            let name_index = self.pool.utf8(&modified_utf8(name.text));
            shorts(&mut bytes, &[name_index]);
            self.annotation_value(&mut bytes, value, descriptor, depth + 1)?;
        }
        for member in &info.methods {
            if member.access & crate::classfile::ACC_ABSTRACT != 0
                && !member.has_default
                && !arguments.contains(member.name.as_str())
            {
                return Err(at(
                    annotation.name,
                    "Provide every required annotation element using named arguments.",
                ));
            }
        }
        Ok(bytes)
    }

    fn annotation_value(
        &mut self,
        output: &mut Vec<u8>,
        value: &AnnotationArgument<'a>,
        descriptor: &str,
        depth: usize,
    ) -> Result<(), Error> {
        let invalid = || Error {
            offset: value.offset(),
            message: "Use a constant annotation value matching the Java element type.",
        };
        if depth > 64 {
            return Err(Error {
                offset: value.offset(),
                message: "Too many nested annotation values for the JVM.",
            });
        }
        if let Some(element) = descriptor.strip_prefix('[') {
            if let AnnotationArgument::Constant(expr @ Expr::List(..)) = value {
                return self.annotation_constant(output, expr, descriptor);
            }
            output.push(b'[');
            if let AnnotationArgument::Array(_, values) = value {
                if values.len() > u16::MAX as usize {
                    return Err(invalid());
                }
                shorts(output, &[values.len() as u16]);
                for value in values {
                    self.annotation_value(output, value, element, depth + 1)?;
                }
            } else {
                shorts(output, &[1]);
                self.annotation_value(output, value, element, depth + 1)?;
            }
            return Ok(());
        }
        match value {
            AnnotationArgument::Nested(annotation) => {
                let info = self.annotation_info(annotation.name)?;
                if descriptor != format!("L{};", info.name) {
                    return Err(at(
                        annotation.name,
                        "This nested annotation does not match the Java element type.",
                    ));
                }
                // A nested value is not a declaration. Its Target and Retention
                // do not restrict its use here; the enclosing annotation controls retention.
                let bytes = self.annotation_bytes(annotation, &info, depth)?;
                output.push(b'@');
                output.extend(bytes);
                Ok(())
            }
            AnnotationArgument::Constant(value) => {
                self.annotation_constant(output, value, descriptor)
            }
            AnnotationArgument::Array(..) => Err(invalid()),
        }
    }

    fn annotation_constant(
        &mut self,
        output: &mut Vec<u8>,
        value: &Expr<'a>,
        descriptor: &str,
    ) -> Result<(), Error> {
        let invalid = || Error {
            offset: value.offset(),
            message: "Use a constant annotation value matching the Java element type.",
        };
        if let Some(element) = descriptor.strip_prefix('[') {
            let Expr::List(_, values) = value else {
                return Err(invalid());
            };
            if values.len() > u16::MAX as usize {
                return Err(invalid());
            }
            output.push(b'[');
            shorts(output, &[values.len() as u16]);
            for value in values {
                self.annotation_constant(output, value, element)?;
            }
            return Ok(());
        }
        let constant = match (descriptor, value) {
            ("Ljava/lang/String;", Expr::Text(_, parts)) => {
                let mut text = String::new();
                for part in parts {
                    if let Part::Literal(value) = part {
                        text.push_str(value);
                    } else {
                        return Err(invalid());
                    }
                }
                let bytes = modified_utf8(&text);
                if bytes.len() > u16::MAX as usize {
                    return Err(invalid());
                }
                output.push(b's');
                self.pool.utf8(&bytes)
            }
            ("Z", Expr::Bool(_, value)) => {
                output.push(b'Z');
                let index = self.pool.entry(3);
                word(&mut self.pool.bytes, u32::from(*value));
                index
            }
            ("I" | "B" | "S", Expr::Int(_, value)) => {
                if (descriptor == "B" && i8::try_from(*value).is_err())
                    || (descriptor == "S" && i16::try_from(*value).is_err())
                {
                    return Err(invalid());
                }
                output.push(descriptor.as_bytes()[0]);
                let index = self.pool.entry(3);
                word(&mut self.pool.bytes, *value as u32);
                index
            }
            ("C", Expr::Char(_, value)) if (*value as u32) <= 0xffff => {
                output.push(b'C');
                let index = self.pool.entry(3);
                word(&mut self.pool.bytes, *value as u32);
                index
            }
            ("J", Expr::Long(_, value)) => {
                output.push(b'J');
                let index = self.pool.entry(5);
                self.pool.count += 1;
                self.pool.bytes.extend(value.to_be_bytes());
                index
            }
            ("D", Expr::Double(_, value)) => {
                output.push(b'D');
                let index = self.pool.entry(6);
                self.pool.count += 1;
                self.pool.bytes.extend(value.to_bits().to_be_bytes());
                index
            }
            ("F", Expr::Double(_, value)) if (*value as f32).is_finite() => {
                output.push(b'F');
                let index = self.pool.entry(4);
                word(&mut self.pool.bytes, (*value as f32).to_bits());
                index
            }
            (_, Expr::Field(receiver, field)) => {
                let Expr::Variable(name) = receiver.as_ref() else {
                    return Err(invalid());
                };
                let kind = resolve_type(&self.type_scope, *name)?;
                if descriptor == "Ljava/lang/Class;" && field.text == "class" {
                    if matches!(
                        kind,
                        Kind::TypeParameter(_)
                            | Kind::Applied(_)
                            | Kind::AppliedInterface(_)
                            | Kind::Nullable(_)
                    ) {
                        return Err(invalid());
                    }
                    output.push(b'c');
                    self.pool
                        .utf8(&modified_utf8(&kind.descriptor(&self.symbols.types)))
                } else {
                    let Kind::Java(index) = kind else {
                        return Err(invalid());
                    };
                    let binary = self.symbols.types.java_name(index);
                    if descriptor != format!("L{binary};") {
                        return Err(invalid());
                    }
                    let info = self.symbols.types.java_class(&binary).ok_or_else(invalid)?;
                    if info.access & 0x4000 == 0
                        || !info.fields.iter().any(|member| {
                            member.name == field.text && member.access & 0x4001 == 0x4001
                        })
                    {
                        return Err(invalid());
                    }
                    output.push(b'e');
                    let owner = self.pool.utf8(&modified_utf8(descriptor));
                    shorts(output, &[owner]);
                    self.pool.utf8(&modified_utf8(field.text))
                }
            }
            _ => return Err(invalid()),
        };
        shorts(output, &[constant]);
        Ok(())
    }
}
