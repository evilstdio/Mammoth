# Mammoth development plan

Mammoth is a statically typed JVM language for first-time programmers and developers building larger applications. Its compiler is written in Rust.

## Learning path

The first program should fit on one line and run with one command. Use inferred types for local values and explicit types where a function defines what callers can pass or receive. Prefer familiar words and consistent syntax for declarations, blocks, and calls. Introduce classes when a program needs to keep state and behavior together. Test the examples and error messages with new programmers as the language grows.


## Language and compiler milestones

1. Compile top-level `print("Hello, world!")` directly into a runnable JVM class. Establish source locations and a repeatable Java verification command.
2. Add text interpolation, numbers, booleans, local type inference, typed functions, conditions, and loops. Define evaluation order, lexical scope, and numeric behavior before adding each feature. Use beginner-readable diagnostics with source highlights.
3. Add modules, project builds, and Java interop. Specify how Mammoth represents primitives, lists, and nullable values. Define how calls to overloaded Java methods resolve and how values with unknown nullability enter Mammoth.
4. Add the standard library, generics, user-defined types, packaging, and editor support. Run compatibility tests against previous releases once the language stabilizes.

The compiler pipeline will be source → parsing → type checking → JVM code generation. Keep the first pipeline small and split stages as type checking and control flow become necessary. Generate class files directly and use Java's runtime and libraries where their behavior matches the language design.

## Performance gates

- Compare release builds of Mammoth with equivalent Java and Kotlin programs on the same JDK and machine. Record tool versions, inputs, output checks, repeated timings, and spread.
- Measure clean compilation and rebuilds after a one-line edit separately. Distinguish command startup from steady-state builds. Add a dependency graph and cached compilation only after profiling real multi-file projects.
- Measure cold process startup, warmed throughput, and memory separately. Include file processing and allocation-heavy workloads. Verify equivalent output and compare bytecode and profiles before changing code generation.
- Test the first lesson with new programmers. Observe where they stall, how they interpret compiler errors, and whether they can fix mistakes without help.


The compiler runs the hello, greeting, arithmetic, and checksum examples on Java 17. It supports Text, Int, Bool, typed parameters, integer arithmetic, conditional blocks, read-only and mutable bindings, and while loops. The checksum example matches an equivalent Java program. Next, build a repeatable Java and Kotlin comparison for the checksum, with separate measurements for compilation, process startup, and warmed execution. The checksum now runs in a function with a typed return value, keeping the calculation separate from printing. Mammoth now also supports classes with private fields and instance methods. Each object has independent state. Constructors accept typed arguments, and functions and methods can take and return Mammoth objects. Interfaces support shared method contracts and JVM dispatch across classes, with optional type annotations on local bindings. Mammoth can declare public Java static methods on classes and call an exact overload using Text, Int, Bool, or a void result. The runner loads external libraries through CLASSPATH. Java class bindings now support a public constructor, public instance methods, and Java object types in signatures. Nullable reference types support Java results and arguments, Mammoth bindings, and explicit null checks. Next, validate Java declarations at compile time. Null checks now narrow read-only bindings and parameters in checked branches, while bodies, and after a null-handling early return. Comparison expressions, short-circuit Boolean operators, and reusable collections remain part of the core language work.









## JVM host integration

Named modules, exported functions, and JAR packaging with resources work. A Java caller calls an exported Mammoth function, and Fabric Loader 0.16.14's language adapter invokes the initialization method from the packaged example. Next, launch a Minecraft test instance and add a client-side example using a Fabric callback and keybind. Then use the same JVM export path to test another Java host application.

Java interface bindings allow Mammoth classes to receive callbacks from Java. Verified both the JDK IntConsumer callback and callbacks from a separate Java class, including nullable arguments. Java static field reads, aliases for Java instance methods, and reference conversion to java.lang.Object support Fabric event registration. The Minecraft 1.20.1 client example registers a G keybind and handles simulated presses with the official Fabric Loader and API. A real game launch and in-world keypress remain to be verified. The --check-java command validates foreign classes and members against the JDK and project classpath before execution. The checksum benchmark records compiler launches, JVM startup, and warmed throughput for Mammoth, Java, and Kotlin. Comparisons produce Bool values, and and/or/not provide short-circuit logic and carry null-check facts through conditions. Reusable fixed-size lists support Text, Int, and Bool with indexing, size, function signatures, and for loops. Double provides fractional arithmetic, unboxed lists, and Java double interoperability. Multi-file projects now run and package named modules with explicit imports and resources. Fixed-size lists can now hold class and interface objects, including Java objects. MutableList adds growable collections with Java-backed storage, including object and primitive element types. The VS Code extension provides Mammoth syntax highlighting, Run File, Java declaration checking, and compiler diagnostics. Next, add broader performance workloads.

## Language completeness (November 2026)

The core language now covers what most beginner and intermediate programs need: control flow with `else if`, `when`, ranges, `break`, and `continue`; `Long`, `Char`, and automatic numeric widening; text operations; classes with public fields, inheritance, abstract classes, enums, and data classes; lambdas and function types; exceptions; and null-safety operators. See CHANGELOG.md for the full list.

Java interoperability no longer needs declarations. The compiler reads class files from the JDK image and the classpath, resolves overloads like Java, understands generic signatures, implements Java interfaces with bridge methods, converts lambdas with `invokedynamic`, and lets Mammoth classes extend Java classes. Explicit `java fun` and `java class` bindings remain for renamed members such as Minecraft's intermediary names.

Next steps, in priority order:

1. Language support for JVM integrations: Java annotations, predictable exported members, and interoperability required by a complete Minecraft mod. Class instance methods now support `export fun` with ordinary public JVM method names.
2. Expand the working Fabric demo with production-profile, saved-world, physical-input, and dedicated-server checks. The initial item/keybind/event/Mixin smoke test now passes in the real Minecraft 1.21.1 development client.
3. Continue type-system correctness, including variance in method bounds, use-site projections, and a design for primitive generic lists. Class override/interface-implementation bounds are now checked, and bounded generic member calls carry receiver substitutions.

Editor expansion and general release packaging are deferred while language and Minecraft compatibility work take priority.

The first complete Fabric development mod now builds and launches successfully in Minecraft 1.21.1. Item registration, client event handling, a simulated G-key press, and a real Mixin injection were observed in the actual client with strict JVM verification. A remapped-JAR inspection checks entry points/resources/Mixin targets. See `examples/fabric-demo/validation`. Next application checks: normal production-profile launch, item appearance/use in a world, physical controls, and dedicated-server behavior. Field annotations, constructor-property annotations, and protected Mammoth fields are now implemented; the real mod also reads mutable/final Shadow fields and keeps Unique state. Next language work includes protected methods, direct Java protected-field access, and source/line metadata, guided by real examples. Broad Minecraft/Mixin parity is not established by this demo.

Java declaration annotations are now implemented for classes, interfaces, functions, methods, and fields, with retention/target/value checks and Java-facing placement. Nested values and arrays of annotations now work, including singleton array shorthand and nested defaults. Field/constructor-property annotations and protected fields have now been checked with Java reflection and cross-package verification, and Shadow/Unique/Final fields passed the real-client smoke test. Production-profile and broader initializer validation remain. Do not describe annotation support alone as complete Mixin support.

Nullable primitive types now use boxed Java values, including map lookups and the numeric `toIntOrNull`, `toLongOrNull`, and `toDoubleOrNull` text methods. Mutable lambda captures also work through shared cells.

Classes and interfaces now cross project module boundaries with shared type identities and separate source scopes. Exported signatures retain their declaring module's types, including objects, Java generics, nullable values, lists, and functions. Project tests package and verify JARs, check resources and Java callbacks, and reject invalid cross-file accesses and inheritance cycles.

Top-level generic functions now support inferred or explicit type arguments, nullable parameters/results, collection helpers, higher-order functions, recursion, and project imports. Exported functions include JVM generic signatures and are tested with a real Java caller.

Generic classes now support inferred constructors, typed fields and methods, generic data classes, nested applications, nullable values, closures, and project imports. JVM signatures preserve class and member type arguments for Java callers. Each declaration produces one erased JVM class; ordinary primitive code keeps its existing representation.

Methods now introduce their own type parameters, with inference from arguments and results, overrides, `super` calls, safe calls, and interface dispatch. Class and interface generic method metadata is tested with Java callers. Interfaces with type parameters are the next type-system milestone.


Parameterized interfaces, generic superclasses, function types with any number of parameters, and single type-parameter bounds are now implemented. Implementations whose descriptors differ from an erased interface or superclass method receive JVM bridge methods. Bounded parameters erase to their bound, as Java does. Java tests implement Mammoth generic interfaces, call inherited generic members, and pass lambdas to generated function interfaces. The next type-system work is the fixed-size generic `List<T>` design, multiple bounds, and variance.

Interfaces can now extend other interfaces, with substituted inherited methods, ancestor subtyping, inference through ancestors, consistency checks, JVM superinterfaces, and Java interop tests. The next type-system work is several bounds per parameter, declaration-site variance, and the fixed-size generic `List<T>` design.

Several bounds per type parameter (`T: A & B`), declaration-site variance for interfaces (`out T`, `in T`), trailing lambdas on bare function calls, implied runtime checks such as `source is Box<Int>` for a `Source<Int>`, and default interface methods (including lambdas inside them) are now implemented. Fixed-size generic `List<T>` now works without copying: reference-element lists only, and no `List<T>` creation inside generic code. Star projections (`Box<*>`) are implemented, including `value is Box<*>` checks from `Any`. Mammoth interfaces can now extend Java interfaces.

Child interfaces now override inherited Mammoth methods with matching signatures and bodies. `super<Interface>.method()` calls a direct parent's default, including generic defaults, from classes, interface bodies, and their lambdas. Default resolution selects the most specific declaration, and unrelated conflicts require an override. Specialized generic interface defaults include the boxed entries and bridges needed for both Java and Mammoth overrides to dispatch correctly. Tests cover strict verification, primitive values, cross-module JARs, and Java callers.

Generated class files now carry SourceFile and LineNumberTable data (real `.mam` stack traces; Mixin nondefault Unique initializers verified in game). Protected methods and access to Java superclasses' public/protected fields are implemented with JVM receiver rules. The Fabric demo gained a Mammoth `Item` subclass. Next: generic Java superclasses.
 Generic Java superclasses (`extends ArrayList<Text>()`) are now supported.
 Java generic call arguments are now checked against type arguments.
 Methods now carry LocalVariableTable entries for debuggers.
 Classes support static members; a static Mixin handler and static Unique field were verified in game.
 Static init blocks and inferred static field types are supported; the Fabric demo uses a Mammoth @Accessor/@Invoker interface in game.
 Nested Java class imports and Double-to-float arguments work; the Fabric demo registers a real custom block.

## Production-readiness queue (highest priority first)

Work in correctness milestones with reproductions, valid-program coverage, rejection tests, and full release checks before delivery. This queue supersedes older "next" suggestions above.

1. **Java generic type safety (in progress).** Completed: invariant assignment to parameterized Java destinations, inherited views, raw-to-parameterized rejection, contextual Java constructors, and nested erased-argument checks. Completed next milestone: original Java method-parameter validation, wildcard upper/lower direction and nested invariance, plus spread varargs checks. Completed next milestone: inferred method bounds, ancestor-based inference, method-variable shadowing, boxed recursive bounds and bounded native-to-Java forwarding. Completed next milestone: per-variable lower/exact/upper constraint collection, deterministic common-parent candidates, invariant anchors, and accessible Java supertypes. Completed next milestone: bounded dependent/nested constraint propagation and independent/self-bound candidate filtering. Completed next milestone: expected-result constraints for Java static/direct instance calls and result ancestor views. Completed next milestone: unique-signature fixed-parameter hints for nested calls and direct-SAM lambda result constraints. Completed next milestone: explicit lambda input constraints, safe parameter variance and inherited/reordered SAM substitution. Completed next milestone: source Java class/interface argument arity and deferred bound checking. Completed next milestone: generic-aware overload applicability filtering and explicit lambda parameter candidate checks. Completed next milestone: erased parameter specificity among equal-ranked candidates and explicit ambiguity diagnostics. Completed next milestone: cloned-state literal lambda body/result applicability checks across overloads. Completed next milestone: candidate-specific nested re-probing, spread varargs element context, original-signature wildcard parameter checks, per-site wildcard captures for Java results/fields (read-only upper, write-only lower) and typed generic Java field reads. Completed next milestone: expected-result context through inherited, implicit, `this`/`super` and safe Java calls; contextual `super.` nullability; inherited Java static methods. Next: full native implemented-interface candidates, lambda constraints, full wildcard capture/read types, remaining boxed primitive paths, and explicit cast policy. Avoid declaring this item complete until the interactions are audited.
2. **Nullability and conversion audit.** Completed 2026-11-08: checked parameterized Java casts, initialization-order forward reads, boxed parameter and alias audit (see HANDOFF). Boxed Java Integer/Long/Double/Boolean/Character results and type arguments now map to Mammoth numbers. Remaining: Java array element nullability, indirect this-escape. Check Java boundary guarantees, mutable aliases, field reads/writes, generic nullable primitives and conversions; distinguish deliberately unchecked Java/raw operations from compiler guarantees.
3. **Bytecode and compiler robustness.** Progress 2026-11-08: incremental LocalVariableTable sync (cubic compile time fixed), truncation fuzz test, mutation fuzz runs clean, block-comment diagnostic. Remaining: type-directed semantic fuzzing, classfile fuzzing. Exercise control-flow joins, exceptions, lambdas, initialization and JVM limits together. Add bounded malformed-source/classfile fuzzing and ensure clean errors rather than crashes, hangs or invalid bytecode.
4. **Written semantic contract.** Specify actual numeric, evaluation-order, initialization, visibility, exception, generic and Java boundary rules; resolve inconsistencies with the tests and implementation.
5. **Real application validation.** Maintain substantial programs. For Minecraft validate world interactions, remapped production loading and dedicated-server behavior; keep development-client evidence separate.
6. **Repeatable supported builds.** Automated clean builds/tests on documented OS/JDK combinations, dependency pinning, reproducible failure reports and regression gates.
7. **Measured performance.** Establish compilation/startup/runtime/memory baselines and Java/Kotlin comparisons; optimize measured bottlenecks.
8. **Release contract and distribution.** Define supported features and compatibility, install/version reliably, publish limitations/documentation/licensing and bug-report guidance before a stable release.

New syntax is considered when a real application needs it; it does not replace the correctness work above.

## Correctness work, November 2026 (session 3)

The compiler now has three independent checks beyond its example tests: a differential fuzzer against javac, a soundness fuzzer that feeds it ill-typed programs, and hand-written adversarial batteries (see `tests/fuzz`). They found and fixed invalid bytecode from untyped branch values and stale shared-variable slots, null passing through `as`, unchecked `MutableList` casts and map keys, unchecked type arguments on Java interfaces, instance-field writes from static code, and reads of top-level variables before their declaration. Java overloads are now chosen in javac's phases.

Open correctness work, in order:

1. Consider this-escape from initializers.
2. Element checks for arrays passed through generic functions.
3. Wildcard types written in Mammoth source, so conservatively rejected wildcard programs can be expressed.
4. Constructing `java.lang.String` from `byte[]` or `char[]`, and `goto_w` for branches over 32 KB.
5. Extend the fuzzers to multi-file projects and Java subclasses of Mammoth classes.

This work does not establish full Java or Kotlin compatibility, and no performance claims follow from it.
