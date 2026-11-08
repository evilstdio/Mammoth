//! Real project builds: link modules, package resources, then verify the JAR.
use std::{fs, path::Path, process::Command};

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

fn run_project(project: &Path, expected: &str, jar: &Path) {
    let compiled = Command::new(env!("CARGO_BIN_EXE_mammoth"))
        .arg("--project")
        .arg(project)
        .arg(jar)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}: {}",
        project.display(),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-jar")
        .arg(jar)
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}: {}",
        project.display(),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        normalize(expected),
        "{}",
        project.display()
    );
}

#[test]
fn projects_link_and_verify() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("mammoth-projects-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&temporary).unwrap();
    let mut projects: Vec<_> = fs::read_dir(root.join("tests/projects"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    projects.sort();
    for (index, project) in projects.iter().enumerate() {
        let jar = temporary.join(format!("{index}.jar"));
        if let Ok(expected) = fs::read_to_string(project.join("expected.out")) {
            run_project(project, &expected, &jar);
        } else {
            // First line: diagnostic fragment. Second line: owning source path.
            let expected = fs::read_to_string(project.join("expected.error")).unwrap();
            let mut lines = expected.lines();
            let message = lines.next().unwrap();
            let source = lines.next().unwrap();
            let result = Command::new(env!("CARGO_BIN_EXE_mammoth"))
                .arg("--project")
                .arg(project)
                .arg(&jar)
                .output()
                .unwrap();
            let error = String::from_utf8_lossy(&result.stderr).replace('\\', "/");
            assert!(
                !result.status.success(),
                "{} compiled unexpectedly",
                project.display()
            );
            assert!(error.contains(message), "{error}");
            assert!(
                error.contains(&format!("{source}:")),
                "Wrong diagnostic source: {error}"
            );
            assert!(!jar.exists(), "A failed build produced a JAR");
        }
    }
    run_project(
        &root.join("examples/project"),
        "3.0",
        &temporary.join("original.jar"),
    );
    run_project(
        &root.join("examples/project-classes"),
        "Maya: 95\nSam: 80\nClass average: 87.5",
        &temporary.join("example.jar"),
    );
    fs::remove_dir_all(&temporary).unwrap();
}

#[test]
fn java_calls_exported_instance_methods() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "mammoth-method-exports-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("exports.jar");
    run_project(
        &root.join("tests/projects/method-exports"),
        "3\n14\nMammoth\njob",
        &jar,
    );
    let source = temporary.join("Caller.java");
    fs::write(
        &source,
        r#"
public class Caller {
    public static void main(String[] args) throws Exception {
        var counter = exports.API.counter();
        System.out.println(counter.add(3));
        System.out.println(counter.describe(5000000000L, 1.5, "wide"));
        System.out.println(counter.optional(null));
        Integer kept = counter.identity(42);
        System.out.println(kept);
        System.out.println(counter);
        System.out.println(exports.API.derived().add(3));
        var box = exports.API.box("typed");
        String value = box.get();
        System.out.println(value + ":" + box.keep("kept"));
        var method = counter.getClass().getMethod("identity", Object.class);
        System.out.println(method.getTypeParameters().length);
        System.out.println(counter.getClass().getMethod("add", int.class).invoke(counter, 2));
        try { counter.describe(1L, 2.0, null); throw new AssertionError("null accepted"); }
        catch (NullPointerException expected) { System.out.println("null rejected"); }
        try { counter.getClass().getMethod("hidden"); throw new AssertionError("hidden exported"); }
        catch (NoSuchMethodException expected) { System.out.println("hidden stays internal"); }
        exports.API.job().run();
        Runnable callback = exports.API.job();
        callback.run();
    }
}
"#,
    )
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([temporary.as_path(), jar.as_path()]).unwrap();
    let ran = Command::new("java")
        .args(["-Xverify:all", "-cp"])
        .arg(classpath)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "4\nwide:5000000000:1.5\nnull\n42\ncounter=4\n16\ntyped:kept\n1\n6\nnull rejected\nhidden stays internal\njob\njob"
    );
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn java_inherits_specialized_interface_defaults() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "mammoth-java-defaults-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("defaults.jar");
    run_project(
        &root.join("tests/projects/parent-defaults"),
        "42\n42\n10\nkept\nleft+right",
        &jar,
    );
    let source = temporary.join("Caller.java");
    fs::write(
        &source,
        r#"
class JavaNumbers implements defaults.API$Interface$0 { }
class JavaOverride implements defaults.API$Interface$0 {
    public Integer $next() { return 100; }
    public Integer $echo(Integer value) { return value + 20; }
}
class JavaNamed implements defaults.API$Interface$1 { }
public class Caller {
    public static void main(String[] args) {
        defaults.Base$Interface$0<Integer> parent = defaults.API.numbers();
        System.out.println(parent.$next());
        System.out.println(parent.$echo(8));
        System.out.println(parent.$keep("java"));
        defaults.Base$Interface$0<Integer> inherited = new JavaNumbers();
        System.out.println(inherited.$next());
        System.out.println(inherited.$echo(10));
        System.out.println(new JavaNamed().$label());
        System.out.println(defaults.API.viaNumbers(new JavaOverride()));
        System.out.println(defaults.API.viaSource(new JavaOverride()));
        System.out.println(defaults.API.viaNumbers(defaults.API.changed()));
        System.out.println(defaults.API.viaSource(defaults.API.changed()));
    }
}
"#,
    )
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([temporary.as_path(), jar.as_path()]).unwrap();
    let ran = Command::new("java")
        .args(["-Xverify:all", "-cp"])
        .arg(classpath)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "42\n9\njava\n42\n11\nleft+right\n130\n130\n120\n120"
    );
    fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn java_can_infer_mammoth_generic_exports() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "mammoth-java-generics-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("library.jar");
    run_project(
        &root.join("tests/projects/generic-functions"),
        "global\n18\n[MAYA]\n[8]",
        &jar,
    );
    let classes = temporary.join("classes.jar");
    run_project(
        &root.join("tests/projects/generic-classes"),
        "20\nPROJECT\nchanged",
        &classes,
    );
    let methods = temporary.join("methods.jar");
    run_project(
        &root.join("tests/projects/generic-methods"),
        "7\ninterface\n6",
        &methods,
    );
    let contracts = temporary.join("contracts.jar");
    run_project(
        &root.join("tests/projects/generic-interfaces"),
        "-1
6
3
30
5000000001
big:5000000000
6
42
8",
        &contracts,
    );
    let family = temporary.join("family.jar");
    run_project(
        &root.join("tests/projects/generic-inheritance"),
        "8
score:4
14
11
score
5.0
amount=2.5
5",
        &family,
    );
    let calc = temporary.join("calc.jar");
    run_project(
        &root.join("tests/projects/function-types"),
        "123\nx-y-z\nv10.5false\n15\n24",
        &calc,
    );
    let ranks = temporary.join("ranks.jar");
    run_project(
        &root.join("tests/projects/bounds"),
        "4\nc\nLeo!\n3\n100",
        &ranks,
    );
    let streams = temporary.join("streams.jar");
    run_project(
        &root.join("tests/projects/interface-inheritance"),
        "1
counter:2
9
5
l1l2
letters
counter+21
l2
letters!
last
any l3
echo",
        &streams,
    );
    let source = temporary.join("Caller.java");
    fs::write(
        &source,
        r#"
class JavaSource implements contracts.API$Interface$0<Integer> {
    int current = 0;
    public Integer $next() { current += 1; return current; }
    public Integer $peek() { return current; }
    public <R> R $convert(java.util.function.Function<Integer, R> change) { return change.apply(current); }
}

class JavaStream implements streams.API$Interface$0<Integer> {
    int count = 0;
    public Integer $next() { count += 1; return count; }
    public Integer $peek() { return count + 1; }
    public String $name() { return "java"; }
}

public class Caller {
    public static void main(String[] args) {
        System.out.println(contracts.API.drain(new JavaSource(), 3));
        contracts.API$Interface$0<Integer> numbers = contracts.API.numbers(5);
        Integer next = numbers.$next();
        String converted = numbers.$convert(value -> "n" + value);
        Long weight = contracts.API.named("java").$value();
        System.out.println(next + 1);
        System.out.println(converted);
        System.out.println(weight / 1000000000L);
        System.out.println(contracts.API.describe(contracts.API.named("j")));
        Integer inherited = family.Derived.score(3).$take();
        Double measured = family.Derived.weight(1.5).$amount();
        System.out.println(inherited + 1);
        System.out.println(measured * 2);
        System.out.println(calc.API.combine((a, b, c) -> a + b * c));
        String joined = calc.API.joiner("+").invoke("p", "q", "r");
        System.out.println(joined);
        calc.API.visit((text, number, ratio, flag) -> System.out.println(text + number + ratio + flag));
        Integer ruled = calc.API.rule().apply.invoke(1, 1, 1);
        System.out.println(ruled);
        Integer bigger = ranks.API.largest(10, 3);
        String later = ranks.API.largest("ant", "bee");
        System.out.println(bigger + later);
        Integer best = ranks.API.ranked().$best();
        System.out.println(best);
        System.out.println(streams.API.total(new JavaStream(), 3));
        System.out.println(streams.API.label(new JavaStream()));
        streams.Base$Interface$0<Integer> parent = streams.API.counter(10);
        Integer viaParent = parent.$next();
        System.out.println(viaParent);
        System.out.println(streams.API.both(new JavaStream()));
        System.out.println(new JavaStream().$shout());
        String[] ends = streams.API.swapEnds(new String[] {"x", "y", "z"});
        System.out.println(ends[0] + ends[2]);
        System.out.println(streams.API.peekAny(new JavaStream()));
        Runnable job = streams.API.job();
        job.run();
        Integer number = demo.Generic.identity(42);
        String name = demo.Generic.identity("Java");
        java.util.ArrayList<String> names = demo.Generic.singleton(name);
        java.util.ArrayList<Integer> sizes = demo.Generic.transform(names, value -> value.length());
        System.out.println(number);
        System.out.println(name);
        System.out.println(sizes.get(0) + 1);
        var box = demo.Library.make(25);
        Integer original = box.value;
        box.value = original + 1;
        Integer viaMethod = box.$get();
        Integer viaFunction = demo.Library.read(box);
        Integer fixed = demo.Library.fixed().value;
        String text = demo.Library.make("typed").value;
        System.out.println(viaMethod);
        System.out.println(viaFunction);
        System.out.println(fixed);
        System.out.println(text.toUpperCase());
        Integer kept = methods.API.service().$keep(13);
        Integer throughInterface = methods.API.contract().$keep(17);
        String mapped = methods.API.box(5).$map(value -> "java " + value).value;
        System.out.println(kept);
        System.out.println(throughInterface);
        System.out.println(mapped);
    }
}
"#,
    )
    .unwrap();
    let libraries = std::env::join_paths([
        jar.as_path(),
        classes.as_path(),
        methods.as_path(),
        contracts.as_path(),
        family.as_path(),
        calc.as_path(),
        ranks.as_path(),
        streams.as_path(),
    ])
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([
        temporary.as_path(),
        jar.as_path(),
        classes.as_path(),
        methods.as_path(),
        contracts.as_path(),
        family.as_path(),
        calc.as_path(),
        ranks.as_path(),
        streams.as_path(),
    ])
    .unwrap();
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(classpath)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "6\n7\nn6\n5\nj:5000000000\n4\n3.0\n7\np+q+r\nv10.5false\n3\n10bee\n99\n6\njava:1\n11\njava+1\njava!\nzx\nany 1\necho\n42\nJava\n5\n26\n26\n12\nTYPED\n13\n17\njava 5"
    );
    let wrong_bound = temporary.join("WrongBound.java");
    fs::write(
        &wrong_bound,
        "class WrongBound { void wrong() { ranks.API.largest(new Object(), new Object()); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong_bound)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java accepted an argument outside a Mammoth type parameter bound"
    );
    let wrong_second = temporary.join("WrongSecond.java");
    fs::write(
        &wrong_second,
        "class WrongSecond { void wrong(streams.Base$Interface$0<Integer> only) { streams.API.both(only); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong_second)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java accepted an argument that satisfies only the first of several bounds"
    );
    let wrong_array = temporary.join("WrongArray.java");
    fs::write(
        &wrong_array,
        "class WrongArray { void wrong() { streams.API.swapEnds(new int[] { 1, 2 }); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong_array)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java passed a primitive array to a Mammoth List<T> parameter"
    );
    let wrong_interface = temporary.join("WrongInterface.java");
    fs::write(
        &wrong_interface,
        "class WrongInterface { void wrong() { contracts.API$Interface$0<String> s = contracts.API.numbers(1); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong_interface)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java accepted an incompatible generic interface type"
    );
    let wrong = temporary.join("Wrong.java");
    fs::write(
        &wrong,
        "class Wrong { void wrong() { var box = demo.Library.make(1); box.value = \"wrong\"; } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java accepted an incompatible generic field value"
    );
    let wrong_method = temporary.join("WrongMethod.java");
    fs::write(
        &wrong_method,
        "class WrongMethod { void wrong() { methods.API.service().<String>$keep(3); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&libraries)
        .arg("-d")
        .arg(&temporary)
        .arg(wrong_method)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java accepted an incompatible generic method argument"
    );
    fs::remove_dir_all(temporary).unwrap();
}

/// Generated class files carry SourceFile and LineNumberTable attributes, so
/// real exception stack traces name the `.mam` file and line across modules,
/// interface defaults, constructors, field initializers, and lambdas.
#[test]
fn stack_traces_name_mammoth_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("mammoth-traces-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("traces.jar");
    run_project(
        &root.join("tests/traces/project"),
        "negative area | shapes.mam:8 $checked | main.mam:20 main
side too large | shapes.mam:20 $sign | shapes.mam:16 $area | main.mam:25 main
negative limit | shapes.mam:32 validate | shapes.mam:27 <init> | main.mam:30 main
side too large | shapes.mam:20 $sign | shapes.mam:16 $area | main.mam:37 lambda$0",
        &jar,
    );
    // An uncaught exception from a single-file program prints its own trace.
    let source = temporary.join("crash.mam");
    fs::write(
        &source,
        "fun fail(value: Int): Int {\n    if value > 1 {\n        error(\"boom\")\n    }\n    return value\n}\nprint(fail(1))\nprint(fail(2))\n",
    )
    .unwrap();
    let classes = temporary.join("classes");
    let compiled = Command::new(env!("CARGO_BIN_EXE_mammoth"))
        .arg(&source)
        .arg(&classes)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&classes)
        .arg("Main")
        .output()
        .unwrap();
    assert!(!ran.status.success());
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(stderr.contains("at Main.fail(crash.mam:3)"), "{stderr}");
    assert!(stderr.contains("at Main.main(crash.mam:8)"), "{stderr}");
    fs::remove_dir_all(&temporary).unwrap();
}

/// Mammoth protected methods are JVM protected methods: a Java subclass in
/// another package can override and call them, and javac rejects unrelated
/// callers.
#[test]
fn java_subclasses_use_protected_methods() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("mammoth-protected-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("engines.jar");
    run_project(
        &root.join("tests/projects/protected-methods"),
        "8\n19\nstate 8\n7\n21\n7",
        &jar,
    );
    let source = temporary.join("JavaDoubler.java");
    fs::write(
        &source,
        r#"
public class JavaDoubler extends child.Engine$$0 {
    JavaDoubler() { super(3); }
    @Override protected int $step(int value) { return value + 100; }
    int go() { return (Integer) $run(2); }
    public static void main(String[] args) {
        System.out.println(new JavaDoubler().go());
    }
}
"#,
    )
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([temporary.as_path(), jar.as_path()]).unwrap();
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&classpath)
        .arg("JavaDoubler")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(normalize(&String::from_utf8_lossy(&ran.stdout)), "203");
    let outsider = temporary.join("Outsider.java");
    fs::write(
        &outsider,
        "class Outsider { Object peek(child.Engine$$0 engine) { return engine.$run(1); } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&outsider)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java called a Mammoth protected method from an unrelated class"
    );
    fs::remove_dir_all(&temporary).unwrap();
}

/// Mammoth classes extend generic Java classes: inherited methods, protected
/// methods, and protected generic fields use the superclass's type arguments,
/// overrides get erased bridges, and Java sees the parameterized superclass.
#[test]
fn mammoth_extends_generic_java_classes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/java/generic-super");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "mammoth-generic-super-{}-{stamp}",
        std::process::id()
    ));
    let library = temporary.join("lib");
    let classes = temporary.join("classes");
    fs::create_dir_all(&library).unwrap();
    let javac = Command::new("javac")
        .arg("-d")
        .arg(&library)
        .arg(root.join("lib/Holder.java"))
        .output()
        .unwrap();
    assert!(
        javac.status.success(),
        "{}",
        String::from_utf8_lossy(&javac.stderr)
    );
    let compile = |source: &Path, output: &Path| {
        Command::new(env!("CARGO_BIN_EXE_mammoth"))
            .env("CLASSPATH", &library)
            .arg(source)
            .arg(output)
            .output()
            .unwrap()
    };
    let compiled = compile(&root.join("main.mam"), &classes);
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([classes.as_path(), library.as_path()]).unwrap();
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&classpath)
        .arg("Main")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "50\n66\n7\nHOLDER\n1\nb\nTWO2\n4"
    );
    // Java sees the parameterized superclass and can use the class as one.
    let caller = temporary.join("Caller.java");
    fs::write(
        &caller,
        r#"
public class Caller {
    public static void main(String[] args) {
        lib.Holder<Integer> holder = new Mammoth$0(5);
        Integer read = holder.read();
        System.out.println(read + 1);
        System.out.println(Mammoth$0.class.getGenericSuperclass());
        java.util.ArrayList<String> loud = new Mammoth$2();
        loud.add("java");
        System.out.println(loud.get(0));
    }
}
"#,
    )
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&classpath)
        .arg("-d")
        .arg(&temporary)
        .arg(&caller)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let java_path =
        std::env::join_paths([temporary.as_path(), classes.as_path(), library.as_path()]).unwrap();
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&java_path)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "6\nlib.Holder<java.lang.Integer>\nJAVA"
    );
    // Type arguments are enforced, final fields stay read-only, and protected
    // members stay protected.
    for (source, message) in [
        (
            "import lib.Holder\nclass Reader(s: Text) extends Holder<Text>(s)\nprint(Reader.protectedNames)\n",
            "This variable has not been defined here.",
        ),
        (
            "import lib.Holder\nclass Reader(s: Text) extends Holder<Text>(s) { fun bad() { protectedNames.add(1) } }\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nHolder.genericTie(null)\n",
            "This Java call is ambiguous.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nclass Sub(s: Text) extends Holder<Text>(s)\nlet wrong: JavaList<Text> = Sub(\"a\").numericEmpty()\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nclass Sub(s: Text) extends Holder<Text>(s) { fun wrong(): JavaList<Text> { return numericEmpty() } }\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nclass Sub(s: Text) extends Holder<Text>(s) { fun wrong(): JavaList<Text> { return super.numericEmpty() } }\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nlet holder: Holder<Text>? = Holder<Text>(\"x\")\nlet wrong: JavaList<Text>? = holder?.numericEmpty()\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nclass Sub(s: Text) extends Holder<Text>(s) { fun wrong(): Text { return shapeOf(\"a\", 1) } }\n",
            "No version of this Java method accepts these argument types.",
        ),
        (
            "import lib.Holder\nprint(Holder.shapeOf(1, 2))\n",
            "This Java class has no public static method with that name.",
        ),
        (
            "import lib.Holder\nimport java.util.Collections\nHolder.nest(Collections.singletonList(Collections.singletonList(5)))\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.nestedSpread(\"a\", ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.lowerOverload(ArrayList<Text>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.lowerOnly(ArrayList<Long>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nHolder.numbers().add(2.5)\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nimport java.math.BigInteger\nlet values = Holder.numbers()\nvalues.add(BigInteger(\"7\"))\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nHolder.unknown().add(\"text\")\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nimport java.math.BigInteger\nHolder.numberMap().put(\"b\", BigInteger(\"3\"))\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nHolder.wildField.add(1)\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nHolder<Text>(\"x\").texts.add(StringBuilder())\n",
            "This Java value has a wildcard type, such as List<? extends Number>, so nothing can be added to it.",
        ),
        (
            "import lib.Holder\nHolder.sink().add(\"no\")\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nlet first: Int = Holder.sink().get(0)\n",
            "Expected an Int here.",
        ),
        (
            "import lib.Holder\nHolder.plainField.add(2.5)\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nimport java.lang.Number\nlet widened: JavaList<Number> = Holder.numbers()\n",
            "Java generic type arguments must match the declared type.",
        ),
        (
            "import lib.Holder\nHolder.pour(Holder.numbers(), Holder.numbers())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.Collections\nCollections.copy(Holder.numbers(), Holder.numbers())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nfun pour<T>(into: JavaList<T>, from: JavaList<T>) { into.add(from.get(0)) }\npour(Holder.numbers(), Holder.numbers())\n",
            "This collection's type arguments do not match the generic function parameter.",
        ),
        (
            "import lib.Holder\nvar values = Holder.numbers()\nvalues = Holder.numbers()\n",
            "Java generic type arguments must match the declared type.",
        ),
        (
            "import lib.Holder\nlet make = { count: Int -> Holder.numbers() }\n",
            "This value has a Java wildcard type",
        ),
        (
            "import lib.Holder\nHolder.bothBodies({ 1 })\n",
            "This Java call is ambiguous.",
        ),
        ("import lib.Holder\nHolder.byResult({ true })\n", "Expected"),
        (
            "import lib.Holder\nHolder.arrayAmbiguous(null)\n",
            "This Java call is ambiguous.",
        ),
        (
            "import lib.Holder\nHolder.ambiguous(null)\n",
            "This Java call is ambiguous.",
        ),
        (
            "import lib.Holder\nHolder.crossed(\"a\", \"b\")\n",
            "This Java call is ambiguous.",
        ),
        (
            "import lib.Holder\nHolder.unrelated(\"text\")\n",
            "This Java call is ambiguous.",
        ),
        (
            "import lib.Holder\nHolder.annotated({ text: Text -> text })\n",
            "Expected an Int here.",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.overloaded(ArrayList<Bool>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder.Numeric\nlet invalid = Numeric<Text>()\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Numeric\nfun invalid(value: Numeric<Text>) {}\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Numeric\nclass Invalid extends Numeric<Text>()\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Numeric\nclass Invalid<T> { fun create(): Numeric<T> { return Numeric<T>() } }\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Ordered\nimport java.util.concurrent.atomic.AtomicInteger\nfun invalid(value: Ordered<AtomicInteger>) {}\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Dependent\nlet invalid = Dependent<Text, Int>()\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder.Recursive\nclass Wrong implements Comparable<Text> { fun compareTo(value: Text): Int { return 0 } }\nlet invalid = Recursive<Wrong>()\n",
            "This Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nHolder.supplied { \"wrong\" }\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nlet wrong: JavaList<Text> = Holder<Text>(\"owner\").numericEmpty()\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.List as JavaList\nimport java.util.ArrayList\nlet wrong: JavaList<Text> = Holder.listIdentity(ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nclass Dog\nclass Cat\nHolder.anchored(ArrayList<Dog>(), Cat())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nclass Dog\nclass Cat\nHolder.anchoredReverse(Cat(), ArrayList<Dog>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nclass Wrong implements Comparable<Text> { fun compareTo(other: Text): Int { return 0 } }\nHolder.self(Wrong())\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.concurrent.atomic.AtomicInteger\nHolder.ordered(AtomicInteger(1))\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nclass Only implements Runnable { fun run() {} }\nHolder.intersection(Only())\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.dependentAnchored(ArrayList<Text>(), 1)\n",
            "This inferred Java type argument does not satisfy its declared bounds.",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nimport java.util.List as JavaList\nHolder.nestedUpper(ArrayList<JavaList<Int>>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder<Text>(\"x\").exact(ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder<Text>(\"x\").lower(ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder<Text>(\"x\").upper(ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nHolder.nested(ArrayList<ArrayList<Text>>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nimport java.util.ArrayList\nimport java.lang.Number\nimport java.math.BigInteger\nlet n: Number = BigInteger(\"1\")\nHolder<Number>(n).exact(ArrayList<Int>())\n",
            "This argument does not match the Java type",
        ),
        (
            "import lib.Holder\nclass Bad(start: Int) extends Holder<Int>(start) { fun set() { value = \"text\" } }\n",
            "Expected an Int here.",
        ),
        (
            "import lib.Holder\nclass Bad(start: Int) extends Holder<Int>(start) { fun rename() { label = \"x\" } }\n",
            "This Java field is final, so it cannot be changed.",
        ),
        (
            "import lib.Holder\nclass Kept(start: Int) extends Holder<Int>(start)\nprint(Kept(1).value)\n",
            "This Java field is protected.",
        ),
        (
            "import lib.Holder\nclass Kept(start: Text) extends Holder<Text>(start)\nlet size: Int = Kept(\"a\").read()\n",
            "Expected an Int here.",
        ),
    ] {
        let file = temporary.join("bad.mam");
        fs::write(&file, source).unwrap();
        let result = compile(&file, &temporary.join("bad"));
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!result.status.success(), "accepted: {source}");
        assert!(stderr.contains(message), "{source}\n{stderr}");
    }
    fs::remove_dir_all(&temporary).unwrap();
}

/// Methods carry LocalVariableTable entries, so debuggers show Mammoth
/// parameter and variable names with their live ranges.
#[test]
fn debuggers_see_mammoth_variable_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("mammoth-locals-{}-{stamp}", std::process::id()));
    let compiled = Command::new(env!("CARGO_BIN_EXE_mammoth"))
        .arg(root.join("tests/traces/vars.mam"))
        .arg(&temporary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&temporary)
        .arg("Main")
        .output()
        .unwrap();
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "19\nhi maya"
    );
    let javap = |class: &str| {
        let output = Command::new("javap")
            .arg("-l")
            .arg("-cp")
            .arg(&temporary)
            .arg(class)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    };
    let main = javap("Main");
    for expected in [
        "0 101 0 values [I",
        "0 101 1 bonus I",
        "3 98 2 sum I",
        "25 19 5 value I",
        "31 7 6 doubled I",
        "57 7 3 extra I",
        "86 15 4 bump Ljava/lang/Runnable;",
    ] {
        assert!(
            main.iter().any(|line| line == expected),
            "missing {expected}: {main:#?}"
        );
    }
    let account = javap("Mammoth$0");
    assert!(
        account
            .iter()
            .any(|line| line.ends_with("0 this LMammoth$0;")),
        "{account:#?}"
    );
    assert!(
        account
            .iter()
            .any(|line| line.ends_with("2 label Ljava/lang/String;")),
        "{account:#?}"
    );
    fs::remove_dir_all(&temporary).unwrap();
}

/// Static members compile to JVM static methods and fields with plain names,
/// so Java calls them directly and reflection sees their modifiers.
#[test]
fn java_uses_mammoth_static_members() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary =
        std::env::temp_dir().join(format!("mammoth-statics-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&temporary).unwrap();
    let jar = temporary.join("stats.jar");
    run_project(
        &root.join("tests/projects/static-members"),
        "13\n21\n7\nregistry\n14",
        &jar,
    );
    let source = temporary.join("Caller.java");
    fs::write(
        &source,
        r#"
import java.lang.reflect.Modifier;
public class Caller {
    public static void main(String[] args) throws Exception {
        System.out.println(stats.Registry$$0.register("java"));
        stats.Registry$$0.count = 40;
        System.out.println(stats.Registry$$0.register("x"));
        System.out.println(stats.Registry$$0.name);
        var hidden = stats.Registry$$0.class.getDeclaredField("hidden");
        System.out.println(Modifier.isPrivate(hidden.getModifiers()) && Modifier.isStatic(hidden.getModifiers()));
        var name = stats.Registry$$0.class.getDeclaredField("name");
        System.out.println(Modifier.isFinal(name.getModifiers()));
    }
}
"#,
    )
    .unwrap();
    let compiled = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let classpath = std::env::join_paths([temporary.as_path(), jar.as_path()]).unwrap();
    let ran = Command::new("java")
        .arg("-Xverify:all")
        .arg("-cp")
        .arg(&classpath)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        normalize(&String::from_utf8_lossy(&ran.stdout)),
        "14\n411\nregistry\ntrue\ntrue"
    );
    let outsider = temporary.join("Outsider.java");
    fs::write(
        &outsider,
        "class Outsider { int peek() { return stats.Registry$$0.hidden; } }",
    )
    .unwrap();
    let rejected = Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg("-d")
        .arg(&temporary)
        .arg(&outsider)
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "Java read a private Mammoth static field"
    );
    fs::remove_dir_all(&temporary).unwrap();
}
