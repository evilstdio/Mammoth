# Module initialization and compiler commits, 2026-11-10 (latest, read this first)

The uncommitted compiler work is now seven verified local commits, `717689d` to `6b1709f`: dictionary literals, implicit collection names, init-assigned fields, operator methods, library folds, tailrec, probes. Each tree passed `cargo test --release --offline`, `cargo fmt --check` and clippy on its own. `node tests/requests.cjs` passes from the second commit on. The first cannot pass it, because the test needs both of the first two.

Reviewing that work found these bugs, fixed before committing, each with a regression test: `tailrec` with an expression body overflowed the stack; a local shadowing the function looped forever; a public method could be replaced by a subclass, so converting it changed dispatch; `collection -= 6` on a Java list removed by position. `tailrec` is now a promise: what cannot loop is a compile error. Tests: `tests/programs/tailrec-loops.mam`, `tests/errors/tailrec-*.mam`, `tests/programs/collection-minus-values.mam`.

Module values. Design in the header of `src/jvm/globals.rs`, user rules in the README under Top-level variables. `starts_with_class` picks `<clinit>` for a program with a `module` line whose top level is only declarations. Everything else runs in `main`. Each use of a value calls `$ready(name, position)`, which compares against `$progress`, so too-early reads throw instead of seeing zero. There is no cross-module static check: a startup cycle across modules is caught when it runs, and two threads starting such a cycle can deadlock as with any JVM class cycle.

Tests: `tests/projects/module-values` (order, dependencies, once, threads, class entry, all value kinds), `tests/projects/module-value-cycle` (cycles, failed start, callback into the entry module), `tests/projects/reject-module-*`, `tests/programs/global-*`, `tests/projects.rs` `java_entering_through_exports_finds_module_values_started`, and the bundled library's own limits through `requests-import`, `bundled-requests` and `node tests/requests.cjs`.

Checks on this tree: `cargo test --release --offline`, `cargo fmt --check`, clippy, `node tests/requests.cjs` with 27 scenarios, `difffuzz.py 1201 40` with 0 problems, `soundfuzz.py 1202 80 8` with 0 bad. `target\release\mammoth.exe` is rebuilt from this tree. Git is local only.

Known limits found while reviewing: a dictionary literal lowers to `Pair` and `mapOf`, so a program that declares its own function or class with those names captures or breaks the literal. `tailrec` is not parsed on a function declared inside another.

Next task: fix passing an `Int` to an `Int?` parameter on overloaded names, listed below, which lets the library drop its `timeout: Int = 0` convention. Then the `BodyHandler<byte[]>` cast.

# mammoth.requests library, 2026-11-10 (latest)

Source: `src/requests.mam` (Mammoth), linked by `src/libraries.rs` on an import of a public name, which is listed there. A new public class must be added to that list. Tests: `node tests/requests.cjs`, with `ONLY=<part of a scenario name>` to narrow and `MAMMOTH=<exe>` to choose the compiler.

Status: the five planned areas are done and documented in the README section `mammoth.requests`: general JSON; request configuration; response and error API; sessions; redirects, uploads and downloads. Engine: `Http.open` sends and follows redirects and returns an `Exchange` with an unread stream, then `Http.request` or `Http.download` consumes it. `requests` and `Session` hold generated-looking forwarding methods, so a new option has to be added to all of them.

Not verified by a test: an actual connection timeout, because a local server cannot make connecting hang reliably, so only `connectTimeout` validation and wiring are exercised; any https request, including an https-to-http redirect, because the suite has no trusted local certificate. Origin comparison includes the scheme in the code.

The library is a usable first release of its agreed scope, not a finished HTTP client. Deferred on purpose: async, proxies, retries, a streaming response API other than `download`, compressed responses, persistent cookies, client certificates and custom trust, a size limit on in-memory bodies. `tests/requests.cjs` needs dictionary literals and implicit collection names, so it passes from commit `bd4a03a` onward and not on the earlier library commits.

Compiler findings while writing the library:
- Fixed, narrow: `a[i]` ignored overloaded `get`. Test `tests/programs/overloaded-index.mam`.
- Fixed, narrow: Java `byte[]` had no type spelling, now `ByteArray`.
- Fixed: top-level values in a linked library module were never initialized. See the section above. The library now uses them for its limits.
- Open: `reply.body()` of a Java `BodyHandler<byte[]>` throws `ClassCastException` when used as a `byte[]`, even through `Any` and `as ByteArray`. Reproduce with `client.send(request, BodyHandlers.ofByteArray()).body()`. The library reads `ofInputStream` instead, which also lets one daemon timer enforce the body deadline.
- Open: passing an `Int` to an `Int?` parameter fails when the method name is overloaded anywhere in the file ("No version of this function accepts these arguments"). Overloading is decided per name, so `Json.get` makes `Session.get` go through it. Session methods take `timeout: Int = 0`, with 0 meaning the session's timeout, instead of `Int?`.
- Open: `mapOf<Text, Any>("a" to 1)` is rejected because `to` makes a `Pair<Text, Int>`, and `Any?` is refused as a type argument of a Java collection. The library's documented route for mixed values is `Json.of` on each value, or a `MutableMap<Text, Any>`.

# Dictionary literals, 2026-11-10 (latest)

- Added nonempty {key: value, ...} expression literals, lowered to existing Pair/mapOf construction. Nested dictionaries, expression keys, duplicate keys and trailing commas preserve existing mapOf semantics. No new HTTP library or JSON serializer was added.
- Parser lookahead respects explicit lambda parameter lists, comments, quoted text, nested delimiters, :: and ?:; statement-leading and multiline lambda bodies remain lambdas. Empty {} deliberately stays an empty lambda; use typed mapOf for an empty dictionary. Keys and colon must share an expression line unless the key expression is parenthesized.
- Program tracks dictionary usage so prelude dependency selection includes the existing map helper and Pair even when the source contains no mapOf name. Type behavior remains existing mapOf behavior; mixed JSON-style values are not automatically unified.
- Added a runnable regression and malformed-entry rejections covering nested/computed keys, duplicate keys, callback values, typed/empty-value lambdas, commented declarations and Elvis expressions. Full release suite, formatting and Clippy required before delivery.

# Session 5: classes and everyday syntax (2026-11-09, read this first)

Work continues on branch `compiler-correctness`, local only, identity `sss47 <sss47@users.invalid>`, no remote, no AI attribution. Commit only after `cargo test --release --offline`, `cargo fmt --check` and `cargo clippy --offline --all-targets -- -D warnings` pass. Use a separate `CARGO_TARGET_DIR` while a fuzzer or test holds the binary.

**Design notes for what was added.**
- **Secondary constructors** (`Class.secondary`, `Symbols.secondary`, `special.rs` `secondary_constructors`, `overloads.rs` `constructor_for`). Real `<init>` methods that delegate. `Emitter::member` must not cache `<init>` by name.
- **Properties** (`syntax.rs` `property_accessors`, `finish_property`). A property with accessors or marked open, override or abstract has a private field `__field_x`, a getter `x` and a setter `x$set`. Reads reach the getter through the existing `field_on` fallback. Writes go through `classes.rs` `property_setter`. `check_property_ready` rejects early reads. `globals.rs` follows `.x` and `.x$set`.
- **Object expressions** (`syntax.rs` `object_expression`). The object becomes a hidden class `__Anon<offset>` with one function-typed constructor parameter per method, and its methods forward to lambdas written at the creation site, so ordinary lambda capture applies. The body is parsed twice, first to learn member names. Bare member names become `__self<offset>.name`.
- **Sized lists** (`collections.rs` `sized_collection`). Blocks run in the library's `__fill*` and `__growList`. A loop must not be emitted inside an expression, because operand stacks are not tracked for it.
- **Variance on classes** uses the interface machinery. **Java `*`** is a shared hidden star type parameter (`java_star`), and `java_type_arguments_fit` accepts anything for it.
- **`Nothing`** functions are declared to return `RuntimeException`, and each bare call becomes `throw f()`. Names come from a source scan. **Infix** names also come from a source scan.
- **Ranges as values** are eager lists through `__rangeTo`, `__rangeUntil` and `__rangeDown`.
- Library imports used as static call receivers must start with an uppercase letter, hence `M__Collections`.

- **Inner classes** (`classes.rs` `outer_path`, `create_inner`, `inner_on_receiver`, `labeled_this`). The class gets a hidden first parameter and field `__outer`, and an outer class with inner classes emits private members as package-private. **Delegation** (`add_delegated_methods`) writes forwarders at the end of parsing. **Enum value bodies** (`add_enum_value_methods`) fold into `if this == E.A` tests in the enum's own method.
- **Flow typing** (`jvm.rs` `collect_non_null_facts`, `forget_facts`, `field_fact`, `classes.rs` `forget_assigned`, `forget_in_loop`). Facts live in `non_null_slots`. Field facts use numbers from `FIELD_FACTS` up. An assignment turns a fact into `u16::MAX`, and loops drop facts about what they assign before the loop is compiled. `MethodState` keeps a copy of the facts, so probing cannot lose them.
- **Lambdas with no value** where a generic result is expected are listed in `unit_lambdas` and return null.
- Library type parameters are named `__*` and are the only ones that may bind a nullable element (`is_library_parameter`). Prelude generics must therefore not rely on `__T` being non-null.

**Fuzz evidence this session.** Soundness fuzz with the new class features and flow-typing statements (`tests/fuzz/soundfuzz2.py`): 150 programs on an intermediate compiler had 0 unexplained failures, and an earlier 150 found the lambda-as-element bug, since fixed. Differential: 100 programs, 0 problems. Mutation fuzz with a debug compiler: about 3975 inputs on a recent compiler, 0 problems. On commit `d03ede8`: soundness fuzz 260 programs (seeds 6001 and 7001) with 0 unexplained failures, differential 120 programs with 0 problems, and mutation fuzz with a debug compiler 4075 inputs with 0 problems. The two commits after that, which only change which library code is selected and give library code a line table, were checked by the test suite and Fabric `verifyMod`, not by a fresh fuzz run.

**Fabric.** `build` and `verifyMod` passed on the final compiler and produced JAR `3A62CD15...29CB`, the hash already smoke-tested in a real client, so the smoke test was not repeated. If a build seems stale, delete `build/libs`, `build/devlibs` and `build/loom-cache/remapped_working` under `examples/fabric-demo`.

**Recursive local functions** that capture are read twice. On the second reading, calls go through a one-item list (`__rec<offset>_name`, `recursive_holders`) that receives the lambda after it is built.

**Known limits.** No coroutines, no `Float`, `Byte`, `Short` or unsigned types in Mammoth source, no reified type parameters, non-local `return` from lambdas is not supported (a plain `return` leaves the lambda). See the object expression limits in the README. Top-level `obj.fail()` methods returning `Nothing` are not treated as jumps.

**Library selection.** `add_prelude` keeps a library extension only when `library_extension_applies` says its arguments could match, because names such as `toString` appear in nearly every program. Library functions get a one-entry line table, line 1.

**Probes.** `tests/fuzz/kotlin_probe.py` and `tests/fuzz/probe2.py` list everyday constructs, `tests/fuzz/advcases5.py` and `advcases6.py` are adversarial batteries for this session.

# Session 4: language features (2026-11-08, read this first)

The goal is closing the feature gap with Kotlin. Work is on branch `compiler-correctness`, committed locally, identity `sss47 <sss47@users.invalid>`, no remote and no AI attribution. Commit only after `cargo test --release --offline`, `cargo fmt --check` and clippy pass; use a separate `CARGO_TARGET_DIR` so a running fuzzer cannot lock the binary.

**Done so far, each with tests in `tests/programs` and rejections in `tests/errors`:**
- **Default and named arguments** (`src/jvm/defaults.rs`). The parser makes a hidden helper `name$default$i` per default, taking only the earlier parameters whose names appear in its source text. `complete_args` rewrites a call into positional form; it is a no-op when the count matches and nothing is named, so ordinary calls pay nothing. Temporaries (`Expr::Temp`) keep written evaluation order. Constructor helpers are static `new$default$i`.
- **Sealed types** (`branching.rs` `exhaustive`, `covers_sealed`). `when` statements on sealed subjects must be exhaustive. Same-file rule in `check_classes`.
- **Extension functions** (`src/jvm/extensions.rs`). `fun Text.shout()` becomes static `Text$ext$shout` with parameter `this`. `method_call_expected` only takes the slow path when some extension has that name: it tries the member first, then picks the most specific receiver.
- **Built-in library** (`src/prelude.mam`, `syntax.rs` `add_prelude`). Parsed on each compile, but only functions the source mentions are kept. `in_prelude` functions have no line numbers. A fixed list reaches `Iterable` extensions by copying through `toMutableList`.
- **Jumps as values** (`Expr::Jump`, `jump_value`), **objects** (`is_object`, static `INSTANCE`), **operators** (error-path rewrites in `arithmetic_tail`, comparison and index), **`in`** (`contains_check`, `Expr::InRange`), **progressions** (`loop_progression`, overflow-safe), **raw text**, **data class members** (`add_data_members`), **destructuring** (parser desugar, `statement_list`).
- `expression` and `expression_as` now release any locals made while computing a value, so branch joins stay valid.

- **Overloading** (`src/jvm/overloads.rs`). The first parse records names declared twice in one scope; the second gives every declaration of such a name a hidden `name$ov$types` name. Calls probe argument kinds and score candidates. Costs apply only to overloaded names.
- **Two-pass parsing** (`parse_source`). A lenient first pass (`discovering`) finds nested types, function arities, interface properties and overloads. A second pass runs only when one of those is present.
- **Nested types** flatten to top-level types (`nested_type`), so inner names must be unique per file. **Function references** become lambdas using arities from the first pass. **Varargs** are `Signature::vararg`, gathered in `complete_varargs`.
- **Property reads** fall back in `field_on` to `name()`, `getName()` and extensions. **Interface properties** are abstract methods; a class field of that name gets a generated getter (`property_getters`).
- Real programs: `examples/json.mam`, `algorithms.mam`, `library.mam`.

**Known limits.** No coroutines. Implicit receiver members work in a lambda inside an extension only through `this`. A constructor default cannot mention the class's type parameters. Overloaded names are hidden from Java. Overloads with a generic parameter type may not line up with an override written with concrete types. Prelude errors are reported as "in the built-in library". `a to b` works only in call arguments.

**Probe.** `tests/fuzz/kotlin_probe.py` runs 171 everyday constructs; 142 passed at last count. Remaining: setters, secondary constructors, `inner` classes, overriding properties, class-level `out`/`in`, `MutableList<*>`, runtime-sized arrays, `Nothing` in expressions, unary operator methods, sequences, and coroutines. The `:` inheritance spelling is intentionally not adopted.

**Next.** Re-run the fuzzers against this compiler, then the remaining probe items.

# Session summary: session 3, 2026-11-08 (read this first)

**State.** Local Git repository, no remote, identity `sss47 <sss47@users.invalid>` set for this repository only, no AI attribution in commits. `main` holds the baseline; the work is on `compiler-correctness`. Commit `acab45e` has one failing test (`generic-methods`), fixed by the next commit.

**Done, newest details in the entries below.**
1. Fixed the invalid bytecode from the fuzzer: stack frames for untyped branch values, stale shared-variable slots.
2. Differential, soundness and adversarial tools (`tests/fuzz`), plus a seeded differential test in `cargo test`.
3. Soundness: null through `as`, `MutableList` casts, map keys, array element reads, generic Java interface signatures, instance field writes from static code, early reads of top-level variables, enum subclasses, final Java overrides.
4. Overloads follow javac's phases and lambda specificity. Explicit type arguments on Java calls.
5. Interop: `byte[]` as an indexable list (`ListKind::Byte`, elements read as `Int`, distinct from `List<Int>` in `same_kind`), `for` over `Iterable` classes, `EnumMap`, nested Java arrays, `byte`/`short` parameters, nullable lambda inputs, null-tolerant text joining and `==`.
6. Diagnostics and robustness: correct stack-trace lines after probes, unreachable `catch`, unrelated `==`, `while true`, a compiler panic on `static init`.

**Evidence.**
- Differential fuzz: 0 problems on 620 programs (seeds 2, 5, 7) against javac.
- Soundness fuzz: 0 unexplained failures on 150 grown programs after the last soundness fix, 75 of them with random class declarations. A later final run is recorded below if it finished.
- Mutation fuzz with a debug compiler: 6300 inputs found one panic, now fixed.
- A further differential run of 150 programs and soundness run of 20 grown programs also had 0 problems.
- On the final compiler (commit `b539fc5`): differential 0 problems on 100 programs, soundness 0 unexplained failures on 20 grown programs. The earlier, larger runs predate the last few compiler commits. The mutation fuzz was not re-run on the final compiler.

**Final checks, on the last commit's source.**
- `cargo test --release --offline`: unit 5, annotations 1, fuzz 1, programs 3, projects 9, all passing.
- `cargo fmt --check` and `cargo clippy --offline --all-targets -- -D warnings` passed.
- `target\\release\\mammoth.exe` was rebuilt from this source.
- Fabric: `build`, `verifyMod` and the real-client `smokeTest` passed. The JAR is `3A62CD15...29CB`, and rebuilding with the final compiler gave the identical hash.

**Known limitations.**
- Top-level initialization order follows method calls by name, so a same-named method in another class can cause a false rejection. It does not follow lambdas. Indirect this-escape can still observe null or 0.
- Arrays passed through generic functions are not element-checked. Raw Java parameters can pollute typed collections.
- Mammoth cannot write wildcard types, so some valid wildcard programs are rejected.
- `java.lang.String` cannot be constructed directly, so `new String(bytes, charset)` needs `charset.decode(ByteBuffer.wrap(bytes)).toString()`. Mammoth code cannot create a `byte[]` itself. Branches over 32 KB are a clean compile error.
- `a ?: return x` is not supported. Compound assignment to Java fields through an explicit receiver is not supported.
- Generic default methods cannot be called through `super.` when the superclass inherits them.

**Next concrete task.** Extend the fuzzers to multi-file projects and Java subclasses of Mammoth classes.

# Latest: generic Java interfaces, narrow numbers, null text (2026-11-08, session 3)

- **Unsound before.** `class C implements Conv<Text, Int> { fun conv(a: Int): Int }` compiled because only the erased signature was checked. `check_java_interface` now uses the same type-argument comparison as Java superclasses (`classes.rs` `uses_java_arguments`).
- **Also fixed.**
  - An `enum` cannot be extended, and a `final` Java method cannot be overridden (`check_classes`, `check_generic_java_override`).
  - Assignments can start with a call: `make(k).v += 2`, `list()[1] = 7` (`syntax.rs` `member_assignment`).
  - `int[][]` and `String[][]` from Java are indexable nested lists (`kind_of`).
  - An `Int` argument reaches Java `byte` and `short` parameters after any `int` overload, truncated like a Java cast.
  - Text templates, `+` joining, and `==`/`!=` accept a null Java result instead of throwing.
- **Batteries.** `advcases.py`, `advcases2.py`, `advcases3.py`, `jdkcases.py` in the scratchpad hold about 340 focused programs. All `R_` cases are rejected and all `A_` cases run, except these by-design or known items: `a ?: return 0` is unsupported, `"{{"` is not an escape (`\\{` is), a string constant over 65535 bytes and a branch over 32 KB are clean compile errors.

# Latest: real-library call forms (2026-11-08, session 3)

- **Battery.** `jdkcases.py` in the session scratchpad runs 122 common JDK usage snippets (streams, collections, Optional, java.time, files, concurrency, comparators). 111 pass. The 11 that do not: 6 use the Kotlin-style `Runnable { }` constructor form (write `let r: Runnable = { ... }` instead, which works); 2 use fully qualified names in expressions; `"ab".chars()` is a Mammoth builtin returning a list; `byte[]` is an opaque Java value with no `.size()` or indexing; an unannotated `Map.compute` lambda cannot compare its `Int` input to null and now says to write `v: Int?`.
- **Fixed.**
  - Explicit type arguments on Java calls: `Collections.emptyList<Text>()`, `Comparator.naturalOrder<Text>()`. They are exact constraints, so `singletonList<Text>(5)` is rejected (`interop.rs` `explicit_java_call`, `explicit_java_arguments`).
  - `executor.submit { "x" }` picks `Callable`, since a lambda parameter returning a value is more specific than a void one, as in javac (`lambda_returns_value`).
  - `for` over a Mammoth class implementing `Iterable` (`jvm.rs` for loop, via `java_generic_views`).
  - Mammoth enums are `Enum<E>`, so `EnumMap<E, Int>` satisfies its bound (`project.rs`).
  - A Java lambda input can be declared nullable: `{ k: Text, v: Int? -> ... }` (`lambda_input_fits`).
- **Fabric.** `build`, `verifyMod` and the real-client `smokeTest` passed with the compiler at commit `849a06d` (before this milestone).

# Latest: initialization order and control-flow checks (2026-11-08, session 3)

- **Global order.** `globals.rs` `check_global_order` rejects a top-level statement that reaches, through bare function calls or constructors (field initializers and init blocks), a top-level variable declared later. Previously `Text` read as null and `Int` as 0. Lambdas are skipped because they run later. Calls through objects and methods are not followed, which is a known gap. Locals and parameters with the same name as a global are excluded.
- **`while true`** with no `break` now ends the path, so `fun f(): Int { while true { return 1 } }` compiles and code after the loop is rejected.
- **Unrelated equality.** `==` and `!=` between classes (or `Text`) with no subclass relation is rejected. Interfaces and generic types are not checked.
- **Unreachable catch.** A `catch` after one for its parent class is rejected.
- **Top-level `throw`** as the last statement no longer emits a dead `return`.
- **Top-level function values** can now be called inside expressions in other functions. A method of the current class with the same name takes priority (`call_value`).

# Latest: soundness and overload fixes from a new fuzzer (2026-11-08, session 3)

- **Tools.** `difffuzz.py` (same random program in Mammoth and Java, stdout and crash status must match) and `soundfuzz.py` (grows programs from random, usually ill-typed statements; anything accepted must not raise a type or linkage error it did not ask for). Both are in the session scratchpad, not the repo. Differential runs had 0 problems on 420 programs.
- **Fixed.**
  - `x as T` to a non-null type let null through. It now checks (`casts.rs`).
  - `x as MutableList<Int>` was an unchecked cast. It now follows the same "implied by the value's type" rule as Java generics (`classes.rs` `check_runtime_target`).
  - `map[key]` and `map[key] = v` did not check the key type, so a wrong key could corrupt a typed map (`collections.rs` `map_key`). Null keys are still allowed.
  - Stack-trace lines were lost when a statement began with a speculative probe (`Method::restore` dropped the entry at the snapshot offset). The state now keeps the entry count and last entry.
  - Reading a non-null reference element from an array now checks null (`guard_array_element`), at `xs[i]` and in `for`. `let x: Text? = xs[i]` and `print(xs[i])` still see null.
  - Java overload selection follows javac's phases: strict, boxing, variable arity, and then most specific (`interop.rs` `java_phase`). The cost total no longer picks a winner between unrelated candidates, so `crossed(list, list)` is ambiguous as in javac.
- **Limits.** An array passed through a generic function (`first<T>(xs: List<T>): T`) is not element-checked. A `for` loop over an array with null elements raises at that element.

# Latest: invalid bytecode from the semantic fuzzer fixed (2026-11-08, session 3)

- **Stack frames.** `lambdas.rs` `value_or_nothing` passed stack depth 0 to nested calls, so a `when` or `if` value with no expected type dropped operands already on the stack from its frames. It now uses `method.stack.len()`. The earlier note that top-level code was safe was wrong: both forms failed.
- **Shared lambda variables.** `Method::boxed_slots` kept slots after their binding left scope, so a later local in the same slot was read as a shared one-item array. `Method::local` now drops entries at or above the new slot.
- **Validation.** `tests/programs/branch-value-operands`. The debug test suite passes, which also runs the `base == method.stack.len()` assertion in `expression`. The semantic fuzzer had 0 problems on seeds 1 to 4, 80 programs each, against the debug compiler; before the fixes about 8% failed verification.
- **Git.** Local repository only, no remote. Identity `sss47 <sss47@users.invalid>` is set for this repository. `main` holds the baseline, and work is on branch `compiler-correctness`. Commits carry no AI attribution or co-author trailers, at the user's request.

# Session summary: overnight 2026-11-08 (read this first)

Milestones, newest last. Each has its own "Latest:" entry below with details.

1. **Candidate-specific nested inference.** Overload candidates re-probe nested calls with their own parameter types. Varargs elements get context and are checked.
2. **Wildcards.**
   - Wildcard parameters are checked against the original signature (`? super Integer` accepts `ArrayList<Int>`).
   - Per-site wildcard captures make `List<? extends N>` results read-only.
   - Generic Java fields are no longer raw.
   - Lambdas with inferred results cannot return captures.
3. **Expected types through inherited, implicit, `this`/`super` and safe Java calls.**
   - A `super.` call's Java result is null-checked from context.
   - Inherited Java static methods can be called by bare name.
4. **Checked parameterized Java casts and initialization order.**
   - `as`/`is` to `ArrayList<Text>` must be derivable from the value's type.
   - Forward reads in initializers and `init` blocks are rejected.
5. **Robustness.**
   - Cubic LocalVariableTable sync made incremental (52 s → 64 ms; output byte-identical).
   - Truncation fuzz test added; mutation fuzz runs were clean.
   - Lambdas can be passed for erased `Object` generic parameters.
   - Block-comment diagnostic.
6. **Java number boxes as Mammoth numbers.** Integer/Long/Double/Boolean/Character results and type arguments map to Mammoth numbers, and box methods can be called on primitives.
7. **Generic-aware specificity among tied overloads** (`genericTie(null)` is ambiguous, as in javac).
8. **Language and interop additions found by real programs:**
   - nullable function types `((A) -> B)?`;
   - `requireValue` on narrowed numbers;
   - calling call results `f(x)(y)`;
   - `super.` to inherited interface defaults;
   - **silent wrong dispatch fixed** for overrides of defaults that a superclass inherits;
   - Java interface defaults (`andThen`, `reversed`) on Mammoth objects;
   - inherited Java static fields;
   - `Type.class` expressions;
   - assigning Java fields.
9. **Exhaustive enum `when` values** without `else` (Mammoth and Java enums).
10. **Real programs.** `examples/calculator.mam` and `examples/shop.mam`, plus the `dispatch-matrix` test.
11. **Fabric demo re-validated.** `build`, `verifyMod` and the real-client `smokeTest` passed. The JAR SHA256 `918305D5…463A` is recorded in `examples/fabric-demo/validation/README.md` and is unchanged by later compiler changes (rebuilt and compared).

**Final checks.**
- `cargo test --release --offline`: unit 5, annotations 1, programs 3 (all programs, all error fixtures, truncation fuzz), projects 9.
- `cargo fmt --check` passed.
- `cargo clippy --offline --all-targets -- -D warnings` passed.
- `target\release\mammoth.exe` was rebuilt from this source by the final test run.

**Backups.** Source snapshots taken before milestones 3 and 4 are in the session scratchpad (`backup-m1`, `backup-m2`, outside the repo). There is no git repository; nothing is committed.

**Known limitations (not fixed).**
- Mammoth cannot write wildcard types. These are conservatively rejected:
  - reassigning a capture-typed `var` from another call;
  - returning a capture from a lambda with an inferred result;
  - `List<List<? extends N>>` parameters.
- Overload ranking is still cost-first. `overloaded(emptyList())` picks `List<String>` over `Collection<Integer>`, where javac reports ambiguity.
- Java arrays containing null are read as non-null, with no per-element guard.
- Indirect this-escape (methods called from initializers, a superclass constructor calling an override) can observe null/0, as in Java and Kotlin.
- Raw Java parameters can pollute typed collections (documented as unchecked).
- Compound assignment to Java fields is not supported.
- Generic default methods cannot be called through `super.` when the superclass inherits them.
- A top-level Mammoth function shadows an inherited Java static method of the same name.

**Next concrete tasks.**
1. A type-directed semantic fuzzer: generate well-typed programs mixing generics, lambdas and exceptions, and run them under `-Xverify:all`.
2. Java array element nullability policy (a boundary scan, or nullable element types).
3. Exercise Fabric world interactions (gametest) for `onUse`/`use`.
4. Exhaustive `when` statements and sealed hierarchies; done for enum `when` values.

# Latest: assigning Java fields (2026-11-08)

- **Implementation.** `classes.rs` `set_field` routes Java static owners (`static_owner`) and non-null `Kind::Java` receivers to `interop.rs` `write_java_field`.
  - `Types::java_public_field` finds the owner, access flags and erased type.
  - The expected type is `generic_kind` of the field signature with the receiver substitution, made nullable for references.
  - It emits `putfield`/`putstatic` on the declaring class.
- **Validation.** The full release suite, fmt and clippy passed.
- **Limits.**
  - Compound assignment to Java fields is not supported.
  - A nullable Java receiver needs a null check first, and reports the old message.
- **Changed files.**
  - Source: `src/jvm/{classes,interop}.rs`.
  - Tests: `tests/programs/java-field-writes.{mam,out}`, `tests/errors/java-{final-static-write,field-write-type,field-compound-write}.mam`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: class literals in expressions (2026-11-08)

- **Implementation.** `classes.rs` `field_access` checks `Expr::Field(Variable(T), "class")`, where `T` is not a local and resolves as a type, and calls `class_literal`.
  - Primitives use `getstatic Box.TYPE`; everything else loads the class constant with `ldc` through `instance_class`.
  - The result is `Kind::Java(java_generic("java/lang/Class", [T]))`.
  - `T?.class` parses as a safe call, and `List<Text>.class` is a parse error.
- **Validation.** The full release suite, fmt and clippy passed.
- **Changed files.**
  - Source: `src/jvm/classes.rs`.
  - Tests: `tests/programs/class-literals.{mam,out}`, `tests/errors/class-literal-{mismatch,type-parameter}.mam`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: inherited Java static fields (2026-11-08)

- **Lookup.** `interop.rs` `inherited_java_static_field(method, class, name, base, protected)` searches the class's Java root and then the Java interfaces of its ancestors, using `java_static_field_owner` (`java_hierarchy`, public or protected). It emits `getstatic` on the declaring class, with the generic type from `java_field_result`.
  - `java_field_generic_type` now also matches protected fields; callers check visibility.
- **Callers.**
  - The last fallback in `load` (bare name, protected allowed).
  - `field_access` for `MammothClass.FIELD`: protected only when the current class is that class or a subclass of it.
- **Validation.** The full release suite, fmt and clippy passed.
- **Limits.** Writing inherited Java static fields is not supported. Protected access from outside reports the generic "not defined" message.
- **Changed files.**
  - Source: `src/jvm.rs`, `src/jvm/{interop,classes}.rs`.
  - Tests: `tests/java/generic-super/{lib/Holder.java,main.mam}`, `tests/projects.rs`, `tests/programs/java-inherited-static-fields.{mam,out}`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: Java interface defaults on Mammoth objects; Fabric re-validation (2026-11-08)

- **Receiver selection.** `classes.rs` `method_receiver` falls back to `java_interface_receiver` for a native class with no Mammoth method or Mammoth default of that name. It picks the first `java_generic_views` view that is a Java interface declaring the method; `java_methods` includes defaults. It skips when the Java superclass chain (`java_root` → `java_inherited_methods`) already has the method. Note that `java_root` is always `Some` (it falls back to `java/lang/Object`).
- **Fabric.**
  - `dev.ps1 build`, `verifyMod` and `smokeTest` passed with the current compiler. Markers: glow block, stack trace `Controls.mam:70`, Mixin tick, shadow and final fields, unique initializers, keybind, accessor, and `SMOKE PASS`.
  - `validation/README.md` was updated (date note and SHA256 `918305D53C55296CCE8F6406D1E72648CF187B64C5FBD0F4C348438B36F7463A`), and `validation/client-smoke.txt` was replaced with this run's report.
- **Validation.** The full release suite, fmt and clippy passed.
- **Changed files.**
  - Source: `src/jvm/classes.rs`.
  - Tests: `tests/programs/{java-interface-defaults,dispatch-matrix}.{mam,out}`, `tests/errors/java-interface-default-mismatch.mam`.
  - Fabric validation: `examples/fabric-demo/validation/{README.md,client-smoke.txt}`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: overrides of inherited default methods, shop example (2026-11-08)

- **Bridge fix.** When a class declares a method, `bridges.rs` `native_bridges` now also adds the required and body signatures from interfaces listed by *ancestor* classes.
  - Reproduced with the previous compiler: `n.tag(5)` printed `t5` (the default) instead of `b5`. Now correct, including through the superclass type.
- **Super to defaults.** `classes.rs` `inherited_default_super_call` handles the case where `find_method(parent)` fails but `default_method(parent)` finds the interface.
  - It resolves the interface view through the superclass kind and uses `interface_body_signature` for the ABI.
  - It emits `invokespecial` Methodref (tag 10) on the superclass with `$name`; JVM resolution finds the inherited default.
  - Generic default methods are rejected with an explicit message.
- **Validation.** The full release suite, fmt and clippy passed.
- **Changed files.**
  - Source: `src/jvm/{bridges,classes}.rs`.
  - Examples and tests: `examples/shop.mam`, `tests/examples/shop.out`, `tests/programs/inherited-defaults.{mam,out}`, `tests/errors/super-missing-default.mam`.
  - Docs: CHANGELOG, HANDOFF.

# Latest: calling call results (2026-11-08)

- **Parser.** At the top of `syntax.rs` `postfix`, a `(` directly after a `)` or `]` that closes a `Call`/`MethodCall`/`Index` receiver becomes `MethodCall(receiver, "invoke", args)`, which reuses the existing function-value invoke path. The check runs before whitespace is skipped and requires the previous source character to be `)` or `]`, because argument parsing may already have skipped a newline.
- **Diagnostics.** In `builtins.rs` `boxed_method`, a missing `invoke` on a number reports that the value is not a function.
- **Validation.** The full release suite, fmt and clippy passed.
- **Changed files.**
  - Source: `src/syntax.rs`, `src/jvm/builtins.rs`.
  - Tests: `tests/programs/call-results.{mam,out}`, `tests/errors/call-result-argument.mam`, `tests/errors/call-result-not-function.mam`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: nullable function types, calculator example (2026-11-08)

- **Syntax.** `syntax.rs` `annotation()` keeps the parts of a parenthesized list. A single function-type part with no following `->` is a grouped type, optionally followed by `?`. `jvm.rs` `grouped_type` and `resolve_type` resolve `((A) -> B)?` (nullable) and `((A) -> B)` (grouping) before function-type parsing. `(A) -> B?` keeps its meaning: a nullable result.
- **requireValue.** A non-null primitive argument is now a no-op instead of an error.
- **Real application.** `examples/calculator.mam` with `tests/examples/calculator.out`. Writing it also hit, without fixing:
  - calling a call result, `f(x)(y)`, is a syntax error;
  - fully qualified Java names in expressions need an import.
- **Validation.** The full release suite, fmt and clippy passed.
- **Changed files.**
  - Source: `src/syntax.rs`, `src/jvm.rs`.
  - Examples and tests: `examples/calculator.mam`, `tests/examples/calculator.out`, `tests/programs/nullable-functions.{mam,out}`, `tests/errors/nullable-function-call.mam`, `tests/errors/require-value-narrowed-type.mam`.
  - Docs: README, CHANGELOG, HANDOFF.

# Latest: generic-aware overload specificity (2026-11-08)

- `select_specific_java_overload` takes the call's receiver.
- A candidate dominates another only if `java_overload_more_specific` (erased) and `generic_more_specific` both hold. For each parameter, the left formal's `generic_kind` (class variables substituted from the receiver, method variables removed) must pass `java_signature_accepts` against the right formal. Parameters that are unresolved or contain wildcards keep the erased answer.
- Compared against javac on three null cases: tie is ambiguous, agree picks ArrayList, and a variable versus a concrete type picks the concrete one.
- Mammoth's cost-first ranking is unchanged, so `overloaded(emptyList())` still picks `List<String>` over `Collection<Integer>` by cost; javac reports ambiguity there.
- Validation: the full release suite, fmt and clippy passed.
- Changed files: `src/jvm/interop.rs`, `tests/java/generic-super/{lib/Holder.java,main.mam}`, `tests/projects.rs`, README, CHANGELOG, HANDOFF.

# Latest: Java number boxes as Mammoth numbers (2026-11-08)

- **Mapping.** `generics.rs` `unboxed(binary)` maps Integer/Long/Double/Boolean/Character to Int/Long/Double/Bool/Char.
  - `generic_kind` uses it for `GType::Class` (so type arguments and boxed formals map).
  - `convert_from_java` uses it as the fallback generic result for erased boxed results, so the existing nullable/unbox branches apply.
  - Short/Byte/Float are excluded: unboxing them needs other conversions, and Mammoth has no such types.
- **Box methods.** `builtins.rs` `boxed_method` boxes a primitive receiver and calls `java_instance_call` on its box class when there is no Mammoth number method, or when the call has arguments.
- **Reproduced before fixing.** These were all rejected:
  - `Integer.valueOf(7) + 1`
  - `boxed == 7`
  - `let n: Int = javaIntegerReturning()`
  - `let xs: JavaList<Int> = javaListOfInteger()`
  - `ArrayList<Int>().addAll(javaListOfInteger())`
  - `5.compareTo(3)`
- **Validation.**
  - `cargo test --release --offline` passed: 5, 1, 3 and 9 tests.
  - fmt and clippy passed.
  - The Fabric demo `dev.ps1 build` (compiler input changed, `compileMammoth` re-ran) was successful.
- **Limits.**
  - Unboxing a null into a non-null number fails with the JVM's NullPointerException (`Integer.intValue()`), not Mammoth's own message.
  - `Short`/`Byte`/`Float` remain Java objects.
- **Changed files.**
  - Source: `src/jvm/{generics,interop,builtins}.rs`.
  - Tests: `tests/projects.rs` (one message), `tests/programs/java-boxed-numbers.{mam,out}`, `tests/errors/int-missing-box-method.mam`, `tests/errors/java-boxed-list-mismatch.mam`.
  - Docs: README, CHANGELOG, PLAN, HANDOFF.

# Latest: compiler robustness (2026-11-08)

- **LocalVariableTable performance.**
  - The old `sync_locals` was O(live × table) per statement (cubic overall).
  - `Method` now tracks `open_locals: Vec<(binding index, table index)>` and `variables_low`.
  - All truncations and pops of `variables` go through `truncate_variables` / `pop_variable`. The parameter slot rewrite in method prologues lowers `variables_low`.
  - `snapshot`/`restore` save both fields.
  - Sync reconciles only bindings at or above the low-water mark.
  - Verified byte-identical class output for all 108 programs and examples, comparing against a separately built pre-change compiler.
- **Lambdas for erased generic parameters.** `java_argument_cost` gives `Argument::Lambda` cost 6 for a `java/lang/Object` target, after SAM targets at cost 1. Emission passes the generic parameter kind as the lambda's expected type. Raw `Object` parameters need typed lambda parameters.
- **Diagnostics.** `Parser::error` substitutes a block-comment explanation when the error position is at, or one character after, `/*`.
- **Fuzzing.**
  - `tests/programs.rs` `truncated_programs_fail_cleanly`: 7 cuts per program and example, exit code 0/1 only, no "panicked", 20 s hang limit.
  - The ad hoc seeded fuzzer is `scratchpad/fuzz.py`, not in the repo. Seeds 1 (truncate), 7 and 13 (mutate) found 0 problems.
- **Probed and already handled.** Paren nesting of 3k and 50k (64-level limit), 3k nested blocks, a 100k-term sum, 70k-byte strings, 20k-statement methods, unterminated strings, invalid UTF-8, empty files and huge integers. The exceptions/lambdas/generics stress program verifies with correct output.
- **Validation.**
  - `cargo test --release --offline` passed; programs now has 3 tests.
  - fmt and clippy passed.
- **Limits.**
  - Calling a call result directly, as in `f()()`, is a syntax error.
  - Lambdas passed to raw `Object` parameters need parameter types.
  - Deeper semantic fuzzing (type-directed generation) is not done.
- **Changed files.**
  - Source: `src/jvm.rs`, `src/jvm/{branching,collections,exceptions,interop}.rs`, `src/syntax.rs`.
  - Tests: `tests/programs.rs`, `tests/programs/java-lambda-elements.{mam,out}`, `tests/errors/block-comment.mam`, `tests/errors/java-lambda-element-result.mam`.
  - Docs: README, CHANGELOG, PLAN, HANDOFF.

# Latest: checked Java generic casts and initialization order (2026-11-08)

- **Casts.**
  - `classes.rs` `check_runtime_target` calls `java_parameterized`, then `implied_java_target`.
  - Upcast: some `java_generic_views(subject)` view's `substitution` to the target class agrees with the target arguments.
  - Downcast: the target class is instantiated with `Types::cast_markers` (hidden parameters keyed at site `(usize::MAX, usize::MAX)`) and viewed as the subject class. Every marker must be mentioned (`Types::mentions`); markers are then replaced and compared with the subject's own substitution.
  - `java_generic_views` is now `pub(super)`.
- **Initialization order.**
  - `Emitter.pending_fields` is filled before the constructor's initializer loop and drained as each field is stored.
  - `Emitter.pending_statics` works the same way in `statics.rs` `<clinit>`.
  - Checks: `load` (bare name) via `check_initialized`, `field_access` with a `this` receiver, and `read_static_field` (bare or `Class.x`).
  - `lambda` now saves and clears both lists around `lambda_body` (the old `lambda`).
- **Reproduced before fixing.**
  - `N.anything() as ArrayList<Text>` caused a ClassCastException at use, and `is JavaList<Text>` was true for a list of Int.
  - Forward reads: Text → NPE; Int → silently 0 (`a = b + 1` printed 1). This applied to instance, static, init and static init, and to `this.` and `S.` forms.
- **Audited and not changed.**
  - Boxed parameter conversions are rejected appropriately.
  - Passing `Int?` to an `int` parameter is rejected.
  - Typed generic alias mixing is rejected.
  - Raw Java parameters can pollute typed collections (as Java/Kotlin; documented "raw = unchecked").
  - `Object[]` covariance gives ArrayStoreException (Java semantics).
  - Java arrays with null elements are read as non-null (no per-element guard, for cost).
  - Indirect this-escape (methods called from initializers, a superclass constructor calling an overridden method) still sees null/0, as in Java/Kotlin.
  - Java `Integer` results remain Java objects (no arithmetic without `intValue()`).
- **Validation.**
  - `cargo test --release --offline` passed: 5, 1, 2 and 9 tests.
  - fmt and clippy passed.
  - Fabric `dev.ps1 build` (with verifyMod) was run; see the newest note below for its result.
- **Changed files.**
  - Source: `src/jvm.rs`, `src/jvm/{classes,generics,statics,lambdas}.rs`.
  - Tests: `tests/programs/initialization-order.{mam,out}`, and 9 files under `tests/errors/` (`field-forward-reference*`, `init-block-forward-reference`, `this-forward-reference`, `static-forward-reference`, `static-init-forward-reference`, `java-generic-cast-unchecked`, `java-generic-is-unchecked`, `java-generic-cast-mismatch`).
  - Docs: README, CHANGELOG, PLAN, HANDOFF.
- **Next.** Priority 5: robustness (malformed input, deep nesting, JVM limits).

# Latest: expected types through inherited, super and safe Java calls (2026-11-08)

- **Expected-type threading.** Each of these now passes `expected` into `java_arguments_expected`:
  - `java_inherited_call` and `java_super_call` take `expected`.
  - `call_method_on_expected` passes it for Mammoth-subclass receivers.
  - `super_call_expected` passes it.
  - `safe_access_expected` now uses `call_method_on_expected` with `expected.non_null()`.
  - Implicit calls: `call` delegates to a new `call_expected`. `expression_as` routes `Expr::Call` with an expected type there, and the type reaches only `implicit_this_call` → `java_inherited_call`.
  - `native_classes::applied_method` passes `None`.
- **Super nullability.** `java_super_call` now takes `allow_null` from the context. Before, it was hard-coded to `true`, so `return super.x()` into a non-null result failed with a nullability error.
- **Inherited statics.**
  - `java_methods(statics)` now walks the superclass chain (interfaces stop the walk; interface statics are not inherited).
  - `java_methods_visible(.., protected)` includes protected methods.
  - `java_inherited_static_call` (bare name inside a class with a Java root, after `implicit_this_call`, when no Mammoth function, type or local has that name) and `java_static_invoke` (uses the chosen method's declaring owner) are shared with `java_static_call`.
- **Reproduced before fixing.** Inherited, implicit, `super.` and both `?.` forms of `contextualEmpty()` with a `JavaList<Text>` destination all failed; the direct Java receiver worked. A bare `listIdentity(...)` inside a subclass reported "not defined".
- **Validation.**
  - `cargo test --release --offline` passed: 5, 1, 2 and 9 tests.
  - fmt and clippy (`-D warnings`) passed.
  - No Minecraft launch.
- **Limits.**
  - A top-level Mammoth function shadows an inherited Java static method of the same name. Java resolves the other way.
  - Expected types do not flow into the receivers of chained calls: `a().b()` gets context only for `b`.
  - Overload specificity is still erased.
- **Changed files.**
  - Source: `src/jvm.rs`, `src/jvm/{interop,classes,branching,native_classes}.rs`.
  - Tests: `tests/java/generic-super/{lib/Holder.java,main.mam}`, `tests/projects.rs`.
  - Docs: README, CHANGELOG, PLAN, HANDOFF.
- **Next.** Priority 4: audit casts, boxed primitives (Java `Integer` results stay Java objects), nullability and mutable aliases.

# Latest: candidate-specific nested inference and Java wildcard capture (2026-11-08)

- **Nested overload context.**
  - `interop.rs` `candidate_kinds` runs per applicable candidate, after erased applicability.
  - It re-probes only context-sensitive arguments (`Call`, `MethodCall`, `List`, `When`, `Elvis`, `Safe`), only where the candidate gives a parameterized hint that differs from the first probe.
  - A refinement is kept only if its descriptor matches, so costs and specificity are unchanged.
  - A refinement error eliminates the candidate.
  - `java_probe_hints` takes `spread: Option<bool>`; a known spread hints varargs elements.
  - `Emitter.candidate_depth` bounds nesting to 4.
  - The chosen candidate's refined kinds feed final inference.
- **Varargs and wildcard parameter checks.**
  - `varargs_array` receives the element's generic formal.
  - `check_emitted_argument` uses `java_signature_accepts` for formals with wildcards; otherwise it uses `check_type_argument`, as before.
  - `java_signature_accepts`' `? super` branch accepts boxed equivalents (`boxed_same`).
  - This fixed a pre-existing false rejection: `List<? super Integer>` rejected `ArrayList<Int>`.
- **Wildcard capture.**
  - `Types::result_kind(gtype, map, site)` turns wildcards into hidden `TypeParameter`s named `"?"` (upper bound) or `"? super"` (lower bound, in `captures` key.4).
  - Captures are keyed by `(line_starts pointer, offset, ordinal, bound, lower)` in the new shared `captures` table, so probe and emit agree. Fresh-per-evaluation captures broke `collect(Collectors.toList())`.
  - Used for Java method results (`generic_result`), inherited Java field reads, and new `java_field_result` (static and Java-object instance fields; before, these were raw).
  - `fits` accepts a lower capture's bound.
  - `has_capture` / `capture_lower` helpers.
  - `argument_mismatch` gives the wildcard message for writes into upper captures.
  - Lambdas with an inferred result reject capture types (`WILDCARD_ESCAPE`).
- **Reproduced before fixing.**
  - Overloaded nested factories: 9 cases, all "does not match the Java type's type argument".
  - `lower(ArrayList<Int>())`.
  - Heap pollution with `List<? extends Number>.add(BigInteger/Double)`, `List<?>.add(Text)`, assigning to `JavaList<Number>`, generic static fields, and Java-object fields. Each produced ClassCastException in Java.
- **Validation.**
  - `cargo test --release --offline` passed: unit 5, annotations 1, programs 2 (all programs and error fixtures), projects 9.
  - `cargo fmt --check` passed.
  - `cargo clippy --offline --all-targets -- -D warnings` passed.
  - No Minecraft launch: compiler-only change.
- **Limits.**
  - Mammoth cannot write wildcard types, so these are conservatively rejected:
    - reassigning a capture-typed `var` from another call;
    - storing a capture in a lambda with an inferred result;
    - writing captures into Java-declared wildcard fields;
    - `List<List<? extends N>>` parameters, which no Mammoth value can satisfy.
  - `? super` captures do not use the class parameter's declared bound when read (they read as `Any`).
  - Overload specificity still compares erased parameters. `overloaded(Collections.emptyList())` with `List<String>` and `Collection<Integer>` picks List, where javac reports ambiguity.
  - Java `Integer` results remain Java objects, not Mammoth `Int` (`let n: Int = javaIntegerList.get(0)` is rejected; this predates this milestone).
- **Changed files.**
  - Source: `src/jvm.rs`, `src/jvm/{interop,generics,java_fields,bounds,lambdas,project}.rs`.
  - Tests: `tests/java/generic-super/{lib/Holder.java,main.mam}`, `tests/projects.rs`, `tests/programs/java-overload-wildcards.{mam,out}`, `tests/errors/java-wildcard-lower-write.mam`, `tests/errors/java-nested-overload-mismatch.mam`.
  - Docs: README, CHANGELOG, PLAN, HANDOFF.
- **Next.** Expected-type propagation through inherited Java methods, super calls and safe calls (priority 3).

# Latest: lambda-body overload applicability (2026-11-08)

- When several erased-compatible overloads have literal lambda arguments, first retain generic/annotation applicability checks, then trial full lambda result inference and contextual lambda-body compilation for each surviving candidate. A body that does not type-check eliminates that candidate before specificity/ambiguity selection.
- Trials use cloned Emitter and complete Method state (including constant pool/reference caches, helpers/bootstrap entries, captured/boxed locals, loops, exception regions, frames and debug tables). Shared Types interning/cache registries remain shared, but trial bytecode/capture state is discarded. Cloning is gated to multi-candidate literal-lambda calls; ordinary and single-candidate calls keep their existing paths.
- The selected call is inferred/emitted normally afterward. Candidates are never invoked at compile time. All-invalid diagnostics prefer a candidate's body error over an unrelated parameter-annotation failure. Multiple still-compatible SAM interfaces remain ambiguous.
- Regression coverage: Int/Text source results, reversed declaration order, implicit lambda parameter bodies requiring String.length or Integer.intValue, captured mutation executes once, repeated captures inside loop/try/finally remain correct, all-invalid body rejection and genuinely ambiguous matching bodies. Full release suite, formatting and Clippy required before delivery. No Minecraft launch.
- Remaining: candidate-specific nested target typing and target-dependent varargs; broader Java phased/generic most-specific semantics; inherited/safe contextual dispatch; full wildcard capture/read typing, raw/cast policies and constructed intersections. Shared compiler interning during speculation can retain unused valid type identities; large overload-set compile-time/memory costs still need profiling.

# Latest: overload specificity and ambiguity (2026-11-08)

- Factor conversion scoring into overload_score, preserving the existing Mammoth cost model and generic applicability filtering. For equal best scores, prefer fixed invocation over spread, then compare parameter subtyping rather than declaration order. Require one undominated candidate or report an ambiguous Java call.
- Specificity covers reference inheritance, reference-array covariance, primitive-array references to Object/Cloneable/Serializable, primitive widening, and expanded varargs component types (including zero supplied elements). Primitive arrays are not treated as Object[] or covariant primitive-element arrays. Matching parameter lists may use declaring-owner/compatible covariant-return specificity for inherited duplicates.
- Regression coverage: List beats Collection in either declaration order, String beats Object for null, String[] beats Object[], typed zero-element varargs, float/double conversion ties, constructor selection, and primitive-array versus Object. Reject unrelated null overloads, unrelated interfaces, crossed multi-parameter overloads and unrelated array overloads. Explicitly typed arguments disambiguate the tested interface/crossed calls.
- Full release suite plus final targeted Java edge-case checks, formatting and Clippy pass. No Minecraft launch.
- Limits: specificity is over erased parameter types among minimal Mammoth conversion scores, not the full Java phased/generic most-specific algorithm. Lambda-body candidate retry and candidate-specific nested target typing remain pending; inherited/safe call context, full wildcard capture/read types, raw/cast policies and constructed intersections also remain incomplete.

# Latest: generic-aware overload applicability (2026-11-08)

- Reproduced overload(List<String>) being selected for ArrayList<Int> and failing before a valid overload(Collection<Integer>) could be tried.
- Extracted candidate inference/validation from argument emission into infer_java_candidate. When several erased-compatible candidates exist, independently check receiver/method substitutions, input/result constraints, declared bounds and explicit lambda parameter compatibility before conversion-cost ranking. Candidate filtering does not emit arguments or probe lambda bodies. Single erased-compatible calls keep the existing full validation path without redundant prefiltering.
- Final selected candidates still perform lambda-result inference and full argument emission/checking. Existing conversion costs and fixed/spread ranking are preserved. If every candidate fails, report a generic/bound/parameter error instead of emitting a bad call.
- Added regressions for static overloads in different declaration orders, bound-invalid fallback to Object while preserving valid bound preference, constructor overloads, inherited instance overloads, fixed-generic rejection followed by a valid varargs candidate, and explicit lambda annotations selecting the compatible interface. Added all-invalid rejection coverage.
- The concrete Function<String,Integer> test exposed missing boxing of primitive lambda results when a concrete Java SAM signature names a boxed result. Compatible primitive results now box to the Java reference result (also Number); incompatible results still fail. This is scoped to Java SAM return adaptation, not general source coercion changes.
- Full release suite, final targeted Java integration cases, formatting and Clippy pass. No Minecraft launch.
- Remaining: most-specific/ambiguity rules for equal-ranked overloads; lambda-body-based candidate retry, candidate-specific nested target typing, full wildcard capture/read types, raw/cast policy and inherited/safe contextual dispatch. Generic applicability filtering is not full javac overload resolution.

# Latest: Java class/interface application bounds (2026-11-08)

- Reproduced EnumSet<Text> being accepted in a function signature. Source-written Java generic applications now check argument count and defer bound validation until Mammoth declarations are resolved, using the existing module-owned pending-bound queue.
- Retain Java class/interface formal bounds separately from inheritance signatures. Check each argument against all substituted bounds (including dependent and self-recursive forms) using the same generic signature matcher as method bounds.
- Applies to nested arguments, declared parameter/return/field types, explicit constructors, generic native bodies and Java superclasses. Raw Java types remain unchanged. This does not validate every compiler-internal inferred application or introduce Java wildcard source syntax.
- Added five rejection fixtures (enum bounds, nested invalid bounds, too many/few arguments, parameterizing a nongeneric class) and one valid declaration program with Java enum types. Expanded Java integration cases for Numeric<Number>, multiple/recursive/dependent bounds, invalid constructors/signatures/superclasses and unbounded Mammoth variables. Valid primitive boxing, bounded native parameters and Java subclasses pass strict verification.
- Full release suite, formatting and Clippy pass. No Minecraft launch for this compiler-only change.
- Remaining: generic-aware overload reinference, target-dependent varargs and inherited/safe context propagation; full wildcard capture/read types and raw/cast policy; native implemented-interface candidates and constructed intersections. Java wildcard type syntax remains unsupported.

# Latest: lambda inputs and inherited Java SAMs (2026-11-08)

- Reproduced Optional.of(3).map { x: Text -> x.length() } compiling and crashing with Integer-to-String ClassCastException. Typed lambda parameter annotations must now accept the target input type; erased Object descriptors no longer justify narrowing. Native function inputs and known Any inputs receive the same check. Deliberately raw generic Java interfaces retain the existing unchecked annotation behavior.
- Explicit Java lambda input annotations contribute upper/exact nested constraints before solving method variables. A generic Function factory can infer its input type from { text: Text -> ... }, then infer its result from the body. Bounds and argument checks remain in place.
- Resolve symbolic functional-interface arguments through inherited Java interfaces before substituting the declaring SAM's parameters/result. Supports fixed arguments and reordered variables. Also handle non-generic SAM descriptors; generic SAM methods remain deferred.
- Preserve known Any arguments when mapping Java generic results rather than collapsing them into a raw type; collapse only when every argument is unresolved.
- Coverage: reject narrowed Optional and native function inputs, accept compatible broader parameters; infer annotated Function inputs/results; inherited Parent<String,U> and reordered Parent<B,A> functional interfaces execute with strict verification. Full release suite, additional program/rejection tests, formatting and Clippy pass. No Minecraft launch.
- Remaining: Java class-application bounds; overload reinference, target-dependent varargs and inherited/safe call context; unknown implicit lambda inputs still require context/annotations; full intersections/wildcard capture and raw/cast policy. This does not establish complete Java inference or generic safety.

# Latest: nested target typing and Java lambda results (2026-11-08)

- Reproduced typed singletonList(emptyList()) failing because initial argument probing had no parameter context. Unique arity-compatible Java signatures now provide fixed-parameter hints from receiver/expected-result substitutions before probing. Overloaded candidates and varargs tails deliberately get no speculative hints; final overload/argument/bound checks remain authoritative.
- Infer Java functional-interface lambda results when their SAM declares its own generic return signature. Probe using established input types, collect result constraints across lambdas and ordinary arguments, then re-solve bounds. Exact result anchors retain normal contextual lambda checking. Current SAM helper supports methods declared directly by the functional interface; inherited SAM declarations are deferred.
- Probing snapshots/restores the enclosing Method and null/generic/inference flags. Helper methods generated during lambda probing are retained, matching the existing native generic probing strategy; lambda body side effects do not execute during compilation or gain extra runtime calls.
- New regressions: nested and deeply nested empty factories; map values with nested inferred collections; incompatible nested elements; Optional.map and Stream.map return inference; captured mutable side effects occur once; Number-bounded Supplier output; two supplier results find common parents in both orders; normal argument plus lambda result can widen together.
- Remaining: lambda input-annotation constraints, inherited SAM substitution, general overload reinference and target-dependent varargs; static/direct context through inherited/safe dispatch; Java class-application bounds; native implemented-interface candidates, full intersections/wildcard capture and raw/cast policies. This is not complete Java inference.
- Full release, formatting and Clippy required before delivery. No Minecraft launch for this compiler change.

# Latest: expected Java result inference (2026-11-08)

- Carry expected result types explicitly through Java static and direct Java-instance dispatch into method inference. Existing noncontextual entry points delegate with None; nested argument probing cannot consume/leak an outer call's result context.
- Expected results add upper/exact constraints to the same solver used for input arguments. They never overwrite argument constraints or bypass declared bounds.
- Preserve symbolic method variables while viewing a Java result through the destination's ancestor signature (List<T> -> Collection<Text> infers T=Text). No intermediate runtime casts or raw parent applications are used to invent a match.
- Regressions: typed Collections.emptyList and Stream.empty; typed function returns and arguments; widening singletonList(Dog) to List<Animal>; nested known-type calls; Collection<Text> ancestor destinations; instance factory methods, including Number-bounded results; reject singletonList(42) into List<Text>, a bounded numeric factory into List<Text>, and an invariant list identity with an incompatible destination.
- Limits: contextual constraints are implemented for Java static and direct Java-instance dispatch. Mammoth-inherited Java/super calls, safe-call wrappers, and implicit Text/Any fallback dispatch still need dedicated context propagation. Probing nested no-argument generic calls before their expected parameter type is known can still reject legal bidirectional cases. No lambda-body constraint collection, overload reinference, constructed intersections or full wildcard capture. Java class-application bounds remain pending.
- Full release suite, formatting and Clippy required before delivery. No Minecraft launch for this compiler-only change.

# Latest: dependent-bound propagation (2026-11-08)

- Feed non-self-recursive declared bounds back into method inference until substitutions stabilize. For B extends A, an inferred B contributes a lower constraint to A; nested Java bounds can infer previously unknown arguments (B extends List<A>). Multi-variable chains propagate across rounds.
- Candidate selection now filters independent and self-recursive bounds using a tentative candidate substitution. This selects an accessible CharSequence view for mixed StringBuilder/StringBuffer instead of selecting Serializable and rejecting afterward. Bounds referring to other method variables are propagated and checked afterward rather than filtered against stale substitutions.
- Iteration is bounded (4 * method-variable count + 8); nonconverging inference is rejected rather than hanging. Self-recursive bounds are validated without recursively expanding them. Existing final signature and bound checks remain in place.
- Expanded the compiled-Java Holder integration test: dependent arguments in either order, multi-variable chains, inference solely from a nested bound, mixed bounded common types in either order, and a typed collection anchoring a deliberately invalid dependent call.
- Corrected an old rejection fixture: dependent("text", 1) is legal with A inferred to a common parent. Replaced it with dependentAnchored(List<Text>, 1), which must fail.
- Remaining: finite-candidate solver, no constructed intersection types, contextual-result/lambda constraints, recursive bound constraint expansion or overload reinference. Native implemented-interface candidate enumeration and Java class type-application bounds also remain pending. Full wildcard capture/read types and raw/cast policies remain unaudited. No Minecraft launch for this compiler-only change.

# Latest: Java argument constraints and common parents (2026-11-08)

- Replaced first-argument inference with collection of lower, exact and upper constraints across all non-lambda arguments, including spread varargs and inherited Java signatures. Concrete generic arguments anchor exact equality; wildcard arguments provide directional constraints.
- Solve each method variable from satisfying candidate types, remove dominated supertypes, and break incomparable ties by stable JVM signature rather than argument order. Single-type and equality-anchored calls avoid hierarchy searches. Existing argument and declared-bound validation still run after solving.
- Candidate types include native class ancestors (with substitutions), interface ancestors for interface values and Java ancestor/interface views; Java traversal skips inaccessible candidate classes while still traversing their parents. No raw generic parent is invented when its arguments cannot be resolved.
- Regressions cover Arrays.asList(Dog, Cat) and reverse order, subtype Collections.addAll, Java choose(U,U), exact List<U> anchors in both argument orders, and StringBuilder/StringBuffer's inaccessible shared superclass.
- Limitations: this is a per-variable finite-candidate solver, not Java's full inference algorithm. It does not construct intersection types, propagate dependent bounds as constraints, solve from lambda bodies or contextual result types, or redo overload selection. Bounds are checked after a deterministic candidate is selected, so some legal bounded calls may still be rejected. Native class candidates do not yet enumerate all native implemented-interface views.
- Next: bound-aware/dependent constraint propagation and contextual/lambda constraints, plus Java class-application bounds, overload integration, full wildcard capture/read typing, raw and cast policies. Full release checks, formatting and Clippy are required before delivery; no Minecraft launch for this compiler change.

# Latest: inferred Java method bounds and ancestor inference (2026-11-07)

- Reproduced Collections.max(ArrayList<Any>) accepting Thread objects and crashing inside Java with ClassCastException. Method signatures now retain every declared bound, including secondary interfaces, recursive and dependent bounds, and validate inferred variables against them.
- Method variables are removed from the receiver substitution before inference, so a method's <T> shadows the class's T. Inference walks the declaring Java ancestor view rather than zipping the subclass's parameter positions. Tests use Flipped<A,B> extends HashMap<B,A> and a generic Java Map key method.
- Java method-bound checks recognize boxed primitive interface views and exact boxed identity. Bounded Mammoth generic values can select Java reference overloads through their declared bounds; conversion emits checkcast for the target reference type, including secondary bounds.
- Fixed an intersection-test blocker: unrelated Java interfaces both had absent native roots and were falsely treated as duplicates. Class implements and interface extends lists now compare Java binary identities or real native roots. Real duplicates still fail.
- Coverage: Collections.max/min on Text/Int and Mammoth subclasses; invalid Comparable bound; Java Number & Comparable, Runnable & AutoCloseable, dependent and incompatible recursive bounds; method variable shadowing; reordered native superclass arguments; generic forwarding through primary and secondary bounds; duplicate Java interfaces. Full release suite, formatting and Clippy pass. No fresh Minecraft launch.
- Remaining: inference is still first-match, not a full constraint solver. Audit order-dependent inference, common supertypes, contextual result inference, bounds at Java class type applications, overload selection when generic candidates differ, lambdas, full wildcard capture/read types and raw/cast policies. Do not claim complete Java generic safety.

# Latest: Java method parameter signatures (2026-11-07)

- Reproduced ArrayList<Text>.addAll(ArrayList<Int>) compiling and crashing with ClassCastException. Added validation of each non-lambda argument against the selected method's original generic signature, not only its erased JVM descriptor.
- Preserve lower wildcard bounds separately from upper bounds. Check concrete arguments invariantly, upper bounds covariantly, and lower bounds contravariantly. Resolve actual ancestor/interface views, including Mammoth subclasses. Nested invariant positions containing wildcard captures reject concrete substitutions conservatively rather than permitting list pollution.
- Spread generic varargs infer/check every trailing argument against the signature's element, covering Collections.addAll and Arrays.asList.
- Added seven rejection files, one runnable program, and expanded the compiled-Java Holder integration fixture with concrete, upper/lower, and nested wildcard parameters plus six rejections. Existing raw-only unresolved generic calls still work; known receiver constraints do not allow a raw argument to bypass their check.
- Remaining priorities: generic method inference currently uses first positional matches (no full constraint solver, bounds checking or LUB inference); audit superclass substitutions during inference, method/class variable shadowing, lambda constraints, wildcard capture/read types, raw/cast and boxed primitive paths. Super wildcard read representation is still the existing flattened bound, not a full capture type. Java generic safety remains incomplete.
- Validation: full release tests, formatting and Clippy; no fresh Minecraft launch for this change.

# Latest: Java generic assignment safety (2026-11-07)

- Reproduced a compiler-accepted ArrayList<Text> -> ArrayList<Int> assignment that caused a ClassCastException after inserting an Int through the alias.
- Parameterized Java destinations now require invariant type arguments, resolved in the destination ancestor's view. Covers variables, native function arguments/returns, nested generic types, Java superclasses of Mammoth classes, and implemented Java interfaces reached through Mammoth inheritance.
- Existing raw values cannot be assigned to parameterized destinations. Fresh Java constructors with omitted arguments can use a same-class parameterized destination, e.g. let values: HashMap<Text, Int> = HashMap(). Java constructor argument checking now receives that application.
- Removed the skip for nested Mammoth type parameters when checking erased Java arguments; ArrayList<ArrayList<T>>.add(ArrayList<Int>()) is rejected in generic code.
- Added ten rejection fixtures and one runnable regression covering safe ancestor conversions, generic identity/nesting and nullable values. Full release suite, fmt and Clippy passed. No new Minecraft launch was performed for this compiler change.
- IMPORTANT: Java generic safety is still incomplete. Concrete Java generic method parameters can skip type-argument validation; wildcards currently lose direction during signature conversion, method inference/bounds need auditing, raw APIs remain available, and casts/boxed primitive paths need review. See PLAN.md's production-readiness queue. Do not claim full type safety.

# Handoff: custom block, nested imports, float params (latest), static init and Mixin accessor, static members, debugger variable names, Java type-argument checks, generic Java superclasses, debug line numbers, protected members, richer Fabric demo

## Latest work: custom Minecraft block, nested Java imports, float parameters

- **Compiler.**
  - In `project.rs`, Java imports that fail to load retry with trailing `/` replaced by `$`, from the right.
  - In `interop.rs`, `conversion_cost(Double, Float)` is 3, ranked after an exact `double` match, and `convert_to_java` emits `d2f`.
  - Both are covered by `tests/programs/java-nested-classes.mam` plus `java-nested-missing` (in the rejection fixtures).
- **Fabric demo.**
  - `main.mam` adds `GlowBlock extends Block`, with a protected `onUse` override (chime sound and an action-bar message), registered as `mammoth_demo:glow_block` together with a `BlockItem`.
  - `checkGlowBlock()` provides the smoke data.
  - Resources: `blockstates/glow_block.json`, `models/block/glow_block.json` (cube_all with the amethyst texture), `models/item/glow_block.json`, and a lang entry.
  - `verifyMod` finds Demo classes by superclass (declaration order changed their numbers) and checks `method_55766`.
  - Smoke markers for the block passed in the real client, and no resource or model errors were logged.
  - The JAR SHA256 is recorded in `validation/README.md`.
- **Validation.**
  - The release suite passed: 54 programs, 204 rejection fixtures, 9 project/Java tests.
  - fmt and clippy passed.
  - `build`/`verifyMod` and `smokeTest` passed.
- **Limits.**
  - Block and item right-click behavior is still untested in a world.
  - There is no creative tab entry.
  - The chime sound path (`world.playSound` with float volume and pitch) is compiled but unexercised.
- **Next step.** Exercise world interactions automatically. Fabric's gametest API (`fabric-gametest-api-v1`, already in the dev runtime) or a server-side smoke run could place the glow block and invoke `onUse` and the tusk's `use` in a real world. Alternatively, add **object singletons**.

## Earlier: static init blocks, inferred static types, Mixin accessor

- **Syntax.** `Class.static_initializers` holds `Initializer::Field` and `Initializer::Block` entries in source order. `static init { ... }` is parsed and cannot be private or annotated. `statics.rs` emits `<clinit>` by walking the initializers, and blocks use `scoped_block`.
- **Inference.** In `project.rs`, an untyped static field with a non-literal value gets `Kind::Null` as a placeholder. A pass after instance-field inference resolves it with `static_kind`, or with the class's own static method result for `Call`. The scope closure sees earlier static fields. Failure reports "Give this static field a type".
- **Fabric.**
  - `src/mammoth/src/ClientAccessor.mam` is a `@Mixin(MinecraftClient)` interface with `@Accessor("itemUseCooldown")` getter/setter and `@Invoker("getWindowTitle")`. It is listed as `ClientAccessor$Interface$0` in `mammoth_demo.mixins.json`.
  - Controls casts the client and prints the accessor marker, which passed in the real client.
  - `verifyMod` checks remapped values `field_1752` and `method_24287`, and applies class-mixin checks only to non-interfaces.
  - The stack-trace marker line is computed from `Controls.mam`.
  - The JAR SHA256 is recorded in `validation/README.md`.
- **Validation.**
  - The release suite passed.
  - fmt and clippy passed.
  - `dev.ps1 build`/`verifyMod` and `smokeTest` passed, with all markers including `Accessor: cooldown=0->7, title starts with Minecraft=true`.
- **Limits.**
  - Accessor and Invoker method names are Mammoth-mangled (`$cooldown`), so Mixin cannot infer targets from names. Always give `value`.
  - Static accessors and invokers are untested.
- **Next step.** Add **object singletons** (`object Registry { ... }`, compiled as a final class with a private constructor and a static `INSTANCE`), useful for mod registries and callbacks. Alternatively, to cover more of Fabric's API, test **registering a custom block and block item**, which exercises `AbstractBlock.Settings` builders and generic registries in the real client.

## Earlier: static members

- **Syntax.** `Class.static_fields` and `Class.static_methods` are parsed from a `static` modifier in the class body. Only `private` may be combined with `static`. Enum value parsing stops at `static`.
- **Symbols** (`project.rs`):
  - `Symbols.static_methods` holds signatures resolved in the module scope (without class type parameters). Generic static methods are rejected.
  - `Symbols.static_fields` holds `StaticField` entries `{ name, kind, mutable, private }`. Their types come from the annotation or `literal_kind`.
  - Name clashes with instance members are rejected.
- **Code generation** (`src/jvm/statics.rs`):
  - `find_static_method` and `find_static_field` walk the ancestors.
  - `static_class(receiver)` recognizes a bare Mammoth class name that is not shadowed by a local, field, or global.
  - `static_call` emits `invokestatic`, and `read_static_field`/`write_static_field` emit `getstatic`/`putstatic`.
  - `static_members` writes the fields, the methods (plain names, with annotations and signatures), and `<clinit>`.
- **Where it is hooked in.**
  - `method_call_expected`, `field_access`, and `set_field` (including compound assignment) handle qualified access.
  - `call`, the `load` fallback, and the `Assign` fallback handle unqualified access inside the class.
  - Instance-field use without `this` is rejected with a static-specific message.
- **Fabric.** `ClientMixin.mam` adds the static `@Inject` into `getInstance`, a static `@Unique` counter, and a computed static final Unique Text. Static Unique initializers merge into the target's `<clinit>`, verified in the game. The stack-trace marker is now `Controls.mam:61`. The JAR SHA256 is `9AF28115B7DAB826790166C33AB3376BB934D4A1B40D5A9B4C97338F1E5C0CE8`.
- **Validation.**
  - `cargo test --release --offline` passed: 52 programs, 41 examples, 201 rejection fixtures, 26 project fixtures, and 9 project/Java integration tests.
  - fmt and clippy passed.
  - `dev.ps1 build`/`verifyMod` and `dev.ps1 smokeTest` passed, with all markers including `Static Mixin executed: tag=static-ready`.
- **Limits.**
  - No generic or protected static methods.
  - No `static init {}` blocks.
  - Enum static fields are rejected.
  - `Sub.staticOfBase` resolves through ancestors, but Java subclasses see only the declaring class's members.
  - Untyped static fields accept only literals.
- **Next step.** Support **`static init { }` blocks and static field type inference from arbitrary initializers**, reusing `static_kind` after symbols are built, as instance fields do. Then add **object/companion-style singletons** for mod registries. After that, move to **access wideners/accessors**: Mixin `@Accessor` and `@Invoker` interfaces written in Mammoth, which need interface methods with annotations and no body. Verify them in the real client.

## Earlier: LocalVariableTable (debugger variable names)

- **Data.** `Method.local_table` holds `LocalEntry { label, start, end, name, descriptor, slot }` entries, and `Method.local_table_name` is the attribute name index.
- **Bookkeeping.**
  - `Emitter::sync_locals` compares `method.variables`, skipping empty and `$` names, with the open entries. It closes those that left scope and opens new ones at the current pc.
  - It runs at the start of `mark_line` (every statement, function start, initializer, and lambda start), after `scoped_block`, and after `when`-branch scope exits.
  - Boxed lambda cells use the array descriptor.
  - `MethodState.local_table` lets `restore` truncate probe entries and reopen entries closed after the snapshot.
- **Writing.** `Method::write` closes open entries at the end of the code, drops zero-length ranges, and writes the attribute.
- **Pool names.** Matching uses the source string, because `Pool::ascii` does not deduplicate. Names use `modified_utf8`.
- **Tests.**
  - `projects.rs::debuggers_see_mammoth_variable_names` with `tests/traces/vars.mam` checks exact `javap -l` rows for parameters, loop variables, nested-block variables, lambda values, and method locals.
  - A manual `jdb` breakpoint at `Main:5` listed the arguments and locals with values.
- **Validation.** The release suite passed (8 project tests now), along with fmt, clippy, and the Fabric build and smoke test (see the session report).
- **Limits.**
  - There is no `LocalVariableTypeTable`, so generic variables show their erased type.
  - A variable starts at the next statement boundary, so it is invisible within its own declaring statement.
  - A variable declared as the last statement of a block gets no entry.
  - Slot reuse across sibling scopes is described correctly by the ranges.
- **Next step.** Move to the next mod-relevant language gap: **static members (`companion`/`static` fields and functions in classes)**. Fabric and Mixin APIs often need static fields and `@Inject` handlers that are `static` in static-target Mixins.
  1. Parse `static fun`/`static let`/`static var` inside classes.
  2. Emit `ACC_STATIC` members, using `<clinit>` for initializers.
  3. Resolve `ClassName.member` calls and fields, and reject `this`.
  4. Test with Java reflection and a real Mixin targeting a static Minecraft method.

## Earlier: type-argument checks on Java generic calls

- **Implementation.** In `interop.rs`, `java_arguments` now keeps `generic_parameters`: each parameter's substituted generic kind from the receiver map and method-signature unification. `java_argument` returns the argument's Mammoth kind. `check_type_argument` then rejects values that do not fit.
- **Scope of the check.** It applies only when the JVM parameter is an erased class type different from the substituted type (for example `Object` for `E`), and it skips:
  - lambdas
  - `Any`
  - kinds containing nested Mammoth type parameters (a bare `T` is checked)
- **What it accepts.** Null, the same kind, `fits` against the nullable expected type, or `fits` between the non-null types.
- **Tests.**
  - `tests/programs/java-type-arguments.mam` covers maps, subclasses, nullable values, null, `addAll`, a generic `Bag<T>` storing `T` in an `ArrayList<T>`, and `add(index, value)`.
  - Five `java-type-argument-*` rejection fixtures.
- **Validation.**
  - The full release suite passed: 51 programs, 40 examples, 188 rejection fixtures, 7 project tests.
  - fmt and clippy passed.
  - All `examples/*.mam` compile.
  - The Fabric demo `build`/`verifyMod` and real-client `smokeTest` were run again (see the session report).
- **Limits.**
  - Java generic method type variables bound only by the arguments are unified rather than checked.
  - Index assignment (`map[key] = value`) already had its own check.
  - Values coming out of Java collections are still guarded for null when read.
- **Next step.** Emit `LocalVariableTable` so debuggers show Mammoth variable names:
  - record each `Binding`'s slot, name, descriptor, and live code range in `Method`, starting when the variable is declared and ending at scope exit
  - write the attribute in `Method::write`
  - verify with `javap -l` and a jdb or IntelliJ breakpoint

## Earlier: generic Java superclasses

- **Resolution.** In `project.rs`, the "Generic superclass types are not supported yet" rejection is gone. `java_supers` stores the applied `Kind::Java` index.
- **Substitution.**
  - `Symbols::java_super_view(receiver)` in `classes.rs` returns the top Java superclass with the receiver's type arguments substituted. It goes through `class_view` and `class_map`, so `Bag<Int>` extending `ArrayList<T>` gives `ArrayList<Int>`.
  - `java_inherited_call` now takes the receiver kind and sets `generic_receiver` to that view. `java_super_call` does the same with the current class's self type. Existing `java_arguments` substitution then types results and lambdas.
  - In `java_fields.rs`, `java_instance_field_info` returns field signatures. `Symbols::java_field_generic` substitutes them; reads install it as `last_generic_result` (checkcast), and writes use it as the expected type. Read and write take the receiver kind.
- **Overrides.** `check_generic_java_override` (in `check_classes`) parses each compatible inherited Java method's signature, skips generic methods, and requires substituted parameters to be equal and the result to fit. Primitive results must match exactly.
- **Bytecode.** The `emit_class` class-signature condition also fires for a parameterized Java superclass.
- **Tests.**
  - `tests/programs/generic-java-superclass.mam` (also an example): `ArrayList<Text>` subclasses use overrides, `super`, `removeRange`, and `modCount`, and a generic `Bag<T>` works too.
  - `tests/java/generic-super/{lib/Holder.java,main.mam}` with `projects.rs::mammoth_extends_generic_java_classes`: javac, a CLASSPATH compile, strict runs, a Java caller using `Holder<Integer>` and `ArrayList<String>`, `getGenericSuperclass`, and four rejection cases.
  - Three new error fixtures.
- **Validation.** `cargo test --release --offline` passed: 5 unit, annotations 1, programs 2 (50 programs, 40 examples, 183 rejection fixtures), and projects 7. `cargo fmt --check` and clippy passed. The release compiler was rebuilt. The Fabric demo was rebuilt and smoke-tested again with this compiler (see below).
- **Limits.**
  - Arguments to inherited Java methods are not checked against type arguments; this is the existing lenient Java interop.
  - Generic Java methods are skipped by the override check.
  - Wildcard superclass arguments are not supported, and Java superclass bounds are not checked against type arguments.
- **Next step.** Check Java type arguments at call sites. In `java_arguments`, when `expected` comes from a substituted type variable, reject arguments that do not fit (behind clear errors), with a migration test for existing programs. Then add `LocalVariableTable` emission for debugger variable names.


## Latest work (2026-11-07)

### 1. Source-aware JVM debugging metadata

- **Parsing.** `syntax::Program` has two new fields: `source_name`, set by `main.rs` and `project.rs` via `project::source_name(path)`, and `line_starts`, computed in `parse`.
- **Emission.**
  - `emit_class` and `emit_interface` write a `SourceFile` class attribute. `emit_interface` now takes the `Program`.
  - `Method.lines` holds `(pc, line)` pairs, written as `LineNumberTable` in `Method::write`. Its name constant is interned lazily in `Emitter.line_table_name`.
  - `Method::restore` drops entries beyond the restored code length, so inference probes leave no stale entries.
- **Where lines are marked.** `Emitter::mark_line(method, offset)` runs:
  - for each statement in `block`
  - at the start of functions, methods, and interface defaults, and at the constructor's class name
  - for each field initializer
  - at each lambda's start and at its result expression
- **Bridges.** Callback, native, and Java bridges call `mark_bridge`, which uses `Emitter.bridge_offset`. The line is the forwarded method's declaration, or the class name for inherited bridges.
- **Tests.** `tests/projects.rs::stack_traces_name_mammoth_sources` uses the fixture `tests/traces/project`, which is not auto-run. It checks exact `file:line method` frames for:
  - an interface default
  - methods
  - a constructor field initializer calling an exported function
  - a lambda, across two modules
  - an uncaught single-file trace (`at Main.fail(crash.mam:3)`)
- **Mixin initializers.** Line numbers fix nondefault Mixin field initializers. With line numbers, `@Unique mammothBudget: Int = 40` and `mammothTag: Text = "tusk-" + "ready"` ran in the real client. A controlled rebuild without line tables crashed with a null `mammothTag`, which proves the dependency. That rebuild used a temporary environment switch, removed afterwards. Only these shapes are verified.

### 2. Protected methods

- **Syntax.** `Function.protected`. The parser accepts `protected fun` in classes, including `protected abstract` and `protected override`. `protected export` is rejected, and interfaces and top-level functions cannot use it.
- **Bytecode and symbols.** `Symbols.protected_methods: Rc<Vec<HashSet<&str>>>` is built in `project.rs`. Emission uses `ACC_PROTECTED` for both concrete and abstract methods.
- **Access checks.** `Emitter::check_protected_method` is called from `find_class_method`, which every explicit, implicit, generic, and applied class call goes through.
  - The caller must be the owner or a subclass.
  - The receiver's class must be the calling class or a further subclass, as for JVM protected access.
  - `super` calls and lambdas inside the subclass work.
- **Override rules** in `check_classes`:
  - protected to public is allowed
  - public to protected is rejected
  - a protected method overriding a public Java method is rejected
  - protected methods cannot satisfy Mammoth or Java interface methods
- **Java protected methods.** Calls to and overrides of inherited Java protected methods, such as `Observable.setChanged` and `clearChanged`, already worked. Outside calls are rejected.
- **Tests.**
  - `tests/programs/protected-methods.mam`, also an example: abstract, generic, widening, lambda, and `super` cases.
  - `java-protected-methods.mam`.
  - The `tests/projects/protected-methods` project spans packages `parent` and `child`, with a generic abstract base, bridges, and a public widening. Its counterpart `reject-protected-method` must fail.
  - Ten `protected-method-*` fixtures plus `java-protected-method-outside`.
  - `java_subclasses_use_protected_methods`: a Java subclass in the default package overrides `$step(int)`, so dispatch flows through the generic bridge, and calls the inherited generic `$run`. javac rejects an unrelated caller.

### 3. Java-declared protected fields (`src/jvm/java_fields.rs`)

- **Lookup.** `Types::java_instance_field_member` walks the Java superclass chain for the nearest non-static field.
- **Rules** in `Emitter::inherited_java_field`:
  - public fields are allowed
  - protected fields require the current class to extend the declaring Java class, and the receiver to be the current class or a subclass
  - private and package-private fields are rejected
  - final fields are read-only
- **Reads and writes.** Reads use `convert_from_java`, which keeps the usual non-null guard. Writes narrow with i2b, i2s, and d2f.
- **Where it is wired in.**
  - implicit reads: the `load` fallback
  - implicit assignment: the `Assign` fallback, so `count = count - 1` and `count += 1` work
  - `obj.field` reads: `field_on`
  - `obj.field = v`: `set_field`. Compound assignment with an explicit receiver is rejected with guidance.
- **Generics.** Java superclasses cannot be generic yet; the existing "Generic superclass types are not supported yet" limit still applies. No generic substitution applies, and field types use erased descriptors.
- **Tests.** The `java-protected-fields` program (`ByteArrayOutputStream.count`, `FilterOutputStream.out`) and six rejection fixtures.

### 4. Fabric demo (`examples/fabric-demo`)

- **Item.** `main.mam` defines `TuskItem extends Item`:
  - `hasGlint` always returns true
  - `use` sends an action-bar message and starts a 20-tick cooldown
  - a protected `rumble()` helper
  - registered with `maxCount(16)`
  - `checkTusk()` calls `hasGlint` through the `Item` type, reaching the remapped override
- **Mixin and smoke markers.** `ClientMixin.mam` adds nondefault `@Unique` fields. Controls prints the new markers, and `traceSource()` reports `Controls.mam:57` from a caught exception.
- **verifyMod** additionally checks:
  - TuskItem's superclass `class_1792`
  - the remapped `method_7886` and `method_7836` overrides
  - SourceFile in every `demo/` class, and line numbers in every method that calls code
- **Validation.** `validation/` is updated. The new JAR's SHA256 is `9AF28115B7DAB826790166C33AB3376BB934D4A1B40D5A9B4C97338F1E5C0CE8`.

### Validation results (exact)

- `cargo test --release --offline` passed:
  - 5 unit tests
  - annotations 1/1
  - programs 2/2: 49 programs, 39 examples with output files, 180 rejection fixtures
  - projects 6/6: 25 project fixtures, the trace fixture, the four existing Java integration tests, and the new trace and protected tests
- `cargo fmt --check` and `cargo clippy --offline --all-targets -- -D warnings` passed.
- The release compiler `target/release/mammoth.exe` was rebuilt.
- **Fabric.**
  - `dev.ps1 build` passed, including `verifyMod` ("Verified remapped Mammoth mod", BUILD SUCCESSFUL).
  - `dev.ps1 smokeTest` passed under `-Xverify:all` ("Real Minecraft client smoke test passed"). All eleven markers were present, including:
    - `Unique initializers: budget=40, tag=tusk-ready`
    - `Tusk behavior: maxCount=16, glint=true`
    - `Stack trace source: Controls.mam:57`
  - The Java 21 JDK came from the existing `examples/fabric-demo/.tools` folder, and the Gradle and Minecraft caches were already present.

### Remaining limitations

- **Debug info.**
  - There is no `LocalVariableTable`, so debuggers show no Mammoth local names.
  - Lines are recorded per statement, not per subexpression.
  - Empty module entry classes have no lines.
- **Protected visibility.**
  - It is class-and-subclass only, with no package access.
  - Protected constructors are not supported, nor protected members on interfaces or top-level functions.
  - There is no `internal` visibility.
- **Java protected fields.**
  - Generic Java superclasses are not supported, so field types are not substituted.
  - Compound assignment through an explicit receiver is not supported.
- **Fabric.**
  - TuskItem's `use` (right-click) behavior is compiled and remap-checked but untested in a world.
  - Not tested: a production launcher profile, a dedicated server, multiplayer, and physical keyboard input.
  - Mixin initializers are verified only for an Int literal and a Text concatenation. Initializers on `@Shadow` fields and other constructor shapes are unverified.

### Next concrete step

Support **generic Java superclasses**, such as `class Names extends ArrayList<Text>()`. Mod APIs often need them.

1. **Resolution.** In `project.rs`, replace the "Generic superclass types are not supported yet" rejection by storing the applied `Kind::Java` superclass.
2. **Substitution.** Make `java_root`, `java_inherited_methods`, and `java_instance_field_member` substitute the class's type arguments into inherited method and field signatures. Fields carry theirs in `Member.signature`.
3. **Bytecode.** Emit the class `Signature` with the parameterized superclass, and reuse `java_bridges` for erased overrides such as `add(Object)`.
4. **Tests.** Override `removeRange` and read the protected `modCount`, which is the first real generic case for Java protected members. Add javac and reflection checks.

Older notes follow.

## Latest work: annotated and protected fields

- Added `Field.annotations` and `Field.protected`; constructor `let`/`var` properties accept annotations and private/protected visibility. Bare constructor parameters cannot receive field annotations. Protected methods are explicitly rejected until supported.
- `FieldInfo.protected` carries visibility across modules/inheritance. Emission uses ACC_PROTECTED; `class_field` restricts both reads and writes to the declaring class/subclasses. In a subclass, an explicit receiver must be statically that subclass or a further subclass, as required by the JVM. Implicit `this` accesses continue through the inherited-field cache. Let fields remain final.
- `annotate_member` shares field/method attribute appending. Fields resolve annotation types in their class scope and retain Signature attributes, visibility, and initialization behavior. Constructor-property annotations apply to FIELD, not PARAMETER. Required elements, target, nested-value, and retention checks are reused.
- Java reflection tests check constructor properties, private/final field metadata, generic field signatures, CLASS/SOURCE retention with javap, and Java subclass access to protected fields. `protected-fields` program and a cross-package generic project run under strict verification. Nine rejection fixtures and `examples/field-annotations.mam` added.
- Fabric Mixin now uses a private primitive `@Shadow itemUseCooldown`, nullable final reference `@Shadow @Final options` with no initializer, and private `@Unique mammothSeen` state. The actual development-client smoke test passed with strict verification, reading cooldown 0 and confirming the actual options reference. The JAR verifier checks intermediary field mappings, descriptors, and flags.
- Expected totals: 84 programs/examples, 164 rejection fixtures, 23 project fixtures, five unit tests, four Java integration tests. Full compiler suite, formatting, and Clippy passed; extra field-retention checks passed separately. The mod JAR is rebuilt and its report/hash updated before delivery.

Remaining (at that time): static/native external field declarations, constructor/parameter/type-use annotations, and broader Mixin initializer shapes. Only the demonstrated default-valued primitive/boolean and uninitialized nullable final Shadow reference shapes are verified in the real Mixin. Production-profile, world, physical-input and server tests remain open. Debug source/line information is also still missing from generated bytecode.

Older notes below describe prior milestones.

## Latest milestone: a real Minecraft development mod

User asked to build a complete mod demonstrating the language. Added `examples/fabric-demo`, targeting Minecraft 1.21.1, Yarn 1.21.1+build.3, Fabric Loader 0.16.14, API 0.116.7+1.21.1, Loom 1.9.2, Gradle 8.12.1, Java 21. All gameplay/entry/callback/Mixin code is Mammoth, with Gradle/Groovy build logic and no Java gameplay adapter.

- `build.gradle` compiles the multi-file Mammoth project against Loom's named compile classpath to an intermediate JAR, extracts classes into the main source-set output, packages resources, and remaps the final JAR. Legacy Mixin annotation processing is disabled; Loom remaps emitted annotations. `verifyMod` reads the JAR with ASM and checks entry points, resources, mixin package isolation, the remapped target class and tick selector, and handler descriptor.
- Common `Demo.initialize` registers `mammoth_demo:tusk` and checks its actual registry ID. `Controls.initializeClient` registers a G keybinding and an actual Fabric end-tick callback. A private Mammoth method annotated with real `@Inject(..., at = @At(...))` injects at `MinecraftClient.tick` HEAD. Its class lives in `demo.mammoth.mixin`, separate from entry points/helpers: Mixin forbids directly loading ordinary classes from its reserved package.
- Real Minecraft development-client smoke test passed with `-Xverify:all`: item registered, entry point/callback registered, Mixin executed, simulated G input reached the callback, Boolean toggled, initial resource loading finished, and the game shut down cleanly. The first too-early shutdown raced TextureManager loading; waiting for `getOverlay() == null` fixed it. `smokeTest` verifies marker logs and rejects shutdown failure. Normal `runClient` never simulates input or closes automatically. No user Minecraft installation/profile was modified; development uses the example's isolated `run` folder.
- Wrapper distribution is SHA256 pinned. `dev.ps1` validates an existing Java 21 JDK or downloads/checks/extracts Adoptium locally under `.tools`, and resolves the repository compiler by default. The portable JDK prepared during this session remains in the workspace tools folder, not the source delivery. Minecraft assets/dependencies are cached under the existing Gradle user cache.
- Final production JAR was built and checked after the runtime fixes. `validation/client-smoke.txt` contains only Mammoth marker lines; `validation/README.md` records versions, artifact SHA256 and limits. The generated JAR is supplied in the example's `build/libs` folder.

Compiler bug found and fixed by the real mod: `java_arguments` previously installed `last_generic_result` before emitting arguments. A static-field argument consumed that result and was incorrectly cast (Registries.ITEM to Item); nested argument calls also consumed the outer receiver substitution and erased result information. In `jvm/interop.rs`, capture `generic_receiver` before probing, keep the inferred result local during all argument emission, and install it only when ready to consume the actual invocation result. No workaround in mod code. Two JDK-only verifier regression programs reproduce the old static-field ClassCastException and lost generic context. Full suite passed; totals are now 82 programs/examples, 155 rejection fixtures, 22 project fixtures, five unit tests, four Java integration tests. fmt and clippy passed.

Still untested: production-profile launch of the remapped JAR, physical keyboard input, item rendering/use in a saved world, dedicated server startup, and multiplayer. The item uses the vanilla bone texture and is obtained with `/give @s mammoth_demo:tusk`; no creative-tab insertion or special item behavior. Do not claim full Minecraft/Mixin parity. Next useful work: manual/production validation and field annotations/access for advanced Mixins and mod APIs.

Older notes below predate this actual-client validation.

## Latest work: nested annotation values

- `Annotation.arguments` now contains `AnnotationArgument::{Constant(Expr), Nested(Box<Annotation>), Array(offset, Vec<AnnotationArgument>)}`. This metadata-only AST avoids adding annotations to executable expression visitors. The parser shares declaration/nested argument parsing and caps nesting at 64 levels.
- `jvm/annotations.rs`: extracted `annotation_info` and `annotation_bytes`, shared by declarations and nested values. A nested annotation must match the declared Java descriptor exactly. Every nested element receives the same required/default/name/type checks as a declaration annotation.
- Nested annotations intentionally do not apply their Target/Retention as declaration rules: `@Target({})` works inside another annotation, and SOURCE/CLASS-retained nested types remain in a RUNTIME-retained enclosing value. The JVM stores them using element tag `@`; annotation arrays use `[`.
- Java singleton array shorthand works for constants and nested annotations; explicit arrays and empty arrays still work. Parenthesized literal lists from the original implementation remain accepted. An executable expression still cannot produce an annotation value.
- Class-file metadata reader depth tracking now counts a nested annotation once instead of twice, retaining its depth guard.
- Expanded the existing Java integration test with real compiled `Detail`, `At`, `Inject`, and `Envelope` annotation declarations. Reflection verifies nested SOURCE/CLASS types inside a runtime outer annotation, arrays, singleton shorthand, qualified nested names, empty arrays, nested defaults, and target-independent nested use. Eight extra nested error cases run against those declarations. Added `annotation-nesting-depth.mam` to the rejection fixture suite.
- Expected totals: 80 verified programs/examples, 155 rejection fixtures, 22 project fixtures, five compiler unit tests, and four Java integration tests. Run the full release suite, fmt, and clippy checks. No dependencies added.

Next language milestone: field annotations and the access controls/field representation needed by Mixins. Then validate an actual mod build and game launch. Existing Inject/At tests are compiled test annotations with the same relevant element shapes, not a real Mixin execution. Do not claim Minecraft/Mixin parity yet.

Older notes below describe prior milestones; nested annotation values are now implemented.

## Latest work: JVM declaration annotations

Implemented Java annotation use on classes, interfaces, functions, and methods. Current priorities remain language development and Minecraft mod compatibility; defer extension/release work.

- `syntax.rs`: `Annotation { name, arguments: Vec<(Name, Expr)> }` lists on Class/Interface/Function. Parser accepts `@Deprecated(since = "0.3")`, imported/qualified names, and named arguments. Ordinary expression ASTs represent values, but the emitter only accepts supported constants. Annotation names end at the final identifier, excluding trailing whitespace/comments. Fields and constructors are not accepted annotation targets yet.
- `classfile.rs`: retain class annotations and method AnnotationDefault presence. Parse enum/array annotation metadata for retention/targets; skip other constant payloads safely. Annotation attributes have bounded readers and a nesting limit to reject malformed lengths/deep nesting. Existing signatures are retained.
- `jvm/annotations.rs`: validate annotation class flags/public visibility, target metadata, required/default elements, names/types/counts, duplicate types/arguments; emit RuntimeVisibleAnnotations and RuntimeInvisibleAnnotations. SOURCE is checked but has no emitted annotation attribute. Default retention is CLASS. Values support strings, all primitive constants, Class literals, enum constants, and arrays. String/class/enum constants use modified UTF-8, including NUL/non-BMP characters.
- `jvm.rs`: append declaration attributes without disturbing Code/Signature/BootstrapMethods. Export and Java callback annotations go on plain Java entry points; internal implementations are unannotated when a Java entry exists. Generic interface bridges do not duplicate body annotations. Reset declaration type scopes before resolving class/interface annotations.
- `tests/annotations.rs`: compile real Java annotation declarations, compile annotated Mammoth code against them, package class files as a JAR, compile/run a Java reflection caller under `-Xverify:all`, inspect CLASS retention with javap, and exercise invalid uses. Javac uses explicit UTF-8. On this Windows sandbox, javac directory classpaths and javap JAR real-path handling caused lookup/access failures: the test uses a JAR classpath for javac and directory classpath for javap, with the same classes.
- Added `examples/annotations.mam`, its output fixture, and eight rejection fixtures. Current expected totals: 43 programs + 37 verified examples, 154 rejection fixtures, 22 project fixtures, five compiler unit tests, and four Java integration tests.

Limitations: no nested annotation values, repeated annotations, annotation declarations, field/constructor/parameter/type-use targets, processors, or Java annotation-specific compiler analyses. Use named arguments and literal constants. Generic native class literals and generic Class-element bounds need broader coverage. Annotations on inherited/generated forwarding entries are not automatically copied beyond explicit Java callback entries.

Next milestone: support nested annotation values (`@Inject(..., at = @At(...))`) with element-type checks, then field annotations/access needed for Mixins. Keep validating actual emitted JVM names and descriptors. A complete Minecraft mod has not been launched; do not claim Mixin support is complete.

## Latest work: instance exports and bound checking

User priority: develop the language and support complete Minecraft mods. Defer editor expansion and general release packaging. Prioritize useful compiler capabilities and verified behavior over line count.

- Classes now accept `export fun`, including generic methods. `Function.exported` reuses the existing flag. Parser rejects private/abstract exports and prevents export modifiers on fields/init blocks.
- `jvm.rs` emits the existing public plain-name forwarding entry for exported instance methods even without a callback interface. It checks non-null Java parameters starting at local slot 1 (slot 0 is this), preserves method Signature attributes, and avoids duplicating existing callback/Object/Java bridges. Internal Mammoth calls retain the original implementation name and dispatch. Generic class signatures are unchanged.
- `classes.rs` checks final/static Java method collisions and incompatible inherited signatures before emission. Generic class overrides and interface implementations now compare all method bounds, using both declaring-owner substitution maps and method parameter renaming (`same_method_bounds`).
- `native_generics.rs` adds `infer_call_in` with an outer substitution map. `generic_methods.rs` supplies the receiver map when inferring/checking generic method calls. This fixes calls such as `super.keep<V>(value)` when the inherited bound mentions a superclass parameter specialized to Text.
- New example: `examples/export-methods.mam`. New program: `override-bounds`. New project: `tests/projects/method-exports`. Java integration `java_calls_exported_instance_methods` tests direct calls, inherited override dispatch, reflection, generic metadata, nullable values, non-null guards after long/double parameters, and exported Runnable/toString deduplication.
- Eight new rejection fixtures cover exported private/abstract methods, final/static collisions, incompatible Java results, and changed generic method bounds (including a missing second bound).
- Expected suite totals: 43 programs + 36 output-checked examples, 146 rejected programs, 22 project fixtures, five unit tests, and three Java integration tests. Use the release suite, fmt, and clippy checks.

This does not add annotations or Mixins, rename generated classes, or validate a Minecraft game launch. Next major implementation: JVM annotation parsing, validation, and class-file emission, with proper retention, target, element values, and bridge placement. Add real Java annotation/reflection tests before attempting a Mixin. Variance in method bounds remains a known type-system gap.

The older session notes below are historical; the class override-bound gap described there is now closed for Mammoth inheritance and implementations of declared interface methods. Full compatibility with all Java generic override rules still needs broader coverage.

## Latest session: Codex, 2026-11-07

Reviewed Claude's two-session changes below and passed the release suite before editing. Implemented the previous handoff's next milestone: `super<Interface>.method()` and child-interface overrides. No dependencies were added. The active implementation remains Rust emitting JVM class files directly.

### New behavior

- A class method or interface default body can call a directly listed Mammoth parent default using `super<Parent>.method(args)`, including generic methods, inferred/explicit method arguments, void methods, and calls in lambdas.
- Qualifiers use the bare parent name, even for `Parent<Int>`. The actual type arguments come from the class/interface parent list. The parent can inherit the selected body from an ancestor.
- A qualifier cannot be indirect, a class, abstract at the selected method, or redundant beside a more specific direct parent or superclass. Qualified Java-interface defaults are not implemented.
- Child interfaces may implement inherited abstract methods or override defaults, preserving substituted parameter/result types and method bounds. Optional `override` verifies a parent declaration exists. Bodyless redeclarations remain rejected.
- Default lookup chooses maximal, most-specific declarations independently of parent order. Unrelated conflicts require a body in the child interface or class. Class conflict checks also include interfaces inherited through a superclass.

### Implementation map

- `syntax.rs`: interface methods accept optional `override`; qualified super is a borrowed `Expr::Variable` name (the same source-slice approach as generic call names), intercepted by `method_call_expected` before ordinary variable evaluation.
- `classes.rs`: `interface_method_views` filters shadowed declarations. `default_method` resolves across the class hierarchy. `check_interfaces` checks override signatures and bounds. `interface_super_call` resolves the direct qualifier, then emits `invokespecial` with an InterfaceMethodref owned by that direct interface, while adapting the declaring ancestor's signature.
- `generic_methods.rs`: special calls preserve the original direct owner before method lookup rewrites the receiver to its declaring ancestor. Special takes opcode precedence over interface dispatch. Specialized interface bodies use their boxed descriptor for the direct call.
- `bridges.rs`: `interface_body_signature` boxes primitive positions that override reference-erased parent positions. The actual interface body uses that Java-compatible entry. Synthetic primitive and erased parent entries forward to it with virtual dispatch, so Java overrides work even from a Mammoth primitive call. `native_bridges` also emits these boxed descriptors for Mammoth class overrides. Bridge targets use the pool's full descriptor lookup, bypassing the source member cache keyed by owner/name.
- `jvm.rs`: specialized interface body parameters are unboxed into fresh primitive locals. `Method.boxed_result` boxes only at return emission after the declared primitive source result has been checked, including returns through finally. It does not relax source null checking. Ordinary primitive method bodies are unchanged.
- `native_generics.rs`: `adapted_arguments_to` checks arguments against the source signature before adapting to a distinct emitted descriptor. The existing `adapted_arguments` uses the original descriptor. This prevents boxed entries from making non-null primitive arguments accept null.

### Validation and fixtures

- New programs: `parent-defaults` and `default-primitive-overrides`; strict Java verification covers generic/default dispatch, all five primitive kinds, nullable values, long/double slots, generic methods, method bounds, closures, diamonds, class overrides, and try/finally returns.
- New example: `examples/parent-defaults.mam`, expected output `Hello! Welcome to Mammoth.`
- 16 new rejection fixtures cover qualifier scope/type/directness, redundant qualifiers, missing/abstract methods, signatures/bounds, ambiguity, and primitive null rejection.
- New project: `tests/projects/parent-defaults`, with parents in `defaults.Base` and child interfaces/classes/exports in `defaults.API`.
- New integration test `java_inherits_specialized_interface_defaults`: Java inherits specialized primitive defaults, overrides them, and exercises both Java and Mammoth implementations through both specialized and generic parent views.
- The release suite now includes 42 programs, 35 examples with output fixtures, 138 rejection fixtures, 21 project fixtures, five unit tests, and two Java integration tests. Use `cargo test --release --offline`, `cargo fmt --check`, and `cargo clippy --offline --all-targets -- -D warnings`.
- Compile-only smoke comparison against Claude's installed release: seven interleaved samples per compiler/workload after a warm-up, including a fresh compiler process and file output. Checksum medians were 8.72 ms before / 8.86 ms after; a generated 200-class source was 105.59 / 107.83 ms. Ranges overlap; this small sample does not establish a performance improvement or regression, or a Java/Kotlin comparison. Checksum class bytes have identical SHA256 hashes. These measurements ran while project regression checks were active, so scheduling noise is a further limitation.

The prior session notes below are retained for the other implemented features. Their original validation counts describe Claude's earlier baseline.

Session date: 2026-11-07, second session of the day. The compiler still has no dependencies. The earlier session's work (parameterized interfaces, generic superclasses, function types of any arity, single bounds) is summarized in CHANGELOG.md.

## Completed features

### 1. Interface inheritance

- **Syntax.** `interface Stream<T> extends Source<T>, Named`. The parser stores `Interface.extends: Vec<Name>`.
- **Resolution.** In `project.rs`, parents resolve in the interface's own scope into `Symbols::interface_parents: Rc<Vec<Vec<Kind>>>`.
  - Only Mammoth interfaces are accepted, each listed once.
  - Cycles are rejected by a depth-first search after class cycle detection, using the new `interface_owners` for diagnostics.
- **Traversal helpers** in `classes.rs`:
  - `interface_ancestors(kind)` returns the interface and every ancestor, substituted and deduplicated by root.
  - `interface_method(kind, name)` finds the ancestor that declares a method.
  - `method_receiver(kind, name)` rewrites an interface receiver to the declaring ancestor. It is applied in `call_method_on`, `generic_method`, and `generic_member_result`, so calls go through `invokeinterface` on the parent, using the parent's erased descriptor and substitution.
- **Subtyping and inference.**
  - `interface_view` accepts interface values as well as classes and searches ancestors.
  - `object_is` and `require` use it, including interface-to-interface assignment.
  - Inference through `supertype_view` therefore sees ancestors too.
- **Checks.**
  - `check_interfaces` rejects incompatible inherited parameterizations and unresolved unrelated method names. The latest session permits matching overrides with bodies.
  - `check_classes` expands each `implements` entry to all ancestors. It requires every inherited method and rejects inconsistent parameterizations, including ones inherited through the superclass.
- **Bytecode.**
  - `emit_interface` writes parents into the interfaces table and into the class `Signature` (`Ljava/lang/Object;LParent<TT;>;`).
  - `native_bridges` and `inherited_bridge_names` walk ancestors, so primitive implementations of inherited erased methods get bridges.

### 2. Multiple bounds

- **Syntax.** `<T: A & B>`. The `bounds` field on Class, Interface, and Function is now `Vec<Vec<Name>>`, parsed by `Parser::bound_list`.
- **Storage.** The `Types.bounds` table is `Vec<Vec<Kind>>`.
  - `bound()` returns the first bound, which decides erasure.
  - `all_bounds()` returns every bound.
- **Rules.**
  - Only the first bound may be a class (`is_interface_bound`).
  - Duplicates are rejected by comparing erasures (`erased_bound`).
  - Every bound is checked in `check_bounds`, and `bound_fits` accepts assignment through any bound.
- **Calls.** `Symbols::member_bound` picks the bound that declares the method. `call_method_on` emits a `checkcast` when that bound is not the first one.
- **Signatures.** `formal_parameter` writes every bound: `T::LNamed;:LScored;`.

### 3. Declaration-site variance (`src/jvm/variance.rs`)

- **Syntax.** `out` and `in` are allowed on interface type parameters, as in `interface Source<out T>` (`syntax::Variance`, `Parser::variance`). A parameter literally named `out` or `in` still parses. Classes get a specific error.
- **Storage.** `Types.variances` holds one entry per type parameter index and is set in `project.rs`.
- **Position checks.** `check_variance` runs on each interface method signature. Positions flip through function parameters, follow nested interface variance, and are invariant inside lists, classes, and Java generics. There are three distinct messages.
- **Subtyping.**
  - `Symbols::applied_fits` compares arguments by variance and replaces `same_kind` in both `AppliedInterface` branches of `require`.
  - `argument_fits` compares boxed arguments, so `Int` fits `Any` and `Int?` but never `Long`.
- **No bytecode changes.**

### 4. Implied runtime checks for generic types

- **Helper.** `Symbols::check_runtime_target` replaces three duplicated rejections, in `jvm.rs` `branch_when`, `branching.rs` `when is`, and `casts.rs`.
- **Rule.** A parameterized target is allowed when `implied_target` holds. It views the target's generic self type as the subject's static type, infers its parameters, and requires every written argument to match. The emitted code is a plain `instanceof` or `checkcast`, and smart casts work unchanged.

### 5. Default interface methods

- **Parsing.** `Parser::interface` keeps a body when one follows the signature. Bodiless interface methods now have `is_abstract = true`.
- **Bytecode.** `emit_interface` builds an emitter through the shared `new_emitter`, with `main_class` cleared and `current_interface` set to the interface's self type. Each default body compiles like a class method, with `this` typed as the interface, and is written as a public non-abstract method with a `Code` attribute.
- **Lambdas and helpers.** Lambdas and generated helpers inside defaults become private static interface methods. `Emitter::own_method_tag` selects `InterfaceMethodref`, and the interface class file writes `BootstrapMethods` when needed. Lambdas capture `this` when `current_interface` is set.
- **Calls.**
  - Unqualified calls in a default go through `current_interface` (in `classes.rs`, the implicit method path).
  - Classes reach inherited defaults through `Symbols::default_method` and `has_default`.
  - `method_receiver` now also redirects object receivers whose class lacks the method to the interface view that provides the default. Implicit calls inside a class do the same.
- **Checks.** `check_classes` skips the missing-method error for defaults, and rejects two same-named defaults from unrelated listed interfaces.

### 6. Generic fixed-size `List<T>`

- **Representation.** `ListKind::Nested` already represents a list of a type parameter. Its descriptor is the array of the parameter's erasure (`[Ljava/lang/Object;` or the bound's array), and Java sees `<T> T[] f(T[])`.
- **Changes.**
  - Removed the `List<T>` rejection in `resolve_type`.
  - `Types::primitive_list_argument` (in `native_generics.rs`) is checked at the start of `adapted_arguments`, which every generic function, method, and constructor call goes through. It rejects substituting a primitive into a list's type-parameter element anywhere in a parameter type, nested positions included.
  - `list_expression` rejects list literals whose element type contains a type parameter.
- **Why this is sound without copying.**
  - Every `List<T>` array is created by concrete code with its real element class, so later checkcasts to `String[]` and similar succeed.
  - Primitive arrays never reach `Object[]` parameters.
  - Lists are invariant, which rules out `ArrayStoreException`.

### 7. Star projections (`Box<*>`)

- **Parsing and storage.** The parser accepts `*` in type argument lists. `Types::star(interface, owner, position)` interns one hidden type parameter named `*` per declaration position, in the new `Types.stars` table. `is_star` detects it.
- **Subtyping and inference.**
  - `applied_fits` (in `variance.rs`) accepts any argument for a star. It now covers `Applied` as well as `AppliedInterface`.
  - `infer_call` drops star bindings that come from the expected type.
- **Writes.** `Types::writes_star` rejects calls in `adapted_arguments`, and starred `var` field writes in `classes.rs`.
- **Bounds and runtime checks.** `check_bounds` skips star arguments. `check_runtime_target` allows all-star targets.
- **Signatures.** `signature_type` writes `*` in argument position.

### 8. Interfaces extending Java interfaces

- **Resolution.** `project.rs` accepts Java interface parents in `interface_parents`. The cycle search skips them.
- **Helpers** in `classes.rs`:
  - `java_parents(kind)` collects the Java parents reached through Mammoth ancestors.
  - `java_interfaces(class)` adds them to a class's own Java interfaces.
- **Where the helpers are used.** `object_is`, `java_bridges`, `has_java_callback`, the `check_classes` expansion of each listed interface, `method_receiver` (calls fall back to the Java parent), and `require`, where a Mammoth interface value fits a Java supertype.
- **Bytecode.** `emit_interface` names Java parents through `runtime_class_name` and emits a signature whenever a parent needs one.

### 9. Trailing lambdas on bare calls

- **Expressions.** Expression-level calls without parentheses now accept a trailing lambda for any name, not only capitalized ones.
- **Statements.** Statement-level calls accept `name { }` and `name(args) { }`.

## Validation (all passing)

- **`cargo test --release`:**
  - 40 programs and 34 examples, all run under `java -Xverify:all`
  - 122 error cases
  - 20 project fixtures
  - the javac integration test
- **`cargo fmt --check`** and **`cargo clippy --all-targets -- -D warnings`** are clean.
- **New programs.** `interface-inheritance`, `trailing-lambdas`, `multiple-bounds`, `variance`, `generic-runtime-checks`, `default-methods`, `default-lambdas`, `generic-lists`, `star-projections`, and `java-parent-interfaces`. Most are also examples.
- **New project.** `tests/projects/interface-inheritance` (module `streams`) runs across modules:
  - an interface extending interfaces from another module
  - a covariant `Source<out T>` view
  - an export with two bounds
  - a default method
  - a generic `List<T>` export
  - a `Source<*>` export
  - a `Job extends Runnable` export, which Java runs as a `Runnable`
- **Java test (`tests/projects.rs`).**
  - A Java class implements the Mammoth child interface and is passed where the parent is expected.
  - Java calls an inherited method through the parent type.
  - Java calls the two-bound export.
  - Java calls an inherited Mammoth default method.
  - Java passes a `String[]` to a `List<T>` export, and javac rejects an `int[]` (`WrongArray`).
  - javac rejects an argument that satisfies only the first bound (`WrongSecond`).
- **Release build.** `target\release\mammoth.exe` was rebuilt and the new examples ran from PowerShell.
- **No performance claims.** No code generation hot paths changed, apart from one `checkcast` for later-bound method calls.

## Remaining limitations

- **Generic `List<T>` of primitives.** Not supported: no `int[]` specialization. Generic code also cannot allocate a `List<T>`. A future design could pass a `Class` token or generate specialized copies of generic functions.
- **Variance.** Declaration-site variance applies to interfaces only, and the only use-site form is `*`. Method type-parameter bounds are not checked for variance positions. Java sees invariant interfaces, with no wildcards in generated signatures.
- **Runtime checks.** Unimplied targets, such as `value is Box<Int>` from `Any`, are still rejected. Use `Box<*>` instead.
- **Star projections.** They are not supported in `List` or `MutableList`, and there are no `out X` or `in X` projections at use sites. Values read through two different `Box<*>` share one hidden type, but every write through a star is rejected, so this cannot corrupt the heap.
- **Interfaces.**
  - Child-interface overrides need bodies; re-abstraction is not supported.
  - Qualified `super` currently targets Mammoth interfaces only, not Java interfaces.
- **Override compatibility.** Mammoth method bounds are now compared with ancestor substitutions and renamed parameters. Broader Java generic override compatibility still needs coverage.
- **Bounds.** There are no `where` clauses, and a bound cannot be another type parameter (`T: U`).
- **Static inference.** `static_kind` and `generic_result` do not check bounds. Errors appear at emission instead, still with locations.

## Next implementation priorities

1. Check declaration-site variance inside method type-parameter bounds. Add acceptance and rejection cases that demonstrate safe and unsafe assignments through a variant interface, including nested interface/function types.
2. Generalize method-bound compatibility checks across class overrides and interface implementations, carrying both ancestor substitution maps and renamed method parameters. Test both Mammoth rejection and Java generic signatures.
3. Design use-site projections and primitive generic lists before adding syntax: lists currently preserve reference-array aliasing and must not silently copy primitive arrays. Keep ordinary primitive code unboxed.
4. Continue real application validation and VS Code tooling. Existing Minecraft examples have not been verified in a real game launch.
