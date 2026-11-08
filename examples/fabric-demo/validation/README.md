# Validation: 2026-11-08

Re-run on 2026-11-08 with the compiler after the wildcard-capture, inherited-call, cast, initialization-order, boxed-number and bridge milestones. `build`, `verifyMod` and `smokeTest` passed again with every marker below present; the JAR changed (new SHA256 below) because the compiler's output changed.

The actual Minecraft 1.21.1 development client launched with Fabric Loader 0.16.14, Fabric API 0.116.7+1.21.1, Java 21, and strict JVM verification (`-Xverify:all`). `smokeTest` passed and Minecraft shut down cleanly after its initial resource reload.

Runtime checks observed:

- The custom item registered with the exact ID `mammoth_demo:tusk`.
- The client entry point registered the real Fabric event callback and keybinding.
- Sponge Mixin applied the Mammoth-written injection to `MinecraftClient.tick` and executed it.
- A simulated G press through Minecraft's actual keybinding system reached the real end-tick callback and toggled the Boolean.
- The Mixin read the actual mutable cooldown and final options fields through Shadow annotations and used private Unique state.
- Nondefault Unique initializers (`mammothBudget = 40`, `mammothTag = "tusk-" + "ready"`) ran in the real `MinecraftClient` constructor. A controlled rebuild without LineNumberTable data (a temporary compiler switch, since removed) crashed instead: the initializers were not merged, the Text field stayed null, and the demo's non-null check failed. Mixin needs the line-number data to find field initializers in the Mixin constructor.
- A static Mammoth handler (`private static fun onGetInstance`) injected at HEAD of the static `MinecraftClient.getInstance()` ran, and read a static `@Unique` field whose nondefault initializer (`"static-" + "ready"`) Mixin merged into MinecraftClient's static initializer.
- A Mammoth accessor interface (`ClientAccessor`, with `@Accessor` getter/setter for `itemUseCooldown` and an `@Invoker` for the private `getWindowTitle()`) was applied to MinecraftClient. The smoke test cast the real client to it, read 0, wrote 7, read 7 back, restored the value, and called the private method.
- A custom block, `mammoth_demo:glow_block`, and its block item registered. `GlowBlock` is a Mammoth subclass of `Block` built from `AbstractBlock.Settings` (imported as a nested class) with `strength(1.5)` (a Double passed to a Java float) and a luminance lambda. The smoke test read luminance 12 and hardness 1.5 from the real block state, and checked that the block item links back to the block. Its right-click `onUse` override (protected in Minecraft) compiles to the remapped `method_55766` but needs a world to exercise.
- The custom `TuskItem` subclass of Minecraft's `Item` answered `hasGlint` through the Item type (the remapped override) and stacked to 16.
- An exception thrown and caught inside Mammoth code reported the exact line of its `throw` in `Controls.mam` (the smoke check reads that line number from the source), the exact source line, from the packaged mod classes.
- All required markers were present in the game log. See `client-smoke.txt`.

`build` also passed `verifyMod`, inspecting the final remapped JAR: GlowBlock's superclass `class_2248` and remapped `onUse` override, the glow block's blockstate/model/translation resources, the accessor interface's remapped `@Accessor` (`field_1752`) and `@Invoker` (`method_24287`) targets, the static getInstance handler (`method_1551`, static, CallbackInfoReturnable) and its static final Unique field, TuskItem's superclass and remapped `hasGlint`/`use` overrides (`method_7886`, `method_7836`), SourceFile and LineNumberTable data in every class method that calls code, public static entry points, isolated Mixin package, Minecraft client target `net/minecraft/class_310`, injection selector `method_1574()V`, CallbackInfo handler descriptor, remapped Shadow field names/descriptors and Unique/Final flags, and item resources.

Final JAR: `build/libs/mammoth-fabric-demo-0.1.0.jar`

SHA256: `918305D53C55296CCE8F6406D1E72648CF187B64C5FBD0F4C348438B36F7463A`

This validates the development client and packaged bytecode. A normal launcher/profile run of the production JAR, physical keyboard input, item appearance/use in a saved world, dedicated server startup, and multiplayer remain untested. The item uses a vanilla bone texture. Its right-click `use` behavior (message and 20-tick cooldown) compiles to the remapped override but needs a world to exercise and has not been tested in game. No creative-tab insertion is included.

The real launch exposed and led to a compiler fix: generic Java call results and receiver substitutions must be scoped to their call while nested arguments are probed/emitted. Two JDK-only regression programs reproduce the former incorrect static-field cast and lost generic context. The compiler's full release suite, formatting, and Clippy checks passed after the fix.

