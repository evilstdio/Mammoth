mod classfile;
mod classpath;
mod inflate;
mod java_check;
mod jvm;
mod libraries;
mod project;
mod syntax;

use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{self, Command, ExitCode},
};

fn main() -> ExitCode {
    match run() {
        Ok(status) => status,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = env::args_os().skip(1);
    let first = args.next();
    if first.as_deref().is_some_and(|arg| arg == "--project") {
        let (Some(directory), output, None) = (args.next(), args.next(), args.next()) else {
            return Err(usage());
        };
        let output = output.map(PathBuf::from);
        return project::run(Path::new(&directory), output.as_deref());
    }
    let jar = first.as_deref().is_some_and(|arg| arg == "--jar");
    let check_java = first.as_deref().is_some_and(|arg| arg == "--check-java");
    let input = if jar || check_java {
        args.next()
    } else {
        first
    };
    let (Some(input), output) = (input, args.next()) else {
        return Err(usage());
    };
    if jar && output.is_none() {
        return Err(usage());
    }
    if check_java && output.is_some() {
        return Err(usage());
    }
    let resources = match args.next() {
        None => None,
        Some(flag) if jar && flag == "--resources" => {
            Some(PathBuf::from(args.next().ok_or_else(usage)?))
        }
        Some(_) => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }
    let input = PathBuf::from(input);
    let source = fs::read_to_string(&input)
        .map_err(|error| format!("Could not read {}: {error}", input.display()))?;
    let mut program =
        syntax::parse(&source).map_err(|error| error.display(&input.to_string_lossy(), &source))?;
    program.source_name = project::source_name(&input);
    if check_java {
        let checks = jvm::java_checks(&program)
            .map_err(|error| error.display(&input.to_string_lossy(), &source))?;
        java_check::validate(&checks, &input.to_string_lossy(), &source)?;
        println!("Java bindings verified.");
        return Ok(ExitCode::SUCCESS);
    }
    let classes = libraries::emit(&[&program]).map_err(|(index, error)| {
        if index == 0 {
            error.display(&input.to_string_lossy(), &source)
        } else {
            error.display("<mammoth.requests>", libraries::REQUESTS)
        }
    })?;
    let entry = program
        .module
        .as_deref()
        .unwrap_or("Main")
        .replace('/', ".");

    if let Some(output) = output {
        let output = PathBuf::from(output);
        if jar {
            return package_jar(&classes, &entry, &output, resources.as_deref());
        }
        fs::create_dir_all(&output)
            .map_err(|error| format!("Could not create {}: {error}", output.display()))?;
        write_classes(&output, &classes)?;
        return Ok(ExitCode::SUCCESS);
    }
    run_classes(&classes, &entry)
}

fn usage() -> String {
    "Usage: mammoth <source.mam> [output-directory]\n       mammoth --jar <source.mam> <output.jar> [--resources <directory>]\n       mammoth --check-java <source.mam>\n       mammoth --project <directory> [output.jar]"
        .into()
}

fn write_classes(output: &Path, classes: &[jvm::ClassFile]) -> Result<(), String> {
    for class in classes {
        let path = output.join(format!("{}.class", class.name));
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        fs::write(&path, &class.bytes)
            .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn run_classes(classes: &[jvm::ClassFile], entry: &str) -> Result<ExitCode, String> {
    with_classes(classes, |_, output| {
        let external = env::var_os("CLASSPATH").unwrap_or_default();
        let classpath = env::join_paths(
            std::iter::once(output.to_path_buf())
                .chain(env::split_paths(&external).filter(|path| !path.as_os_str().is_empty())),
        )
        .map_err(|error| format!("Could not build the Java classpath: {error}"))?;
        let status = Command::new("java")
            .arg("-cp")
            .arg(classpath)
            .arg(entry)
            .status()
            .map_err(|error| {
                if error.kind() == ErrorKind::NotFound {
                    "Could not find java. Add Java 17 or later to PATH.".into()
                } else {
                    format!("Could not start Java: {error}")
                }
            })?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from))
    })
}

fn package_jar(
    classes: &[jvm::ClassFile],
    entry: &str,
    output: &Path,
    resources: Option<&Path>,
) -> Result<ExitCode, String> {
    if let Some(resources) = resources
        && !resources.is_dir()
    {
        return Err(format!(
            "Resources directory does not exist: {}",
            resources.display()
        ));
    }
    with_classes(classes, |temp, classes_dir| {
        let archive = temp.join("mammoth.jar");
        let result = (|| {
            let mut jar = Command::new("jar");
            jar.arg("--create")
                .arg("--file")
                .arg(&archive)
                .arg("--main-class")
                .arg(entry)
                .arg("-C")
                .arg(classes_dir)
                .arg(".");
            if let Some(resources) = resources {
                jar.arg("-C").arg(resources).arg(".");
            }
            let status = jar.status().map_err(|error| {
                if error.kind() == ErrorKind::NotFound {
                    "Could not find jar. Add the Java 17 JDK or later to PATH.".into()
                } else {
                    format!("Could not start jar: {error}")
                }
            })?;
            if !status.success() {
                return Err("The JDK could not package the Mammoth JAR.".into());
            }
            fs::copy(&archive, output)
                .map_err(|error| format!("Could not write {}: {error}", output.display()))?;
            Ok(ExitCode::SUCCESS)
        })();
        let _ = fs::remove_file(archive);
        result
    })
}

fn with_classes(
    classes: &[jvm::ClassFile],
    use_classes: impl FnOnce(&Path, &Path) -> Result<ExitCode, String>,
) -> Result<ExitCode, String> {
    let temp = create_run_directory()?;
    let output = temp.join("classes");
    let result = write_classes(&output, classes).and_then(|()| use_classes(&temp, &output));
    for class in classes {
        let _ = fs::remove_file(output.join(format!("{}.class", class.name)));
    }
    for class in classes {
        let path = output.join(format!("{}.class", class.name));
        for parent in path
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != output)
        {
            let _ = fs::remove_dir(parent);
        }
    }
    let _ = fs::remove_dir(&output);
    let _ = fs::remove_dir(&temp);
    result
}

fn create_run_directory() -> Result<PathBuf, String> {
    let temp = env::temp_dir();
    for attempt in 0..100 {
        let path = temp.join(format!("mammoth-{}-{attempt}", process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("Could not create {}: {error}", path.display()));
            }
        }
    }
    Err(format!(
        "Could not reserve a run directory in {}.",
        temp.display()
    ))
}
