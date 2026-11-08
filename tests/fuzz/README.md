# Fuzzing and adversarial tools

These scripts are for deeper runs than `cargo test`. Build a release compiler first, or set `MAMMOTH` to another compiler. A debug compiler also runs the operand-stack assertions.

- `difffuzz.py SEED COUNT` generates the same random program in Mammoth and Java and compares output and crash status under `-Xverify:all`. `tests/fuzz.rs` is a small seeded version that runs in `cargo test`.
- `soundfuzz.py SEED COUNT STEPS` grows programs from random, usually ill-typed statements and class declarations. Anything the compiler accepts must not raise a type or linkage error that its source did not ask for.
- `minimize.py FILE [TEXT]` removes lines while the program still compiles and its failure still contains `TEXT`.
- `advcases*.py` and `jdkcases.py` are hand-written batteries. `R_` cases should be rejected and `A_` cases should run. `jdkcases.py` lists common JDK usage.

Failing inputs are written next to `difffuzz.py` and `soundfuzz.py`; delete them when done.
