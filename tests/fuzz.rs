//! Differential smoke test: each seeded random program exists in Mammoth and
//! in Java. Both run under `-Xverify:all` and must print the same text and
//! fail the same way. `tests/fuzz/` has the larger tools for deeper runs.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const PROGRAMS: u64 = 24;

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn pick<'t>(&mut self, items: &[&'t str]) -> &'t str {
        items[self.below(items.len())]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Type {
    Int,
    Long,
    Double,
    Bool,
}

use Type::{Bool, Double, Int, Long};

impl Type {
    fn tracer(self) -> &'static str {
        match self {
            Int => "trI",
            Long => "trL",
            Double => "trD",
            Bool => "trB",
        }
    }

    fn mammoth(self) -> &'static str {
        match self {
            Int => "Int",
            Long => "Long",
            Double => "Double",
            Bool => "Bool",
        }
    }

    fn java(self) -> &'static str {
        match self {
            Int => "int",
            Long => "long",
            Double => "double",
            Bool => "boolean",
        }
    }
}

/// An expression in both languages.
type Pair = (String, String);

struct Generator {
    random: Random,
    mammoth: Vec<String>,
    java: Vec<String>,
    /// Name, type and whether it can be assigned.
    variables: Vec<(String, Type, bool)>,
    counter: usize,
    in_loop: bool,
}

impl Generator {
    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn emit(&mut self, mammoth: String, java: String) {
        self.mammoth.push(mammoth);
        self.java.push(java);
    }

    fn literal(&mut self, kind: Type) -> Pair {
        let text = match kind {
            Int => self
                .random
                .pick(&["0", "1", "7", "(-5)", "2147483647"])
                .to_owned(),
            Long => self
                .random
                .pick(&["0L", "3L", "(-4L)", "5000000000L"])
                .to_owned(),
            Double => self
                .random
                .pick(&["0.0", "1.5", "(-2.5)", "100000000.0"])
                .to_owned(),
            Bool => self.random.pick(&["true", "false"]).to_owned(),
        };
        (text.clone(), text)
    }

    fn variable(&mut self, kind: Type) -> Option<String> {
        let matching: Vec<String> = self
            .variables
            .iter()
            .filter(|variable| variable.1 == kind)
            .map(|variable| variable.0.clone())
            .collect();
        (!matching.is_empty()).then(|| matching[self.random.below(matching.len())].clone())
    }

    fn expression(&mut self, kind: Type, depth: usize) -> Pair {
        let choice = if depth > 3 {
            self.random.below(2)
        } else {
            self.random.below(8)
        };
        match choice {
            0 => {
                if let Some(name) = self.variable(kind) {
                    return (name.clone(), name);
                }
                self.literal(kind)
            }
            1 => self.literal(kind),
            2 => {
                let inner = self.expression(kind, depth + 1);
                let tracer = kind.tracer();
                (
                    format!("{tracer}({})", inner.0),
                    format!("{tracer}({})", inner.1),
                )
            }
            3 => {
                let test = self.expression(Bool, depth + 1);
                let yes = self.expression(kind, depth + 1);
                let no = self.expression(kind, depth + 1);
                (
                    format!("if {} {{ {} }} else {{ {} }}", test.0, yes.0, no.0),
                    format!("({} ? {} : {})", test.1, yes.1, no.1),
                )
            }
            4 => {
                let subject = self.variable(Int).unwrap_or_else(|| "3".to_owned());
                let first = self.expression(kind, depth + 1);
                let second = self.expression(kind, depth + 1);
                let number = self.random.below(5);
                (
                    format!(
                        "when {subject} {{\n{number} -> {}\nelse -> {} }}",
                        first.0, second.0
                    ),
                    format!("({subject} == {number} ? {} : {})", first.1, second.1),
                )
            }
            _ if kind == Bool => self.boolean(depth),
            _ => {
                let left = self.expression(kind, depth + 1);
                let right = self.expression(kind, depth + 1);
                let operator = self.random.pick(&["+", "-", "*", "/", "%"]);
                (
                    format!("({} {operator} {})", left.0, right.0),
                    format!("({} {operator} {})", left.1, right.1),
                )
            }
        }
    }

    fn boolean(&mut self, depth: usize) -> Pair {
        match self.random.below(3) {
            0 => {
                let kinds = [Int, Long, Double];
                let left_kind = kinds[self.random.below(3)];
                let right_kind = kinds[self.random.below(3)];
                let left = self.expression(left_kind, depth + 1);
                let right = self.expression(right_kind, depth + 1);
                let operator = self.random.pick(&["<", "<=", ">", ">=", "==", "!="]);
                (
                    format!("({} {operator} {})", left.0, right.0),
                    format!("({} {operator} {})", left.1, right.1),
                )
            }
            1 => {
                let inner = self.expression(Bool, depth + 1);
                (format!("(not {})", inner.0), format!("(!{})", inner.1))
            }
            _ => {
                let left = self.expression(Bool, depth + 1);
                let right = self.expression(Bool, depth + 1);
                let (mammoth, java) = if self.random.below(2) == 0 {
                    ("and", "&&")
                } else {
                    ("or", "||")
                };
                (
                    format!("({} {mammoth} {})", left.0, right.0),
                    format!("({} {java} {})", left.1, right.1),
                )
            }
        }
    }

    fn block(&mut self, budget: usize) {
        let saved = self.variables.len();
        for _ in 0..1 + self.random.below(3) {
            self.statement(budget);
        }
        self.variables.truncate(saved);
    }

    fn statement(&mut self, budget: usize) {
        let limit = if budget == 0 { 3 } else { 7 };
        let extra = usize::from(self.in_loop);
        // The extra choice in a loop is `break` or `continue`.
        let choice = self.random.below(limit + extra);
        match if choice == limit { 7 } else { choice } {
            0 => {
                let kind = [Int, Long, Double, Bool][self.random.below(4)];
                let value = self.expression(kind, 0);
                let name = self.fresh("v");
                self.emit(
                    format!("var {name}: {} = {}", kind.mammoth(), value.0),
                    format!("{} {name} = {};", kind.java(), value.1),
                );
                self.variables.push((name, kind, true));
            }
            1 => {
                let writable: Vec<(String, Type)> = self
                    .variables
                    .iter()
                    .filter(|variable| variable.2)
                    .map(|variable| (variable.0.clone(), variable.1))
                    .collect();
                if writable.is_empty() {
                    return;
                }
                let (name, kind) = writable[self.random.below(writable.len())].clone();
                let value = self.expression(kind, 1);
                let operator = if kind == Bool {
                    "="
                } else {
                    self.random.pick(&["=", "+=", "-="])
                };
                self.emit(
                    format!("{name} {operator} {}", value.0),
                    format!("{name} {operator} {};", value.1),
                );
            }
            2 => {
                let kind = [Int, Long, Double, Bool][self.random.below(4)];
                let value = self.expression(kind, 0);
                self.emit(
                    format!("print({})", value.0),
                    format!("System.out.println({});", value.1),
                );
            }
            3 => {
                let test = self.expression(Bool, 1);
                self.emit(format!("if {} {{", test.0), format!("if ({}) {{", test.1));
                self.block(budget - 1);
                self.emit("} else {".to_owned(), "} else {".to_owned());
                self.block(budget - 1);
                self.emit("}".to_owned(), "}".to_owned());
            }
            4 => {
                let name = self.fresh("i");
                let bound = 1 + self.random.below(3);
                self.emit(format!("var {name} = 0"), format!("int {name} = 0;"));
                self.emit(
                    format!("while {name} < {bound} {{"),
                    format!("while ({name} < {bound}) {{"),
                );
                self.emit(format!("{name} += 1"), format!("{name} += 1;"));
                let outer = std::mem::replace(&mut self.in_loop, true);
                self.block(budget - 1);
                self.in_loop = outer;
                self.emit("}".to_owned(), "}".to_owned());
            }
            5 => {
                let name = self.fresh("i");
                self.emit(
                    format!("for {name} in 0..2 {{"),
                    format!("for (int {name} = 0; {name} <= 2; {name}++) {{"),
                );
                self.variables.push((name, Int, false));
                let outer = std::mem::replace(&mut self.in_loop, true);
                self.block(budget - 1);
                self.in_loop = outer;
                self.variables.pop();
                self.emit("}".to_owned(), "}".to_owned());
            }
            6 => {
                let caught = self.fresh("e");
                self.emit("try {".to_owned(), "try {".to_owned());
                self.block(budget - 1);
                self.emit(
                    format!("}} catch ({caught}: ArithmeticException) {{"),
                    format!("}} catch (ArithmeticException {caught}) {{"),
                );
                self.block(budget - 1);
                let outer = std::mem::replace(&mut self.in_loop, false);
                self.emit("} finally {".to_owned(), "} finally {".to_owned());
                self.block(budget - 1);
                self.in_loop = outer;
                self.emit("}".to_owned(), "}".to_owned());
            }
            _ => {
                let test = self.expression(Bool, 2);
                let word = self.random.pick(&["break", "continue"]);
                self.emit(
                    format!("if {} {{ {word} }}", test.0),
                    format!("if ({}) {{ {word}; }}", test.1),
                );
            }
        }
    }
}

fn tracers(mammoth: &mut Vec<String>, java: &mut Vec<String>) {
    for kind in [Int, Long, Double, Bool] {
        let (tracer, source, target) = (kind.tracer(), kind.mammoth(), kind.java());
        mammoth.push(format!(
            "fun {tracer}(x: {source}): {source} {{\nprint(\"t{{x}}\")\nreturn x\n}}"
        ));
        java.push(format!(
            "static {target} {tracer}({target} x) {{ System.out.println(\"t\" + x); return x; }}"
        ));
    }
}

fn program(seed: u64) -> (String, String) {
    let mut generator = Generator {
        random: Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
        mammoth: Vec::new(),
        java: Vec::new(),
        variables: Vec::new(),
        counter: 0,
        in_loop: false,
    };
    let (mut mammoth, mut java) = (Vec::new(), Vec::new());
    tracers(&mut mammoth, &mut java);
    for _ in 0..6 {
        generator.statement(3);
    }
    mammoth.extend(generator.mammoth);
    (
        mammoth.join("\n") + "\n",
        format!(
            "public class J {{\n{}\npublic static void main(String[] args) {{\n{}\n}}\n}}\n",
            java.join("\n"),
            generator.java.join("\n")
        ),
    )
}

fn run(class: &str, classpath: &Path) -> Output {
    Command::new("java")
        .args(["-Xverify:all", "-cp"])
        .arg(classpath)
        .arg(class)
        .output()
        .expect("java runs")
}

fn check(seed: u64, root: &Path) -> Result<(), String> {
    let (mammoth, java) = program(seed);
    let directory = root.join(seed.to_string());
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let source = directory.join("p.mam");
    let reference = directory.join("J.java");
    fs::write(&source, &mammoth).map_err(|error| error.to_string())?;
    fs::write(&reference, &java).map_err(|error| error.to_string())?;
    let javac = Command::new("javac")
        .args(["-nowarn", "-d"])
        .arg(&directory)
        .arg(&reference)
        .output()
        .expect("javac runs");
    if !javac.status.success() {
        return Err(format!(
            "seed {seed}: the Java side did not compile\n{}",
            String::from_utf8_lossy(&javac.stderr)
        ));
    }
    let classes = directory.join("out");
    let compiled = Command::new(env!("CARGO_BIN_EXE_mammoth"))
        .arg(&source)
        .arg(&classes)
        .output()
        .expect("the compiler runs");
    if !compiled.status.success() {
        return Err(format!(
            "seed {seed}: rejected\n{}\n{mammoth}",
            String::from_utf8_lossy(&compiled.stderr)
        ));
    }
    let ours = run("Main", &classes);
    let theirs = run("J", &directory);
    if ours.stdout != theirs.stdout || ours.status.success() != theirs.status.success() {
        return Err(format!(
            "seed {seed}: results differ\n{}\n{mammoth}",
            String::from_utf8_lossy(&ours.stderr)
        ));
    }
    Ok(())
}

#[test]
fn random_programs_match_java() {
    let root = std::env::temp_dir().join(format!("mammoth-fuzz-{}", std::process::id()));
    let threads: Vec<_> = (0..4u64)
        .map(|worker| {
            let root = root.clone();
            std::thread::spawn(move || {
                (0..PROGRAMS / 4)
                    .map(|index| check(worker * 1000 + index + 1, &root))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut failures = Vec::new();
    for thread in threads {
        failures.extend(thread.join().unwrap().into_iter().filter_map(Result::err));
    }
    let _ = fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
