//! JVM interfaces for function types with more than two parameters.
//!
//! Function types with up to two parameters use `java.util.function`
//! interfaces such as `BiFunction`. Larger function types use generated
//! interfaces in the `mammoth/function` package: `FunctionN` for a function
//! with a result, with `Object invoke(Object...)`, and `ProcedureN` for one
//! without, with `void invoke(Object...)`. Each output that uses one of them
//! contains its class file, so compiled programs and JARs remain
//! self-contained and Java code can implement them with a lambda.
use std::collections::HashMap;
use std::sync::Mutex;

use super::{ClassFile, Pool, Types, shorts, word};

/// The most parameters a function type can have: the JVM limits a method to
/// 255 argument slots, including the receiver.
pub(super) const MAX_PARAMETERS: usize = 254;

type Names = (&'static str, &'static str);

/// Interned names so that `FunctionType::interface` can keep returning
/// `&'static str` values like the JDK interfaces. There are at most
/// `2 * MAX_PARAMETERS` entries.
static NAMES: Mutex<Option<HashMap<(usize, bool), Names>>> = Mutex::new(None);

/// The internal class name and erased `invoke` descriptor for a generated
/// function interface.
pub(super) fn names(parameters: usize, result: bool) -> Names {
    let mut table = NAMES.lock().unwrap();
    *table
        .get_or_insert_with(HashMap::new)
        .entry((parameters, result))
        .or_insert_with(|| {
            let class = if result {
                format!("mammoth/function/Function{parameters}")
            } else {
                format!("mammoth/function/Procedure{parameters}")
            };
            let descriptor = format!(
                "({}){}",
                "Ljava/lang/Object;".repeat(parameters),
                if result { "Ljava/lang/Object;" } else { "V" }
            );
            (
                Box::leak(class.into_boxed_str()),
                Box::leak(descriptor.into_boxed_str()),
            )
        })
}

/// Class files for every generated function interface the compiled code
/// refers to.
pub(super) fn interfaces(types: &Types<'_>) -> Vec<ClassFile> {
    let mut wanted: Vec<(usize, bool)> = types
        .functions
        .borrow()
        .iter()
        .filter(|function| function.parameters.len() > 2)
        .map(|function| (function.parameters.len(), function.result.is_some()))
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
        .into_iter()
        .map(|(parameters, result)| interface(parameters, result))
        .collect()
}

fn interface(parameters: usize, result: bool) -> ClassFile {
    let (name, descriptor) = names(parameters, result);
    let mut pool = Pool {
        count: 1,
        bytes: Vec::new(),
        members: HashMap::new(),
    };
    let this_class = pool.class(name);
    let object_class = pool.class("java/lang/Object");
    let invoke = pool.ascii("invoke");
    let erased = pool.ascii(descriptor);
    let signature_name = pool.ascii("Signature");
    let functional = pool.ascii("RuntimeVisibleAnnotations");
    let annotation = pool.ascii("Ljava/lang/FunctionalInterface;");
    // Type variables P1..Pn and R, as in <P1:Ljava/lang/Object;...>.
    let mut variables = String::from("<");
    let mut method = String::from("(");
    for index in 1..=parameters {
        variables.push_str(&format!("P{index}:Ljava/lang/Object;"));
        method.push_str(&format!("TP{index};"));
    }
    method.push(')');
    if result {
        variables.push_str("R:Ljava/lang/Object;");
        method.push_str("TR;");
    } else {
        method.push('V');
    }
    variables.push('>');
    let class_signature = pool.ascii(&format!("{variables}Ljava/lang/Object;"));
    let method_signature = pool.ascii(&method);
    let mut bytes = Vec::with_capacity(pool.bytes.len() + 96);
    word(&mut bytes, 0xcafebabe);
    shorts(&mut bytes, &[0, 61, pool.count as u16]);
    bytes.extend(&pool.bytes);
    // public interface abstract, no superinterfaces or fields, one method.
    shorts(&mut bytes, &[0x0601, this_class, object_class, 0, 0, 1]);
    shorts(&mut bytes, &[0x0401, invoke, erased, 1, signature_name]);
    word(&mut bytes, 2);
    shorts(&mut bytes, &[method_signature]);
    shorts(&mut bytes, &[2, signature_name]);
    word(&mut bytes, 2);
    shorts(&mut bytes, &[class_signature, functional]);
    word(&mut bytes, 6);
    shorts(&mut bytes, &[1, annotation, 0]);
    ClassFile {
        name: name.to_owned(),
        bytes,
    }
}
