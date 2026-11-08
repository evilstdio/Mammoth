# Changelog

## Java call forms, 2026-11-08

- Java calls accept explicit type arguments, such as `Collections.emptyList<Text>()`, and check arguments against them.
- A lambda for an overloaded Java method prefers the parameter that returns a value, as in javac.
- `for` works over Mammoth classes implementing `Iterable`.
- Mammoth enums satisfy `Enum<E>` bounds, so `EnumMap` works.
- Java lambda inputs can be declared nullable, as in `{ k: Text, v: Int? -> ... }`.

## Initialization order and control flow, 2026-11-08

- A top-level statement that reaches a later top-level variable through function calls or constructors is rejected.
- `while true` without `break` no longer needs code after it.
- Comparing unrelated classes, and a `catch` that an earlier `catch` already covers, are compile errors.

## Casts, maps, arrays and overloads, 2026-11-08

- `as` to a non-null type now rejects null at runtime.
- `as MutableList<T>` must be implied by the value's static type.
- Map keys in `map[key]` and `map[key] = value` are checked against the map's key type.
- Reading a non-null reference from an array checks for null, so null elements from Java arrays fail at the read.
- Java overloads are chosen in javac's phases (subtyping, boxing, variable arity), then by specificity. Calls javac reports as ambiguous are now rejected instead of picking by conversion cost.
- Stack traces keep the correct line for statements that start with a speculative check.
- A line break now ends a call or index, so a `when` branch can start with `(` or `[`.

## Verifier fixes from fuzzing, 2026-11-08

- A `when` or `if` value without an expected type, nested inside a call, now keeps earlier operands in its stack frames. Such programs previously failed JVM verification.
- A local variable that reuses the slot of an out-of-scope variable shared with a lambda is no longer treated as shared.

## Exhaustive when over enums, 2026-11-08

- A `when` value without `else` is accepted when its non-null enum subject (Mammoth `enum_values`, or a Java class with `ACC_ENUM` and its enum constant fields) is covered by `Type.CONSTANT` branch values. The fall-through path throws `IncompatibleClassChangeError`.
- Added the `when-enum-exhaustive` program and 2 rejection fixtures. The existing else-required message keeps its prefix.

## Assigning Java fields, 2026-11-08

- `object.field = value` for public fields of Java objects, and `Type.FIELD = value` for public static Java fields. Values are checked against the field's generic type (wildcards flattened conservatively). Byte, Short and Float fields convert as for inherited fields, final fields are rejected, and compound operators ask for the full form. Before, these reported "Only fields of Mammoth objects can be changed here".
- Added the `java-field-writes` program and 3 rejection fixtures.

## Class literals in expressions, 2026-11-08

- `Type.class` is now an expression of type `Class<Type>`, using `ldc` for classes and the box's `TYPE` field for `Int`/`Long`/`Double`/`Bool`/`Char`. This lets Mammoth code call `EnumMap(Class)`, `EnumSet.allOf`, and similar APIs. Before, class literals were only valid in annotation values. Type parameters, parameterized types and function types are rejected.
- Added the `class-literals` program and 2 rejection fixtures.

## Inherited Java static fields, 2026-11-08

- Static fields of a Java superclass or of its interfaces can be read by bare name inside a Mammoth subclass, including protected ones, and through the subclass name from anywhere (public fields only). Generic static fields keep their type arguments, and wildcards are captured. Before, `MAX_PRIORITY` in a `Thread` subclass reported "This variable has not been defined here".
- Added the `java-inherited-static-fields` program, Java integration checks, and 2 rejection cases.

## Java interface defaults on Mammoth objects; Fabric re-validation, 2026-11-08

- **Java defaults.** Calls such as `Increment().andThen(...)`, `compose`, `Comparator.reversed()` and `thenComparing` on Mammoth classes implementing Java interfaces now go through the substituted Java interface view. Before, they reported "This type has no method with that name". Methods found on a Java superclass chain keep their existing path.
- **Dispatch matrix.** Added the `dispatch-matrix` program: overrides at several levels of generic superclasses and interfaces specialized to Int, called through every static type.
- **Fabric demo re-validated.** `build`, `verifyMod` and the real Minecraft client `smokeTest` passed with this compiler, with every marker present. The new JAR SHA256 is recorded in `examples/fabric-demo/validation/README.md`.
- **Tests.** Added the `java-interface-defaults` and `dispatch-matrix` programs and one rejection fixture.

## Overrides of inherited default methods, and a shop example, 2026-11-08

- **Silent wrong dispatch fixed.** A class overriding a generic interface default method that its *superclass* lists, such as `Fancy extends Plain` with `Plain implements Named<Int>` and `override fun tag(value: Int)`, now gets the bridge for the erased interface method. Before, it compiled but calls through `Named<Int>` or `Plain` silently ran the default instead of the override.
- **`super.` to inherited defaults.** `super.greet()` now calls a default method that the superclass inherits from an interface without overriding it, including through generic interfaces and several class levels. It binds statically (`invokespecial` on the superclass), so the override is not re-entered. Before, it reported "The superclass has no method with that name". Generic default methods called this way are reported as not supported yet.
- **Real program.** Added `examples/shop.mam` (inventory, orders, generics, statics, streams, Comparator sorting, enum grouping, exceptions) with expected output. Writing it found the `super.` gap, and that led to the dispatch bug.
- **Tests.** Added the `inherited-defaults` program and the `super-missing-default` fixture.

## Calling call results, 2026-11-08

- `f(x)(y)`, `list.get(0)(y)` and `table[0](y)` call the function value the call or index gives (parsed as `.invoke`). The `(` must directly follow `)` or `]`, so a next-line parenthesized expression keeps its old parse; this was compared against the previous compiler. Calling a number reports "This value is not a function, so it cannot be called.".
- Added the `call-results` program and 2 rejection fixtures.

## Nullable function types and a calculator example, 2026-11-08

- **Nullable function types.** `((A) -> B)?` is now valid type syntax. Before, it reported "Expected '->' and a result type", so a function from `HashMap.get` could not be held as nullable. Parentheses around a function type also work without `?`.
- **`requireValue` on narrowed numbers.** After a null check narrows `Double?` to `Double`, `requireValue(value)` is accepted. Before, it failed with "requireValue expects a value with a known type".
- **Real program.** Added `examples/calculator.mam` (tokenizer, recursive-descent parser, evaluator with variables, functions, data classes, enums, interfaces, exceptions and Java collections), with expected output. Writing it found both issues above.
- **Tests.** Added the `nullable-functions` program and 2 rejection fixtures.

## Generic-aware overload specificity, 2026-11-08

- Among equally ranked Java overloads, one candidate is more specific than another only if its generic parameter types also fit the other's original signature. `genericTie(null)` with `List<String>` and `ArrayList<Integer>` is now ambiguous, matching javac; before, it silently picked `ArrayList<Integer>`. Agreeing type arguments (`ArrayList<String>` over `List<String>`) and method type variables keep the previous result, checked against javac.

## Java number boxes as Mammoth numbers, 2026-11-08

- **Boxed results.** Java results and fields declared as `Integer`, `Long`, `Double`, `Boolean` or `Character` are Mammoth `Int`, `Long`, `Double`, `Bool` or `Char` (nullable where the destination accepts null). Before, they were Java objects: `Integer.valueOf(7) + 1` and `boxed == 7` were rejected.
- **Boxed type arguments.** Java generic signatures map these boxes to the same Mammoth types. A Java `List<Integer>` result can be assigned to `JavaList<Int>` and added to an `ArrayList<Int>`, and `Map<String, Integer>` is `JavaMap<Text, Int>`. A `List<Integer>` still is not a `List<Long>`.
- **Box methods on primitives.** `5.compareTo(3)`, `hashCode()`, `isNaN()` and other methods of the Java box can be called on Mammoth numbers when Mammoth has no built-in method of that name.
- **Tests.** Added the `java-boxed-numbers` program and 2 rejection fixtures. One integration message changed from "Expected an object of the declared class here." to the clearer "Expected an Int here.".

## Compiler robustness: debug-table performance, fuzzing, diagnostics, 2026-11-08

- **Cubic compile time fixed.** LocalVariableTable synchronization compared every live variable with every table entry on every statement. A method with 8,000 locals took 52 s to compile, and 70,000 timed out. It is now incremental: open entries are tracked alongside the binding stack, with a low-water mark. 8,000 locals take 64 ms, and 70,000 fail at once with the method-size error. The class files of all 108 test programs and examples are byte-identical before and after.
- **Lambdas for erased generic parameters.** A lambda passed to a Java parameter erased to `Object` (`ArrayList<() -> Int>.add`, `ArrayList<Runnable>.add`, `HashMap.put` of a function) compiles against the parameter's generic type, ranked after real functional interfaces. Before, it reported "No version of this Java method accepts these argument types".
- **Diagnostics.** `/* ... */` reports that Mammoth comments start with `//`, instead of "Expected a name".
- **Fuzzing.** A new `truncated_programs_fail_cleanly` test cuts every program and example at 7 points and requires a clean error, with no panic or hang. Separate seeded mutation runs (3 × 2,725 truncated or token-mutated programs, compiled and run under `-Xverify:all`) found no panics, hangs, or invalid bytecode.
- **Checked and already handled.** Nesting beyond 64 levels, the method-size limit, string constants over 65,535 bytes, integers out of range, unterminated strings, invalid UTF-8 and empty files all fail cleanly. A stress program combining try/catch/finally with `break`/`continue`, lambda captures in loops, generic methods with `finally`, and returns from `when` verifies and gives the expected output.
- **Tests.** Added the `java-lambda-elements` program, 2 rejection fixtures, and the truncation test.

## Checked Java generic casts and initialization order, 2026-11-08

- **Generic casts.** `as`, `as?` and `is` with a parameterized Java type now require the value's static type to determine every type argument: an agreeing upcast, or a downcast whose arguments follow through the class's generic supertypes (worked out with temporary marker parameters). Before, `anything as ArrayList<Text>` compiled and failed later with ClassCastException, and `is JavaList<Text>` was true for a list of numbers. Raw casts stay available.
- **Forward reads in initializers.** Instance and static field initializers and `init` blocks may no longer read fields whose starting value has not run yet, whether by bare name, `this.`, or the class name. Before, objects read null and numbers silently read 0 (`let a: Int = b + 1` gave 1). Lambda bodies are exempt.
- **Audit notes.** These were checked and left as is:
  - Boxed Java parameters (`Long`, `Double`, `Short`, `Float`) reject mismatched Mammoth numbers, as Java does.
  - Nullable values cannot reach primitive Java parameters.
  - Typed generic methods reject mixing `Dog` and `Cat` lists.
  - Raw Java parameters and `Object[]` covariance remain unchecked, as in Java.
  - Java arrays containing null are not checked element by element (documented).
- **Tests.** Added the `initialization-order` program and 9 rejection fixtures.

## Expected types through inherited, super and safe Java calls, 2026-11-08

- **Expected result types.** Declared destination types now reach generic Java methods called on a Mammoth subclass (`Sub().contextualEmpty()`), unqualified inside it, through `this.` and `super.`, and through `?.` on Java or Mammoth receivers. Before, these failed with "Java generic type arguments must match the declared type". Declared bounds still apply, and each nested argument gets only its own parameter's context.
- **Java `super.` results.** A Java `super.` call's result is null-checked like other Java results unless the destination accepts null. Before, it was always nullable, so it could not be returned as a non-null value.
- **Inherited static methods.** A Java superclass's static methods, including protected ones, can be called by their bare name inside a Mammoth subclass. Static methods of a class's superclasses can also be called through the subclass name. Interface static methods are still not inherited.
- **Tests.** Added Java integration checks for inherited, implicit, `this`, `super`, safe and null-safe contexts, and for inherited statics, plus 6 rejection cases (bounds through each path, wrong static arguments, protected static access from outside).

## Candidate-specific nested inference and Java wildcard capture, 2026-11-08

- **Nested overload context.** Each applicable Java overload re-probes nested calls, list literals and conditionals with its own parameter types before generic filtering. Only same-erasure refinements are kept. Covers static, instance and constructor overloads, reversed declaration order, and deep nesting through overloaded factories. Re-probing is gated to parameterized parameters and bounded to four nested levels.
- **Target-dependent varargs.** Once a candidate is known to spread, its tail elements get the component's generic type as context and are checked against it, so `spread("a", ArrayList<Int>())` against `List<String>...` is rejected.
- **Wildcard parameters.** After emission, an argument for a wildcard parameter is checked against the original signature, not its flattened view. `List<? super Integer>` now accepts `ArrayList<Int>` and `ArrayList<Number>`, and still rejects `ArrayList<Text>` and `ArrayList<Long>`.
- **Wildcard capture.** Java results and field reads with wildcard type arguments get per-site hidden capture types. Upper-bounded captures read as their bound and accept no writes. Lower-bounded captures accept their bound and read as `Any`. Before, `List<? extends Number>` was treated as a mutable `List<Number>`, so `add(2.5)` or `add(BigInteger)` compiled and later failed in Java with ClassCastException.
- **Generic Java fields.** Public generic Java fields read statically or from Java objects keep their type arguments. Before, they were raw, and a `List<Integer>` field accepted `add(2.5)`.
- **Escape guard.** Lambdas with an inferred result cannot return a capture type, because each call would share one capture.
- **Tests.** Added the `java-overload-wildcards` program, 2 rejection fixtures, and Java integration cases: 13 valid checks and 19 rejections.

## Lambda-body Java overload selection, 2026-11-08

- Check literal lambda bodies/results against surviving Java overload candidates before final specificity selection.
- Use isolated cloned emitter/method bytecode state so failed trials cannot leave captured-local boxing, frames, helpers or exception state in the selected call.
- Preserve ambiguity when several unrelated targets still accept the lambda. Prefer relevant body diagnostics when every candidate fails.
- Add Java integration regressions for result/input-body selection, declaration order, runtime side effects occurring once, loop/finally captures and invalid/ambiguous bodies.


## Java overload specificity and ambiguous calls, 2026-11-08

- Resolve equal-ranked applicable overloads using parameter specificity instead of declaration order. Cover reference inheritance, array relationships, primitive widening and expanded varargs components.
- Reject unrelated or crossed equally good candidates with an explicit ambiguity error. Explicit argument types can disambiguate calls.
- Retain generic applicability filtering and the existing conversion ranking; fixed invocation wins a tie with spread invocation.
- Added Java integration coverage for reference/null/array/numeric/constructor/zero-varargs selection, ambiguous interfaces and arrays, and explicit disambiguation. Full javac specificity and lambda-body candidate retry remain incomplete.


## Generic-aware Java overload applicability, 2026-11-08

- Filter erased-compatible overloads using generic argument/result constraints, receiver substitutions, declared bounds and explicit lambda parameter compatibility before ranking.
- Reuse inference validation without emitting arguments or probing lambda bodies during candidate filtering. Preserve the single-candidate path and existing fixed/varargs conversion ranking.
- Add verified constructor, static/inherited method, bound-fallback, varargs and annotated-lambda overload regressions plus all-invalid cases.
- Box compatible primitive Java lambda results for concrete boxed/Number SAM result signatures. Reject incompatible results.
- Equal-ranked specificity, ambiguous overload diagnostics and full lambda-body candidate retry remain incomplete.


## Java class type-argument validation, 2026-11-08

- Reject the wrong number of Java class/interface type arguments, including parameterized nongeneric classes.
- Validate source-written Java applications against every declared bound after native declarations are resolved. Covers dependent, multiple and recursive bounds in nested types, signatures, constructors and Java superclasses.
- Reject EnumSet<Text> while accepting declared Java enum arguments, boxed numeric arguments and appropriately bounded Mammoth type parameters.
- Added declaration/rejection fixtures and compiled-Java integration regressions. Raw APIs and unsupported Java wildcard syntax are unchanged.


## Lambda input safety and inherited interfaces, 2026-11-08

- Reject lambda parameter annotations narrower than their typed target input, fixing a compiler-accepted Optional.map runtime cast failure. Compatible broader parameters remain valid.
- Infer Java method input variables from explicit lambda annotations and preserve known Any arguments rather than treating them as unresolved/raw.
- Substitute inherited/reordered Java functional-interface arguments when inferring lambda results.
- Added rejected narrowing cases, runnable parameter-variance coverage and compiled-Java inherited-SAM/input-inference regressions. Deliberately raw Java interfaces retain unchecked behavior.


## Nested Java context and lambda result inference, 2026-11-08

- Supply fixed-parameter context while probing a unique Java signature, enabling nested typed empty factories without speculative overload selection.
- Infer Java method variables from supported functional-interface lambda return bodies, retaining bounds and exact collection constraints.
- Aggregate results from multiple lambdas and ordinary arguments. Preserve enclosing compiler state while probing; regression checks cover runtime side effects occurring once.
- Add nested factory, Optional/Stream mapping, bounded Supplier and common-parent lambda regressions. General overload, lambda-input and inherited-SAM inference remain incomplete.


## Contextual Java method inference, 2026-11-08

- Use declared destination/return/parameter types to constrain Java static and direct instance generic calls.
- Resolve generic result ancestor views symbolically, including List<T> to Collection<Text>.
- Keep input constraints, invariance and declared bounds enforced; context cannot force an incompatible call to compile.
- Add valid factory/return/argument regressions and invalid generic/bounded destination checks. Nested target-dependent calls, inherited/safe dispatch and lambda constraints remain incomplete.


## Bound-aware Java inference, 2026-11-08

- Propagate dependent and nested Java method bounds back into inference, including multi-variable chains and previously unconstrained variables.
- Filter candidates by independent/self-recursive bounds before selection; retain final checks for all arguments and bounds.
- Bound propagation has a finite convergence limit. Nonconverging cases fail cleanly.
- Add Java integration coverage for argument order, nested inference, bounded common supertypes and invariant anchors. Correct an earlier test that rejected a legal common-parent inference.


## Java inference across arguments, 2026-11-08

- Collect lower, exact and upper constraints before inferring Java method variables. Infer common parent candidates for mixed subtypes while preserving concrete collection invariance.
- Make candidate selection independent of argument order, retain generic ancestor substitutions, and exclude inaccessible Java supertypes.
- Avoid hierarchy searches for equality-anchored and single-type calls.
- Added common-parent/argument-order programs and Java integration coverage for mutable collection anchors. Full intersection, contextual/lambda and bound-propagating inference remain incomplete.


## Java method bounds and inherited inference, 2026-11-07

- Retain and enforce inferred Java method type-parameter bounds, including multiple, recursive and dependent bounds.
- Correct method-variable shadowing and infer generic arguments through the declaring ancestor's substituted signature.
- Support bounded Mammoth generic arguments at Java reference calls, including secondary bounds and boxed primitive Comparable checks.
- Fix false duplicate-interface errors for classes and interfaces naming different Java interfaces; retain rejection of genuine duplicates.
- Expanded verified Java integration regressions and added runnable/rejected JDK-bound fixtures. Full constraint-based inference remains unfinished.


## Java generic method argument safety, 2026-11-07

- Check original generic method signatures before erasure, closing incompatible addAll, putAll, copy and generic constructor argument cases.
- Retain and check extends/super wildcard directions, inherited type arguments and nested invariance. Reject unsafe concrete substitutions into invariant nested wildcard captures.
- Infer and validate each generic varargs element; compatible subtype copies and tested lower-bound consumers continue to run.
- Added seven rejection fixtures, one runnable regression and six compiled-Java signature rejection cases, with positive tests for both wildcard directions and nested covariance. Full capture conversion and constraint-based method inference remain unfinished.


## Java generic assignment safety, 2026-11-07

- Reject incompatible parameterized Java assignments, arguments and returns before JVM type erasure can hide the mismatch. Resolve inherited Java class/interface arguments, including those on Mammoth subclasses.
- Reject assigning an existing raw Java value to a parameterized destination; infer omitted constructor arguments from a same-class declared destination.
- Validate nested Mammoth type parameters in erased Java arguments, and pass generic receiver information into Java constructor checks.
- Added ten rejection cases and a verified runnable program; complete release tests, formatting and Clippy pass. Java wildcard and method-inference checking remain incomplete.


## Nested Java class imports, float parameters, and a custom Minecraft block, 2026-11-07

- **Nested class imports.** `import a.b.Outer.Inner` now finds the nested Java class `a/b/Outer$Inner`, including generic ones, enums, and `as` aliases. Before, nested classes needed an explicit binding with declared members.
- **Float parameters.** A Mammoth `Double` converts to a Java `float` parameter (`d2f`). This conversion ranks after `double` overloads, so `Math.abs(-2.5)` still picks `abs(double)`.
- **Fabric demo.** A real custom block and block item: `GlowBlock extends Block` uses `AbstractBlock.Settings.create().strength(1.5).luminance { state -> 12 }` and overrides the protected `onUse`. It comes with blockstate, model, item model, and translation resources.
  - The smoke test verified the registry IDs, luminance 12, hardness 1.5, and the block item's link to the block.
  - `verifyMod` checks the remapped `onUse` override and the resources.
  - The demo now imports `Item.Settings` instead of using an explicit binding.
- **Tests.** Added the `java-nested-classes` program (SimpleEntry, Thread.State, UnicodeBlock, Point2D.Float) and a rejection fixture.

## Static init blocks, inferred static types, and a Mixin accessor, 2026-11-07

- **Static init blocks.** `static init { }` blocks run in `<clinit>`, interleaved with static field initializers in source order.
- **Inferred static types.** Static fields without a type now infer it from any starting value that `static_kind` understands: constructors, function calls, the class's own static methods, and expressions over earlier static fields. Inference runs after all declarations are resolved.
- **Mixin accessor.** The Fabric demo adds `ClientAccessor.mam`, a Mammoth `@Mixin` interface with `@Accessor` getter/setter methods and an `@Invoker` method. Bodiless annotated interface methods and Java-to-Mammoth-interface casts already supported this.
  - In the real client, the smoke test cast `MinecraftClient` to it, read and wrote the private cooldown field, and called the private `getWindowTitle()`.
  - `verifyMod` checks the remapped accessor and invoker targets.
  - The smoke test now derives the expected stack-trace line from the source.
- **Tests.** Added the `static-init` program and 2 rejection fixtures. The untyped-static fixture now uses `null`.

## Static members, 2026-11-07

- **Syntax.** Classes accept `static fun`, `static var`, and `static let`, optionally `private`, with annotations.
- **Bytecode.** They compile to JVM static methods and fields with plain names. Initializers run in a generated `<clinit>` in declaration order.
- **Access.**
  - Outside the class, use `ClassName.member`, including assignment and compound assignment.
  - Inside the class and its subclasses, the class name can be left out.
  - Private statics are class-only.
  - `static let` is final.
- **Checks.**
  - Static methods have no `this` and reject the object's fields.
  - Untyped static fields need a literal starting value.
  - Static generic methods, protected/abstract/override/export statics, enum static fields, and name clashes with instance members are rejected clearly.
- **Fabric demo.** It now injects a private static handler into the static `MinecraftClient.getInstance()` and reads a static `@Unique` field with a nondefault initializer. Both work in the real development client, and `verifyMod` checks the remapped static handler and field.
- **Tests.**
  - Added the `static-members` verifier program and example, 13 rejection fixtures, and a cross-module `static-members` project.
  - Added the `java_uses_mammoth_static_members` integration test: Java calls and assigns statics, reflection confirms private/static/final modifiers, and javac rejects reading a private static.

## Debugger variable names, 2026-11-07

- Methods, constructors, lambda helpers, and interface defaults now carry a `LocalVariableTable`. Parameters, `this`, local `let`/`var` bindings, loop and catch variables, and captured cells appear with their names and live ranges.
- Entries are synchronized at statement boundaries and block exits, and inference probes that discard code also discard their entries. Lambda-shared `var` cells are described with their real array type.
- A real `jdb` session stopped at a `.mam` line and listed `values`, `bonus`, `sum`, `value`, and `doubled` with their values. The new integration test checks exact entries with `javap -l`.

## Type-argument checks for Java generics, 2026-11-07

- **The new check.** Arguments to Java methods whose parameter type comes from a substituted type variable (`E` in `ArrayList<Text>.add`, `V` in `HashMap<Text, Int>.put`, inherited methods of generic Java superclasses) must fit the type argument. Previously, the erased `Object` parameter accepted any value, so `add(5)` could put an Integer into a list of strings.
- **Still accepted.**
  - subclasses
  - null and nullable values
  - Mammoth type parameters
  - wildcard-typed parameters such as `addAll(Collection<? extends E>)`, through the existing Java subtype rules
  - lambdas and concrete Java parameter types, through overload selection
- **No widening.** Primitive values are not widened, so `ArrayList<Long>().add(5)` is rejected, matching Java.
- **Compatibility.** Every existing program, example, project, and the Fabric demo still compile.
- **Tests.** Added the `java-type-arguments` verifier program and 5 rejection fixtures: list, map, `Long`, sibling class, and inherited generic superclass method.

## Generic Java superclasses, 2026-11-07

- **Syntax.** A Mammoth class can extend a parameterized Java class, such as `class Names extends ArrayList<Text>()` or `class Bag<T> extends ArrayList<T>()`, which also works with the class's own type parameters.
- **Substitution.** Inherited method calls (implicit, explicit, and `super`) and protected or public generic Java fields substitute the superclass's type arguments, through `Symbols::java_super_view`. Field generic signatures are parsed with `parse_field_signature`.
- **Overrides.** Overrides are checked against the substituted generic signature (parameters equal, result compatible). The existing Java bridges supply erased entries, so Java dispatch through `Collection<String>.add` reaches the Mammoth override.
- **Bytecode.** The class `Signature` records the parameterized superclass, so `getGenericSuperclass()` reports `lib.Holder<java.lang.Integer>`.
- **Tests.**
  - Added a JDK verifier program (`generic-java-superclass`, also an example) and 3 rejection fixtures.
  - Added the `mammoth_extends_generic_java_classes` integration test. It compiles a real generic Java `Holder<T>` with protected generic, final, and list fields and a protected method, then runs Mammoth subclasses and a Java caller under `-Xverify:all`. It also checks wrong type arguments, final-field writes, protected access, and result types.

## Debug line numbers, protected methods, Java protected fields, and a richer Fabric demo, 2026-11-07

- **Debug metadata.** Every class file has a `SourceFile` attribute naming its `.mam` file, and methods have `LineNumberTable` entries per statement. This covers functions, methods, constructors, field initializers, `init` blocks, lambda helpers, interface defaults, and generated forwarding/bridge methods, across project modules. Stack traces now read like `at Main.fail(crash.mam:3)`. Probe-and-restore code generation discards line entries with the discarded code.
- **Mixin initializers.** In the real Minecraft client, nondefault `@Unique` field initializers (`= 40`, `= "tusk-" + "ready"`) now run. A controlled build without line numbers left them unset and failed the smoke test, because Mixin locates initializers through line numbers.
- **Protected methods.**
  - `protected fun` is supported, including abstract, generic, and overriding methods, with implicit, explicit, lambda, and `super` calls. It compiles to `ACC_PROTECTED`.
  - Calls are checked for the declaring class or a subclass, and, as the JVM requires, for receivers of the calling subclass's type.
  - Overrides may widen protected to public but not narrow public to protected, including public Java methods.
  - Protected methods cannot implement interface methods or be exported.
- **Java protected fields.** Subclasses of Java classes can read and assign the Java superclass's public and protected instance fields, implicitly or through receivers of their own type, with JVM receiver checks.
  - Final fields are read-only.
  - Private and package-private Java fields are rejected with a clear message.
  - Byte, short, and float fields get the right narrowing conversions.
- **Fabric demo.**
  - `mammoth_demo:tusk` is now a Mammoth `TuskItem extends Item` with glint and right-click behavior, stack size 16, and a protected helper.
  - The Mixin adds nondefault Unique initializers.
  - The smoke test checks the glint override through Minecraft's `Item` type, the stack size, a `.mam` stack-trace line, and the initializers.
  - `verifyMod` checks the remapped overrides and the debug attributes.
  - Both checks passed in the real development client.
- **Tests.**
  - Added the `protected-methods`, `java-protected-fields`, and `java-protected-methods` programs.
  - Added 17 rejection fixtures.
  - Added two projects: a cross-package `protected-methods` project and a `reject-protected-method` project.
  - Added two Java integration tests:
    - real stack traces through a multi-module project and a single file
    - a Java subclass overriding and calling Mammoth protected methods, with javac rejecting an unrelated caller

## Annotated and protected fields, 2026-11-07

- Added Java annotations on fields and constructor properties, retaining visibility, mutability, generic signatures, target checks, retention, and nested values. Constructor-property annotations target fields, not parameters.
- Added protected Mammoth fields with checked read/write access, including the JVM's subclass-receiver restriction. Protected methods and Java-declared protected field access remain unsupported.
- Added verifier programs, a cross-package generic inheritance project, Java reflection/subclass/retention tests, nine rejection fixtures, and a field-annotation example.
- Extended the real Fabric demo with mutable primitive and final reference Shadow fields plus private Unique state. The actual-client smoke test passed, reading Minecraft's cooldown and options fields. Packaged-JAR checks inspect field annotations, flags, descriptors, and remapped names.

## Real Fabric mod and Java generic-call context fix, 2026-11-07

- Added `examples/fabric-demo` for Minecraft 1.21.1: custom item, client keybind, Fabric end-tick callback, and a real Sponge Mixin, all written in Mammoth. Includes a pinned Gradle/Loom pipeline, wrapper distribution checksum, local verified Java 21 setup helper, item model/translations, and install/run instructions.
- Added `verifyMod` to inspect the remapped artifact and `smokeTest` to launch the actual development client with strict verification, exercise a simulated key press and the injected tick handler, verify runtime markers, and shut down cleanly after resource loading. Both passed. Production-profile/world/physical-input testing remains separate.
- Fixed generic Java call context: capture the receiver before probing nested arguments and defer installing the enclosing result type until all arguments are emitted. This prevents a generic result cast being applied to an argument's static field and preserves generic information across nested calls/constructors. The former error crashed actual item registration with a Registry-to-Item ClassCastException.
- Added two JDK-only regression programs, covering the runtime static-field cast and nested receiver/result inference. Full compiler regression tests, formatting, and Clippy passed.

## Nested JVM annotation values, 2026-11-07

- Added nested values such as `@Inject(at = @At(value = "HEAD"))`, arrays of annotations, and recursively nested metadata. A dedicated annotation argument AST keeps annotation values separate from executable expressions.
- Share public-type, element-name, required/default-element, duplicate-name, and constant-type checks between declaration annotations and nested values. Nested types must match the exact Java annotation element type.
- Nested Target/Retention metadata is handled as a value: it does not forbid use through `@Target({})` or omit nested SOURCE/CLASS-retained values inside a retained enclosing annotation. Java reflection tests verify these cases.
- Added Java's one-element array shorthand for strings, primitives, enums, classes, and nested annotations. Preserve explicit arrays, empty arrays, and existing parenthesized constant lists.
- Bound annotation parsing/emission depth and align the class-file reader's nested annotation depth tracking. Added a deep-input rejection fixture.
- Expanded Java integration tests with multi-level values, annotation arrays, defaults, qualified annotation names, empty arrays, singleton shorthand, and invalid nested elements/types. Full Mixin/game execution remains unverified.

## JVM declaration annotations, 2026-11-07

- Added annotations on classes, interfaces, functions, and methods, with imported or fully qualified Java annotation types and named arguments.
- Read Java annotation targets, retention, and default-element presence. Validate public annotation types, allowed declaration targets, required/unknown/duplicate elements, and duplicate annotation types.
- Emit runtime-visible/invisible attributes according to retention, omitting SOURCE attributes. Support literal strings (modified UTF-8), all Java primitive element types, class literals, enum values, and arrays. Reject expressions and invalid values; check byte/short ranges and JVM size limits.
- Place method annotations on exported and Java callback entry points. Preserve generic signatures, Code, BootstrapMethods, and class attributes alongside annotations.
- Added a Java compilation/reflection/strict-verifier integration test with real annotation declarations, javap retention inspection, primitive/class/enum/array values, Unicode and NUL strings, exported methods, interfaces, callbacks, and rejection checks. Added an example and eight rejection fixtures.
- Nested annotations, field/constructor/parameter/type-use annotations, annotation declaration syntax, repeated annotations, and annotation processors remain unsupported. A real Minecraft/Mixin launch remains unverified.

## Exported instance methods and generic override checks, 2026-11-07

- Class methods accept `export fun`, generating public JVM entry points with ordinary method names for Java and reflection. Generic signatures and argument null checks are preserved. Existing callback/Object bridges are reused without duplicate declarations, including exported `Runnable.run` and `toString`.
- Exported instance methods cannot be private or abstract, override final Java methods, conflict with static Java methods, or use incompatible signatures for inherited Java methods.
- Class overrides and interface implementations now compare method bounds with ancestor substitutions and renamed method parameters. This closes a gap that allowed implementations to strengthen or remove generic constraints.
- Generic member inference now includes the receiver's type substitutions when checking method bounds, fixing bounded calls through generic superclass views and `super`.
- Superclass method lookup now separates explicit method type arguments from the method name, so `super.keep<T>(value)` resolves correctly. Supplying type arguments to a nongeneric superclass method is rejected.
- Added an export example, a project with a real Java/reflection caller, positive bound-substitution tests, and rejection cases. Tests include inherited dispatch, generic classes and methods, wide parameter slots, nullable/non-null Java arguments, and callback bridge deduplication.

## Parent default calls and child-interface overrides, 2026-11-07

- Added `super<Interface>.method()` in instance methods and default interface bodies, including their lambdas. The named Mammoth interface must be a direct, nonredundant parent and the selected method must have a body. Generic arguments are inherited from the parent type; method type arguments can be inferred or explicit.
- Child interfaces can override inherited methods with bodies, keeping substituted parameter/result types and method bounds. Optional `override` checks that a parent method exists. Most-specific declarations win independently of parent order. Conflicts from unrelated interfaces require an explicit implementation, including conflicts reached through a superclass.
- Added interface bridges for specialized defaults. Primitive specializations expose boxed Java-compatible bodies with primitive forwarding entries; class overrides also implement the boxed descriptors. Java inheritance and overrides and Mammoth overrides all dispatch through the same method. Qualified super calls target the body directly.
- Source-level primitive parameter and return checks remain strict despite boxed entries, including null rejection and returns through `finally`. Ordinary primitive methods keep their representation.
- Added two verifier programs, a beginner example, 16 rejection fixtures, a cross-module project, and a Java inheritance/override integration test. The tests cover all primitive kinds, nullable values, generic methods and bounds, closures, long/double slots, diamonds, ambiguity, and direct-parent checks.

## Interfaces extending Java interfaces, 2026-11-07

- **Syntax.** `interface Task extends Runnable` and `interface Ranked<T> extends Comparable<T>` are accepted. Java parents can be mixed with Mammoth parents. Java classes and duplicate Java parents are rejected.
- **Behavior.**
  - Java parent methods are callable through the Mammoth interface (`method_receiver` falls back to the Java parent).
  - Mammoth interface values fit the Java parent type.
  - Implementing classes must provide the Java parent's abstract methods, through the existing Java interface check, and get Java bridges such as `compareTo(Object)`.
- **Bytecode.** Java parents are written as JVM superinterfaces with generic signatures. For example, `MammothInterface$1<T> extends java.lang.Comparable<T>`.
- **Tests.**
  - Added the `java-parent-interfaces` program and example and 3 error cases.
  - Added a cross-module `Job extends Runnable` export, which Java assigns to `Runnable` and runs.

## Star projections, 2026-11-07

- **Syntax.** `Box<*>` and `Source<*>` work for generic Mammoth classes and interfaces. Each star is a hidden type parameter per declaration and position, bounded by the declared bound when that bound is free of other parameters.
- **Subtyping.**
  - Any `Box<X>` fits `Box<*>`.
  - `applied_fits` now handles classes as well as interfaces, and is used in the class branch of `require`.
  - A star in an expected type no longer drives constructor inference.
- **Safety.** Reading starred members is allowed. These writes are rejected:
  - calls whose parameter types mention a starred parameter (`writes_star`, checked in `adapted_arguments`)
  - assignments to starred `var` fields
- **Runtime checks.** `is`, `as`, and `as?` against an all-star target work from any value and smart-cast.
- **Interop.** Generic signatures write `*`, so Java sees `Box<?>`.
- **Tests.**
  - Added the `star-projections` program and example and 5 error cases.
  - A cross-module export that takes `Source<*>`, called from Java.

## Generic fixed-size lists, 2026-11-07

- **What works.** Generic functions, methods, and classes accept `List<T>`, including bounded `T` and nested `List<List<T>>`. Elements can be read, written, looped over, and passed on, and lists can be returned. Lists are never copied, so in-place algorithms such as swap, reverse, fill, and rotate change the caller's array.
- **Representation.** A `List<T>` is an array of `T`'s erasure, `Object[]` or the bound's array. Java sees `<T> T[] f(T[])`.
- **Soundness rules.**
  - Primitive lists such as `int[]` cannot be passed where a generic `List<T>` is expected. This is checked for every generic call and constructor, including nested positions such as `Box<List<T>>`.
  - Generic code cannot create a `List<T>` literal, so every generic array originates in concrete code with its real element class.
  - Invariance prevents `ArrayStoreException`.
- **Tests.**
  - Added the `generic-lists` program and example and 5 error cases.
  - Added a cross-module export.
  - Java calls it with a `String[]`, and javac rejects an `int[]`.
  - The obsolete rejection test `generic-fixed-list` was removed.

## Default interface methods, 2026-11-07

- **Syntax and bytecode.** Interface methods can have bodies, as in `fun greet(): Text { return "hello, " + name() }`. These compile to JVM default methods in the interface class file. Generic signatures are kept, and `this` is typed as the interface.
- **Calls.**
  - Unqualified and `this.` calls inside a default body dispatch through the interface.
  - Classes inherit defaults: through object references, implicit calls inside the class, generic interface views, and parent interfaces.
  - Overrides work, including primitive overrides of generic defaults, which get bridges.
- **Checks.**
  - Abstract interface methods are still required.
  - A class that inherits two same-named defaults from unrelated interfaces must declare the method.
  - Missing returns and type errors in default bodies are reported as for any method.
  - Lambdas inside default methods work, including captures of `this`. Their bodies and helpers become private static interface methods, referenced through `InterfaceMethodref`.
- **Internal.** Emitter construction moved into `new_emitter`, shared by classes and interfaces.
- **Tests.**
  - Added the `default-methods` and `default-lambdas` programs, the `default-methods` example, and 5 error cases.
  - A default method in a cross-module project.
  - A Java class that calls an inherited Mammoth default method.

## Implied runtime checks for generic types, 2026-11-07

- `is`, `as`, `as?`, and `when` `is` branches now accept a parameterized target when the value's static type determines every type argument. For example, `source is Box<Int>` works for a `Source<Int>` because `Box<T>` implements `Source<T>`.
  - These checks compile to a plain `instanceof` or `checkcast` of the erased class and smart-cast as usual.
  - They work through generic superclasses, interface inheritance, and type parameters, as in `source is Box<T>` for a `Source<T>`.
- Checks whose arguments cannot be confirmed at runtime are still rejected, with a message that explains the rule. Examples: `Box<Int>` from `Any`, `Box<Text>` from `Source<Int>`, and `Box<Dog>` from a covariant `Source<Animal>`.
- Added the `generic-runtime-checks` program and 3 error cases.

## Declaration-site variance, 2026-11-07

- **Syntax.** Interface type parameters can be marked `out` (covariant) or `in` (contravariant), as in `interface Source<out T>` and `interface Transform<in A, out B>`.
- **Position checks.** Every member signature is checked:
  - Positions flip through function parameters and follow the variance of nested interface types.
  - Marked parameters cannot appear inside invariant types: lists, generic classes, and Java generics.
  - Classes cannot mark parameters, and the error says so.
- **Subtyping.**
  - `Source<Dog>` fits `Source<Animal>`, and `Sink<Animal>` fits `Sink<Dog>`. This holds for classes, interface values, and parent interfaces.
  - Arguments are compared after boxing, so `Source<Int>` fits `Source<Any>` but not `Source<Long>`.
  - No bytecode changes are needed, because erased calls are unchanged.
- **Tests.** Added the `variance` program and example and 10 error cases. The cross-module project now declares `Source<out T>` and uses a covariant view.

## Multiple bounds and trailing lambdas, 2026-11-07

- **Multiple bounds.** A type parameter can have several bounds, as in `<T: Named & Scored>` or `<A: Animal & Named>`, with the same rules as Java:
  - Only the first bound may be a class, and duplicate bounds are rejected.
  - Every bound is checked for written and inferred arguments.
  - Members of every bound are callable. Methods of later bounds get a `checkcast` to their interface.
  - Assignability works through any bound.
  - JVM signatures list every bound, such as `<T extends Named & Scored>`, so javac rejects Java arguments that miss one.
- **Trailing lambdas on bare calls.** `twice { ... }`, `apply(3) { ... }` and `let n = make { ... }` now parse for every function name, at statement and expression level. Previously, only capitalized constructor names and method calls accepted them.
- **Tests.**
  - Added the `multiple-bounds` and `trailing-lambdas` programs and 5 bound error cases.
  - Added a cross-module export with two bounds, which Java calls.
  - Added a javac rejection of an argument that misses the second bound.

## Interface inheritance, 2026-11-07

- **Syntax.** `interface Stream<T> extends Source<T>, Named` declares one or more parent interfaces, generic or not.
- **Typing.**
  - Inherited methods are callable through the child type, with arguments substituted.
  - A child interface value is assignable to every ancestor.
  - Generic inference sees through ancestors, so `drain(stream)` infers `T` from `Stream<Int>`.
  - Classes must implement every inherited method.
- **Checks.** These are rejected:
  - interface cycles
  - non-interface parents
  - duplicate parents
  - redeclared inherited methods
  - same-named methods from unrelated parents
  - one interface inherited with different type arguments, by an interface or by a class (including through its superclass)
- **Bytecode.** Parents are written as JVM superinterfaces with a generic class `Signature`. Bridges cover primitive implementations of inherited erased methods.
- **Tests.**
  - Added the `interface-inheritance` program and example, and 13 error cases.
  - Added a cross-module project in which an interface extends interfaces from another module.
  - The Java integration test now implements a Mammoth child interface in Java and calls an inherited method through the parent type.

## Generic interfaces, generic inheritance, larger function types, and bounds, 2026-11-07

- **Parameterized interfaces.** `interface Source<T>` with any number of type parameters, nullable member types, and generic methods. Classes implement `Source<Int>` or, from a generic class, `Source<T>`. Calls substitute the arguments, type checks are invariant, and `fun take<T>(source: Source<T>)` infers `T` from any implementing class. These interfaces work across project modules.
- **Bridge methods.** When an implementation's descriptor differs from the erased interface or superclass method, for example `$next()I` against `$next()Ljava/lang/Object;`, the class gets a synthetic bridge that casts or unboxes arguments and boxes the result. This also covers methods inherited from a superclass that satisfy a newly listed interface, and abstract generic methods overridden with primitives.
- **Generic superclasses.** Declarations such as `class Score extends Box<Int>(points)` now work across several levels, as in `Labeled<T> extends Box<T>` and `Name extends Labeled<Text>`.
  - Constructor arguments, inherited fields (including compound assignment), inherited methods, implicit calls, and `super` calls all substitute the superclass arguments.
  - Overrides are checked against the substituted signature, and a subclass value is accepted where `Box<Int>` is expected.
  - Privacy and cycle checks still apply, including across modules.
- **Function types with any number of parameters.** Function types with more than two parameters, such as `(Int, Int, Int) -> Int`, compile to generated interfaces `mammoth.function.FunctionN` and `ProcedureN`. These are written only when used and carry JVM generic signatures and `@FunctionalInterface`. Lambdas, captures, fields, lists of functions, nullable parameters, generic functions, project exports, and Java lambdas all work with them.
- **Type parameter bounds.** `fun largest<T: Comparable<T>>(...)`, `class Pen<A: Animal>`, and `interface Ranked<T: Comparable<T>>` are supported. Bounded values offer the bound's methods and fields.
  - Written and inferred type arguments are checked against the bound, including superclass and `implements` types, with precise error locations.
  - Bounded parameters erase to their bound, as in Java, and signatures record the bound, so javac rejects invalid Java arguments.
- **JVM signatures.** Methods and fields now get generic `Signature` attributes whenever the erased descriptor loses information, for example for function types and Java generic collections. Java callers therefore see `Function3<Integer, Integer, Integer, Integer>` rather than a raw type.
- **Mutable lists.** `MutableList.add` and `set` now pass the element type to lambdas, so `functions.add { a, b -> a + b }` infers its parameters.
- **Tests and examples.** Added the programs `generic-interfaces`, `generic-inheritance`, `function-types`, and `bounds`, with matching examples. Added 30 error cases and the project fixtures `generic-interfaces`, `generic-inheritance`, `function-types`, `bounds`, `reject-generic-private`, and `reject-bound`. The javac integration test now implements a Mammoth generic interface in Java, calls inherited generic members, passes Java lambdas to three- and four-parameter function types, and checks that javac rejects bad interface type arguments and out-of-bound arguments.


## Generic methods, 2026-11-07

- Added methods with their own type parameters, including `Box<T>.map<R>`, inferred lambdas, explicit method type arguments, and result-context inference.
- Generic methods work on classes, generic class instances, and interface references, including implicit calls, `super`, overrides, nullable safe calls, and project imports.
- Override and interface checks compare type-parameter relationships rather than requiring identical parameter names. Bodies remain checked for every allowed type.
- Added generic method metadata to interfaces and tested class and interface calls from Java, including Java lambdas and rejected incompatible arguments.
- Added verifier and error cases, a project fixture, and `examples/generic-methods.mam`. Interfaces with their own type parameters remain future work.

## Generic classes, 2026-11-07

- Added declarations such as `class Box<T>(var value: T)`, explicit types such as `Box<Int>`, constructor inference such as `Box(42)`, and generic data classes with several parameters.
- Fields, constructors, and methods substitute the receiver's arguments while sharing one erased JVM class. Primitive values are boxed at class boundaries, with typed getter results and checked setters and assignments.
- Added nested applications, nullable fields and safe calls, generic function integration, function-valued fields, captured values, inferred globals and fields, and project imports with aliases.
- Added JVM generic signatures for classes, constructors, fields, and methods. Java tests infer factory return types, use typed fields and getters, and reject an incompatible field assignment.
- Added verifier and error regressions and `examples/generic-classes.mam`. Generic interfaces, methods with additional type parameters, generic superclass types, bounds, and runtime checks of parameterized Mammoth classes remain future work.

## Generic functions, 2026-11-07

- Added top-level type parameters such as `fun identity<T>(value: T): T`, inferred calls, and explicit calls such as `identity<Int>(42)`.
- Added generic nullable parameters/results, mutable and Java collection helpers, higher-order functions with inferred lambda results, recursion, and inferred globals and fields.
- Exported generic functions retain their types across project imports and expose JVM Signature metadata for Java callers. A javac integration test calls Mammoth generic functions with Java type inference and a Java lambda.
- Generic bodies are checked with distinct type parameters and compile to erased JVM methods. Primitive boxing and result casts occur at calls. Incompatible Java collection arguments are rejected at generic calls.
- Added verifier programs, a cross-module generic project, and error regressions. Generic classes/methods, bounds, nullable type arguments, and fixed-size `List<T>` declarations remain unsupported.

## Shared project types, 2026-11-07

- Classes and interfaces can be imported between source modules, with optional `as` aliases. No extra export keyword is needed for types.
- Constructors, methods, public fields, inheritance, interface contracts, enums, data classes, and nullable object types work across files. Unexported functions and private fields remain inaccessible through imports.
- Exported function signatures are resolved in their declaring file and retain object identities, list element types, function types, Java generic arguments, and nullability in callers.
- Shared declaration and JDK classpath data is retained once per project. Inheritance cycles are checked before code generation, including cycles across files.
- Added project JAR tests for resources, scope isolation, Java bindings and callbacks, mutual type references, and diagnostics in the correct source file. Added `examples/project-classes`.

## Nullable primitive values, 2026-11-07

- Added `Int?`, `Long?`, `Double?`, `Bool?`, and `Char?` with boxed JVM representations. Non-null primitive types retain their existing representation.
- Nullable numbers work in functions, fields, lists, lambdas, Java reference callbacks, map lookups, safe calls, null checks, casts, and `?:` defaults.
- Added text methods `toIntOrNull()`, `toLongOrNull()`, and `toDoubleOrNull()`; invalid numeric input or integer overflow returns null. Helpers are generated only when used.
- Fixed `receiver?.method() ?: fallback` when the receiver is present but the method returns null.
- Added verifier regressions and made the regression runner reject every nonzero Java exit status.

## Overnight session, 2026-11-07

Everything below compiles to verified JVM bytecode. `cargo test --release` builds every program in `tests/programs` and `examples`, runs it with `java -Xverify:all`, and compares its output with the expected `.out` file. Programs in `tests/errors` must fail with a specific message.

### Language

- `else if`, `break`, `continue`, a bare `return`, and compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`), including on list items and fields.
- `for i in 1..10`, `for i in 0 until n`, and character ranges such as `'a'..'z'`.
- `Long` and `Char` types. Integer literals too large for `Int` become `Long`. Mixed `Int`, `Long`, and `Double` arithmetic widens automatically, and an `Int` can be passed where a `Long` or `Double` is expected.
- Unary minus on any expression.
- Text: `+` joins text, `==` compares contents, `<` and `>` compare alphabetically, and `{expression}` interpolates any expression. Text has built-in methods (`length`, `uppercase`, `split`, `substring`, `toInt`, and more), and every Java `String` method also works.
- `print` accepts any value, including lists and objects. `print()` prints a blank line.
- `input()` and `input(prompt)` read a line from the keyboard.
- Math helpers: `abs`, `min`, `max`, `sqrt`, `pow`, `random`, and `randomInt`. Also `sleep`, `exit`, and `currentTimeMillis`.
- Top-level variables can be used and changed from functions, methods, and lambdas.
- `when` as a statement or a value, with several values per branch, `in` ranges, `is` checks, and `else`. `if` can also give a value.
- Null safety: `?.` safe calls and `?:` defaults.
- `is` type checks with smart casts, `as` casts, and `as?` safe casts. `Any` is the type of every non-null value.
- Lambdas `{ x -> x * 2 }` with `it`, trailing lambdas, and function types such as `(Int) -> Int`. Lambdas capture variables and `this`, and can change `var` locals of the enclosing function.
- Nested lists such as `List<List<Int>>`, `MutableList<MutableList<Text>>`, `List<Text?>`, and lists of functions.
- Exceptions: `try`, `catch`, `finally`, `throw`, and `error("message")`.

### Classes

- Public `var` and `let` fields, which other code reads with `object.field` and assigns with `object.field = value`. `private` fields and methods.
- Constructor parameters become fields when written as `let` or `var`, as in `class Point(let x: Int, let y: Int)`.
- A field can start with any expression, and `init` blocks run in order with the field initializers.
- `toString()` controls how an object prints.
- Inheritance with `extends`, `override`, `super.method()`, `abstract class`, and `abstract fun`.
- `enum` types, which compile to real Java enums, with constructor values and methods.
- `data class`, which generates `toString`, `equals`, and `hashCode`.
- `equals` and `hashCode` can be overridden, and `==` uses `equals`.

### Java interoperability

- The compiler reads the JDK's `lib/modules` image and any JARs or folders on `CLASSPATH` directly. The JAR reader, DEFLATE decoder, and class-file parser are written from scratch, so the compiler still has no dependencies.
- `import java.util.Scanner` and `import java.util.*` bring in Java classes. `java.lang` classes are always available.
- Constructors, static methods, static fields, instance methods, and instance fields work without declarations. Overloads are chosen like Java does, including varargs such as `String.format`.
- Java generics: `ArrayList<Text>`, `HashMap<Text, Int>`, typed results, typed lambda parameters, and inference for generic methods such as `Collections.max`.
- `map[key]` and `map[key] = value` on Java maps, and `for` over any Java `Iterable`.
- Mammoth classes can implement any Java interface, including generic ones such as `Comparator` and `Comparable`, through generated bridge methods.
- Lambdas convert to any Java functional interface, such as `Runnable`, `Comparator`, or an event listener, through `invokedynamic`.
- Mammoth classes can extend Java classes (Swing windows, `TimerTask`, `ArrayList`, custom exceptions, Bukkit's `JavaPlugin`), override their methods, and call their protected methods.
- Java results that may be null can be stored in a nullable variable or compared with `null`. Other null results raise a clear error.

### Tooling

- A regression suite in `tests/` with about forty programs and error cases, run under the JVM's strict verifier.
- The VS Code grammar highlights the new keywords.
- New examples: `bank.mam`, `guess.mam`, and `window.mam`.

### Known gaps

- Function types had at most two parameters at this point. Larger function types are now supported.
- A lambda whose last line is an `if` or `when` with statement-only branches (no values) does not compile yet. Add another statement after it, or give each branch a value.
- Fully qualified names in expressions, such as `java.util.Random()`, need an `import` instead.
