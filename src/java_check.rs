use std::{
    fs,
    io::{ErrorKind, Write},
    path::Path,
    process::{Command, Stdio},
};

use crate::{create_run_directory, jvm::JavaCheck, syntax};

pub fn validate(checks: &[JavaCheck], path: &str, source: &str) -> Result<(), String> {
    if checks.is_empty() {
        return Ok(());
    }
    let temp = create_run_directory()?;
    let helper = temp.join("MammothJavaCheck.java");
    let result = fs::write(&helper, include_str!("MammothJavaCheck.java"))
        .map_err(|error| format!("Could not write the Java declaration checker: {error}"))
        .and_then(|()| run_checker(&helper, checks, path, source));
    let _ = fs::remove_file(helper);
    let _ = fs::remove_dir(temp);
    result
}

fn run_checker(
    helper: &Path,
    checks: &[JavaCheck],
    path: &str,
    source: &str,
) -> Result<(), String> {
    let mut child = Command::new(crate::classpath::java_tool("java"))
        .arg(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                crate::MISSING_JAVA.replace("{tool}", "java")
            } else {
                format!("Could not start the Java declaration checker: {error}")
            }
        })?;
    let mut input = child.stdin.take().expect("Java checker stdin is piped");
    let written = checks.iter().try_for_each(|check| {
        writeln!(
            input,
            "{}\t{}\t{}\t{}",
            check.operation, check.class, check.member, check.descriptor
        )
    });
    drop(input);
    let result = child
        .wait_with_output()
        .map_err(|error| format!("Could not finish checking Java declarations: {error}"))?;
    if !result.status.success() {
        return Err("The Java declaration checker failed. Use a Java 17 JDK or later.".into());
    }
    written.map_err(|error| format!("Could not send declarations to the Java checker: {error}"))?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let mut diagnostics = Vec::new();
    for line in stdout.lines() {
        let (index, message) = line
            .split_once('\t')
            .ok_or_else(|| "The Java checker returned an invalid diagnostic.".to_owned())?;
        let check = index
            .parse::<usize>()
            .ok()
            .and_then(|index| checks.get(index))
            .ok_or_else(|| "The Java checker returned an invalid source location.".to_owned())?;
        diagnostics.push(syntax::diagnostic(path, source, check.offset, message));
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics.join("\n"))
    }
}
