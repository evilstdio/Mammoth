use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::{jvm, package_jar, run_classes, syntax};

pub fn run(directory: &Path, output: Option<&Path>) -> Result<ExitCode, String> {
    let mut paths = vec![directory.join("main.mam")];
    let source_dir = directory.join("src");
    if source_dir.exists() {
        collect_sources(&source_dir, &mut paths, 0)?;
        paths[1..].sort();
    }
    let sources: Vec<_> = paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))
        })
        .collect::<Result<_, _>>()?;
    syntax::declare_project_names(&sources);
    let programs: Vec<_> = sources
        .iter()
        .zip(&paths)
        .map(|(source, path)| {
            let mut program = syntax::parse(source)
                .map_err(|error| error.display(&path.to_string_lossy(), source))?;
            program.source_name = source_name(path);
            Ok::<_, String>(program)
        })
        .collect::<Result<_, _>>()?;
    let mut filenames = HashSet::new();
    for (index, (path, program)) in paths.iter().zip(&programs).enumerate() {
        if index != 0 && !program.body.is_empty() {
            return Err(syntax::diagnostic(
                &path.to_string_lossy(),
                &sources[index],
                program.body[0].offset(),
                "Put top-level executable code in main.mam.",
            ));
        }
        match program.module.as_deref() {
            Some(module) => {
                if !filenames.insert(module.to_ascii_lowercase()) {
                    return Err(format!(
                        "{} declares a module name that is already used in this project.",
                        path.display()
                    ));
                }
            }
            None if index != 0 => {
                return Err(format!(
                    "{} needs a module declaration, such as module example.Logic.",
                    path.display()
                ));
            }
            None => {}
        }
    }
    let classes = jvm::emit_project(&programs).map_err(|(index, error)| {
        error.display(&paths[index].to_string_lossy(), &sources[index])
    })?;

    let entry = programs[0]
        .module
        .as_deref()
        .unwrap_or("Main")
        .replace('/', ".");
    if let Some(output) = output {
        let resources = directory.join("resources");
        package_jar(
            &classes,
            &entry,
            output,
            resources.exists().then_some(resources.as_path()),
        )
    } else {
        run_classes(&classes, &entry)
    }
}

fn collect_sources(directory: &Path, paths: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
    if depth == 64 {
        return Err(format!(
            "Project source folders are nested too deeply at {}.",
            directory.display()
        ));
    }
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Could not read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if kind.is_dir() {
            collect_sources(&path, paths, depth + 1)?;
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "mam") {
            paths.push(path);
        }
    }
    Ok(())
}

/// The file name recorded in class files for stack traces, such as `api.mam`.
pub fn source_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "Main.mam".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}
