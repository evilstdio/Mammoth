# Mammoth Language for VS Code

Support for [Mammoth](https://github.com/evilstdio/Mammoth), a beginner-friendly, statically typed language that compiles to JVM bytecode and can call Java libraries.

> **Preview.** Mammoth 0.1 is an experimental preview, and the language may change between releases.

## Features

- Syntax highlighting, bracket matching and `//` comments for `.mam` files.
- A play button in the editor title bar, and **Mammoth: Run Mammoth File** in the Command Palette, which compile and run the current file in a terminal.
- Compiler errors in the **Problems** panel, linked to the line.
- **Mammoth: Check Java Bindings** checks the Java classes and methods a file declares against the JDK and your `CLASSPATH`.

## Requirements

The extension runs the Mammoth compiler, which you install separately:

- **Windows:** download `mammoth-<version>-setup.exe` from the [releases page](https://github.com/evilstdio/Mammoth/releases) and run it. Java is included.
- **Other systems:** build the compiler from source as described in the [Mammoth README](https://github.com/evilstdio/Mammoth#build-from-source), with a Java 17 or later JDK on `PATH`.

Running the compiler needs a trusted workspace.

## Settings

- `mammoth.compilerPath`: the compiler to use. When empty, the extension uses the workspace's `target/release` compiler, then the Windows setup's install folder, then `mammoth` on `PATH`.

For multi-file projects, run `mammoth --project <directory>` in a terminal.

## License

Apache-2.0 WITH LLVM-exception. See [LICENSE](LICENSE).
