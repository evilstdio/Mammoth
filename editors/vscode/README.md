# Mammoth Language for VS Code

Open a `.mam` file for syntax highlighting, bracket matching, and line comments. Build the Mammoth compiler with `cargo build --release` in the Mammoth project first. Java 17 or later must be on `PATH`.

Use the play button in the editor or run **Mammoth: Run Mammoth File** from the Command Palette. The extension uses the workspace's release compiler when it exists. In other workspaces, set `mammoth.compilerPath` to the compiler executable or put `mammoth` on `PATH`.

**Mammoth: Check Java Bindings** checks Java declarations against the JDK and `CLASSPATH`. Compiler errors appear in the terminal and the Problems panel. For multi-file projects, use `mammoth --project <directory>` in the terminal.
