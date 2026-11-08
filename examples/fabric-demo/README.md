# Mammoth Fabric demo

A Minecraft **1.21.1** mod whose gameplay code, entry points, Fabric callback, and Mixin are all written in Mammoth. Gradle/Loom supplies the normal Minecraft dependency and remapping pipeline. There is no Java gameplay adapter.

The demo registers a glowing block, `mammoth_demo:glow_block`, with its block item (a Mammoth `GlowBlock extends Block` that chimes when right-clicked), and registers `mammoth_demo:tusk` as a Mammoth subclass of Minecraft's `Item` (it always has the enchantment glint, stacks to 16, and on right-click shows a message and starts a one-second cooldown), adds a G-key toggle under the Mammoth controls category, handles Fabric client end-tick events, and injects a method at the head of `MinecraftClient.tick`. The Mixin also reads the real mutable cooldown and final options fields through `@Shadow`, and keeps private `@Unique` state, including fields with nondefault initial values. A second, static handler injects into the static `MinecraftClient.getInstance()` and uses a static `@Unique` field. An accessor Mixin interface, `ClientAccessor.mam`, reads and writes a private field with `@Accessor` and calls a private method with `@Invoker`.

## Build and run on Windows

Build the Mammoth compiler first from the repository root with `cargo build --release`, or use the updated release executable already supplied with this project. From this folder:

```powershell
.\dev.ps1 build
.\dev.ps1 runClient
```

The helper uses a Java 21 JDK from `JAVA_HOME` or prepares a local Eclipse Adoptium JDK under `.tools`, checking its published SHA256. It does not change your system Java installation. Gradle 8.12.1 is pinned by the wrapper and its distribution checksum. The first build/launch downloads Minecraft libraries and assets and can take several minutes.

To choose an existing JDK/compiler:

```powershell
.\dev.ps1 build -JavaHome 'C:\path\to\jdk-21' -Compiler 'C:\path\to\mammoth.exe'
```

With a Java 21 JDK configured, the Gradle wrapper also works directly on other platforms:

```sh
./gradlew -PmammothCompiler=/absolute/path/to/mammoth build
```

## Try the mod

The packaged, remapped mod is `build/libs/mammoth-fabric-demo-0.1.0.jar`. Use it with Minecraft 1.21.1, Java 21, Fabric Loader 0.16.14 or later, and Fabric API for 1.21.1. The tested API version is 0.116.7+1.21.1. Copy only the remapped JAR from `build/libs`, not the intermediate `build/mammoth/raw.jar`.

For a normal client, install the Fabric profile and matching Fabric API through your usual launcher. For development, `runClient` launches the isolated `run` directory without modifying your normal Minecraft installation.

In a world with commands enabled:

```text
/give @s mammoth_demo:tusk
/give @s mammoth_demo:glow_block
```

The custom item is named **Mammoth Tusk** and uses Minecraft's bone texture with a glint. Right-click it to see the rumble message in the action bar. It is available through the command; this demo does not add it to a creative inventory tab. Press G to toggle the demo Boolean and see the result in the game log. This is a demonstration toggle, not a gameplay cheat.

## Automated checks

```powershell
.\dev.ps1 verifyMod
.\dev.ps1 smokeTest
```

`verifyMod` inspects the production JAR with ASM. It checks public Fabric entry points, resources, the separate Mixin package, the remapped Minecraft client target, the remapped tick selector, the injection descriptor, and the remapped Shadow/Unique/Final fields, the item's remapped `hasGlint`/`use` overrides, and SourceFile/LineNumberTable data in the Mammoth classes.

`smokeTest` launches the actual development client with strict JVM verification. It verifies item registration and the item's stack size and glint override at runtime, checks that a Mammoth exception reports its `.mam` file and line, checks nondefault `@Unique` initializers, queues one simulated G-key press inside the real keybinding system, checks the Fabric callback, injected Mixin, and Shadow field reads, waits for the initial resource overlay to finish, then closes the client. It requires a working graphical environment. A normal `runClient` does not simulate input or close automatically. The game-execution task has a three-minute timeout in smoke mode; asset downloading happens before that task.

The successful marker log is written to `build/reports/mammoth-smoke.txt`. A smoke run validates development-client behavior, not multiplayer compatibility, a dedicated server launch, physical keyboard input, or item appearance in a saved world. Those remain manual tests.

## Source layout

- `src/mammoth/main.mam`: common initializer, the `GlowBlock` and `TuskItem` classes, and their registration.
- `src/mammoth/src/Controls.mam`: client initializer, event callback, keybind state, and smoke checks.
- `src/mammoth/src/ClientAccessor.mam`: an `@Accessor`/`@Invoker` interface Mixin. Use it with `client as ClientAccessor`.
- `src/mammoth/src/ClientMixin.mam`: private injection handler with real Sponge Mixin annotations.
- `src/main/resources`: Fabric metadata, Mixin configuration, translations, and the item model.

Mixin classes live in `demo.mammoth.mixin`, separate from entry points and helpers. Mixin reserves its configured package and forbids directly loading normal classes from it. The Mixin configuration names Mammoth's generated `ClientMixin$$0` class and `ClientAccessor$Interface$0` interface explicitly; update this if changing the module or declaration order.

Loom remaps the compiler output from Yarn names to intermediary names. Legacy Mixin annotation processing is disabled because Mammoth emits annotation bytecode directly. The generated JAR is checked after remapping; it is not enough for an annotation to look correct in source.

Versions are intentionally pinned: Minecraft 1.21.1, Yarn 1.21.1+build.3, Loader 0.16.14, Fabric API 0.116.7+1.21.1, Loom 1.9.2, Gradle 8.12.1, Java 21. Upgrade them together and rerun both checks.

## Mixin field initializers need line numbers

Mixin merges the field initializers of a Mixin class into the target's constructors, and it finds them using the constructor's line-number table. Mammoth class files now carry SourceFile and LineNumberTable attributes, so `@Unique private var mammothBudget: Int = 40` really starts at 40 in the game. A controlled build without line numbers left such fields unset, and the smoke test failed. Only the field shapes in `ClientMixin.mam` are verified in the real game.
