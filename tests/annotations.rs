//! Compile real Java annotation declarations, then consume annotated Mammoth
//! bytecode through Java reflection and javap under the strict verifier.
use std::{fs, process::Command};

#[test]
fn java_observes_mammoth_annotations() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = std::env::temp_dir().join(format!(
        "mammoth-annotations-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&temporary).unwrap();
    let declarations = [
        (
            "FieldMeta.java",
            "package ann; import java.lang.annotation.*; @Target(ElementType.FIELD) @Retention(RetentionPolicy.RUNTIME) public @interface FieldMeta { String value(); }",
        ),
        (
            "Detail.java",
            "package ann; import java.lang.annotation.*; @Target({}) @Retention(RetentionPolicy.SOURCE) public @interface Detail { String value(); }",
        ),
        (
            "At.java",
            "package ann; import java.lang.annotation.*; @Target({}) public @interface At { String value(); int ordinal() default -1; Detail detail() default @Detail(value=\"default\"); }",
        ),
        (
            "Inject.java",
            "package ann; import java.lang.annotation.*; @Target(ElementType.METHOD) @Retention(RetentionPolicy.RUNTIME) public @interface Inject { String[] method(); At[] at(); }",
        ),
        (
            "Envelope.java",
            "package ann; import java.lang.annotation.*; @Target(ElementType.TYPE) @Retention(RetentionPolicy.RUNTIME) public @interface Envelope { Inject value(); }",
        ),
        (
            "Meta.java",
            r#"package ann;
import java.lang.annotation.*;
@Retention(RetentionPolicy.RUNTIME) @Target({ElementType.TYPE, ElementType.METHOD})
public @interface Meta {
 String value(); int[] counts(); Class<?>[] types(); RetentionPolicy policy();
 long big(); double ratio(); char letter(); float fraction(); byte tiny(); short small();
 boolean flag() default true;
}"#,
        ),
        (
            "ClassMark.java",
            "package ann; public @interface ClassMark {}",
        ),
        (
            "SourceMark.java",
            "package ann; import java.lang.annotation.*; @Retention(RetentionPolicy.SOURCE) public @interface SourceMark {}",
        ),
    ];
    let mut javac = Command::new("javac");
    javac.args(["-encoding", "UTF-8", "-d"]).arg(&temporary);
    for (name, contents) in declarations {
        let path = temporary.join(name);
        fs::write(&path, contents).unwrap();
        javac.arg(path);
    }
    let compiled = javac.output().unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let source = temporary.join("main.mam");
    let mammoth = r#"module demo.Annotated
import ann.Meta
import ann.ClassMark
import ann.SourceMark
import ann.At
import ann.Detail
import ann.Inject
import ann.Envelope
import ann.FieldMeta
import java.lang.annotation.RetentionPolicy
@Meta(value = "Mammoth\u0000", counts = [1, 2, -3], types = [String.class, Int.class], policy = RetentionPolicy.RUNTIME, big = 5000000000L, ratio = 1.25, letter = 'M', fraction = 0.5, tiny = -8, small = 300)
@ClassMark
@SourceMark
@Envelope(value = @Inject(method = "tick", at = @At(value = "HEAD", detail = @Detail(value = "nested source"))))
class Example {
    @Inject(method = "tick", at = @At(value = "HEAD", detail = @Detail(value = "nested")))
    export fun tick() { }
    @Inject(method = ["tick", "draw"], at = [@At(value = "HEAD"), @ann.At(value = "TAIL", ordinal = 2)])
    export fun many() { }
    @Inject(method = [], at = [])
    export fun empty() { }
    @Deprecated(since = "0.3", forRemoval = true)
    export fun greet(): Text { return "hello" }
    @SourceMark
    export fun sourceOnly() { }
}
@Deprecated
interface Contract {
    @Deprecated fun name(): Text
    @Deprecated fun greet(): Text { return "default" }
}
@Deprecated
export fun make(): Example { return Example() }
class Callback implements Runnable {
    @Deprecated
    fun run() { }
}
export fun callback(): Callback { return Callback() }
class AnnotatedBox<T>(@FieldMeta(value = "constructor") protected var value: T) {
    @FieldMeta(value = "body")
    private let hidden = "secret"
    @ClassMark
    private var cached = 1
    @SourceMark
    var scratch = 0
    export fun read(): T { return value }
}
export fun box(): AnnotatedBox<Text> { return AnnotatedBox("value") }
print(make().greet())
"#;
    // Include a real NUL and non-ASCII text to exercise modified UTF-8.
    let mammoth = mammoth.replace("Mammoth\\u0000", "Mammoth\0🐘");
    fs::write(&source, &mammoth).unwrap();
    let output = temporary.join("out");
    let compiled = Command::new(env!("CARGO_BIN_EXE_mammoth"))
        .env("CLASSPATH", &temporary)
        .arg(&source)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let caller = temporary.join("Caller.java");
    fs::write(&caller, r#"
import ann.Meta;
class JavaBox extends demo.Annotated$$2<String> {
 JavaBox() { super("java"); }
 String readProtected() { return value; }
}
public class Caller {
 public static void main(String[] args) throws Exception {
  var value = demo.Annotated.make();
  var box = demo.Annotated.box();
  var protectedField = box.getClass().getDeclaredField("value");
  if (!java.lang.reflect.Modifier.isProtected(protectedField.getModifiers()) || !protectedField.getGenericType().getTypeName().equals("T") || !protectedField.getAnnotation(ann.FieldMeta.class).value().equals("constructor")) throw new AssertionError("constructor property annotation/signature");
  var hiddenField = box.getClass().getDeclaredField("hidden");
  if (!java.lang.reflect.Modifier.isPrivate(hiddenField.getModifiers()) || !java.lang.reflect.Modifier.isFinal(hiddenField.getModifiers()) || !hiddenField.getAnnotation(ann.FieldMeta.class).value().equals("body")) throw new AssertionError("private final field annotation");
  if (box.getClass().getConstructors()[0].getParameterAnnotations()[0].length != 0 || !box.read().equals("value") || !new JavaBox().readProtected().equals("java")) throw new AssertionError("field target and Java subclass access");
  if (box.getClass().getDeclaredField("cached").getAnnotation(ann.ClassMark.class) != null || box.getClass().getDeclaredField("scratch").getAnnotation(ann.SourceMark.class) != null) throw new AssertionError("field retention");
  var meta = value.getClass().getAnnotation(Meta.class);
  var tick = value.getClass().getMethod("tick").getAnnotation(ann.Inject.class);
  if (!tick.method()[0].equals("tick") || !tick.at()[0].value().equals("HEAD") || !tick.at()[0].detail().value().equals("nested") || tick.at()[0].ordinal() != -1) throw new AssertionError("nested singleton values");
  var many = value.getClass().getMethod("many").getAnnotation(ann.Inject.class);
  if (many.at().length != 2 || many.method().length != 2 || !many.at()[1].value().equals("TAIL") || many.at()[1].ordinal() != 2 || !many.at()[0].detail().value().equals("default")) throw new AssertionError("nested arrays and defaults");
  if (value.getClass().getMethod("empty").getAnnotation(ann.Inject.class).at().length != 0) throw new AssertionError("empty annotation array");
  if (!value.getClass().getAnnotation(ann.Envelope.class).value().at()[0].detail().value().equals("nested source")) throw new AssertionError("nested target and retention");
  if (!meta.value().equals("Mammoth\0🐘") || meta.counts()[2] != -3 || meta.types()[0] != String.class || meta.types()[1] != int.class || meta.policy() != java.lang.annotation.RetentionPolicy.RUNTIME || meta.big() != 5000000000L || meta.ratio() != 1.25 || meta.letter() != 'M' || meta.fraction() != 0.5f || meta.tiny() != -8 || meta.small() != 300 || !meta.flag()) throw new AssertionError("annotation values");
  if (value.getClass().getAnnotation(ann.ClassMark.class) != null) throw new AssertionError("CLASS retention");
  if (value.getClass().getAnnotation(ann.SourceMark.class) != null) throw new AssertionError("SOURCE retention");
  if (!value.getClass().getMethod("greet").getAnnotation(Deprecated.class).forRemoval()) throw new AssertionError("export annotation");
  if (value.getClass().getMethod("$greet").getAnnotation(Deprecated.class) != null) throw new AssertionError("duplicate annotation on implementation");
  if (demo.Annotated.class.getMethod("make").getAnnotation(Deprecated.class) == null) throw new AssertionError("function annotation");
  if (demo.Annotated.callback().getClass().getMethod("run").getAnnotation(Deprecated.class) == null) throw new AssertionError("callback annotation");
  var contract = Class.forName("demo.Annotated$Interface$0");
  if (contract.getAnnotation(Deprecated.class) == null || contract.getMethod("$name").getAnnotation(Deprecated.class) == null || contract.getMethod("$greet").getAnnotation(Deprecated.class) == null) throw new AssertionError("interface annotations");
  System.out.println("annotations verified");
 }
}"#).unwrap();
    let classpath = temporary.join("annotated.jar");
    let packaged = Command::new("jar")
        .args(["--create", "--file"])
        .arg(&classpath)
        .arg("-C")
        .arg(&temporary)
        .arg("ann")
        .arg("-C")
        .arg(&output)
        .arg("demo")
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "{}",
        String::from_utf8_lossy(&packaged.stderr)
    );
    let compiled = Command::new("javac")
        .args(["-encoding", "UTF-8"])
        .arg("-cp")
        .arg(&classpath)
        .arg("-d")
        .arg(&temporary)
        .arg(caller)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let runtime = std::env::join_paths([temporary.as_path(), classpath.as_path()]).unwrap();
    let ran = Command::new("java")
        .args(["-Xverify:all", "-cp"])
        .arg(&runtime)
        .arg("Caller")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout).trim(),
        "annotations verified"
    );
    let dump = Command::new("javap")
        .arg("-v")
        .arg("-cp")
        .arg(&output)
        .arg("demo.Annotated$$0")
        .output()
        .unwrap();
    assert!(
        dump.status.success(),
        "{}",
        String::from_utf8_lossy(&dump.stderr)
    );
    let dump = String::from_utf8_lossy(&dump.stdout);
    assert!(dump.contains("RuntimeInvisibleAnnotations:"));
    assert!(
        dump.split("RuntimeInvisibleAnnotations:")
            .nth(1)
            .unwrap()
            .contains("ann.ClassMark")
    );
    assert!(
        !dump
            .split("RuntimeVisibleAnnotations:")
            .skip(1)
            .any(|section| section.contains("ann.SourceMark"))
    );
    // Check errors against the same real annotation classes.
    let fields = Command::new("javap")
        .args(["-v", "-p", "-cp"])
        .arg(&output)
        .arg("demo.Annotated$$2")
        .output()
        .unwrap();
    assert!(
        fields.status.success(),
        "{}",
        String::from_utf8_lossy(&fields.stderr)
    );
    let fields = String::from_utf8_lossy(&fields.stdout);
    assert!(fields.contains("RuntimeInvisibleAnnotations:"));
    assert!(
        fields
            .split("RuntimeInvisibleAnnotations:")
            .nth(1)
            .unwrap()
            .contains("ann.ClassMark")
    );
    assert!(
        !fields
            .split("RuntimeInvisibleAnnotations:")
            .skip(1)
            .any(|section| section.contains("ann.SourceMark"))
    );

    for (text, expected) in [
        (
            "import ann.Inject\n@Inject(method = \"tick\", at = @Deprecated)\nfun bad() {}",
            "This nested annotation does not match",
        ),
        (
            "import ann.Inject\nimport ann.At\n@Inject(method = \"tick\", at = @At)\nfun bad() {}",
            "Provide every required annotation element",
        ),
        (
            "import ann.Inject\nimport ann.At\n@Inject(method = \"tick\", at = @At(nope = \"HEAD\"))\nfun bad() {}",
            "This annotation has no element",
        ),
        (
            "import ann.Inject\nimport ann.At\n@Inject(method = \"tick\", at = @At(value = \"HEAD\", value = \"TAIL\"))\nfun bad() {}",
            "Give each annotation argument only once",
        ),
        (
            "import ann.Inject\nimport ann.At\n@Inject(method = \"tick\", at = [@At(value = \"HEAD\"), @Deprecated])\nfun bad() {}",
            "This nested annotation does not match",
        ),
        (
            "import ann.At\n@At(value = \"HEAD\")\nclass Bad {}",
            "This Java annotation is not allowed",
        ),
        (
            "import ann.At\n@Deprecated(since = @At(value = \"HEAD\"))\nclass Bad {}",
            "This nested annotation does not match",
        ),
        (
            "import ann.Inject\nimport ann.At\n@Inject(method = \"tick\", at = [[@At(value = \"HEAD\")]])\nfun bad() {}",
            "Use a constant annotation value",
        ),
        (
            "import ann.Meta\n@Meta\nclass Bad {}",
            "Provide every required annotation element",
        ),
        (
            "import ann.Meta\n@Meta(value = null)\nclass Bad {}",
            "Use a constant annotation value",
        ),
        (
            "@SuppressWarnings(value = [42])\nfun bad() {}",
            "Use a constant annotation value",
        ),
    ] {
        fs::write(&source, text).unwrap();
        let rejected = Command::new(env!("CARGO_BIN_EXE_mammoth"))
            .env("CLASSPATH", &temporary)
            .arg(&source)
            .arg(&output)
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
    fs::remove_dir_all(&temporary).unwrap();
}
