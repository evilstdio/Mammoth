# Mammoth Language for VS Code

Open a `.mam` file for syntax highlighting, bracket matching, and line comments. Install Mammoth with its Windows setup, which includes Java, or build the compiler with `cargo build --release` and put a Java 17 or later JDK on `PATH`.

Use the play button in the editor or run **Mammoth: Run Mammoth File** from the Command Palette. The extension uses `mammoth.compilerPath` when set, then the workspace's release compiler, then the Windows setup's install folder, then `mammoth` on `PATH`.

**Mammoth: Check Java Bindings** checks Java declarations against the JDK and `CLASSPATH`. Compiler errors appear in the terminal and the Problems panel. For multi-file projects, use `mammoth --project <directory>` in the terminal.
