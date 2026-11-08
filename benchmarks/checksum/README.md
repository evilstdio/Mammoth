# Checksum benchmark

This run used Mammoth 0.1.0 built with Rust 1.98.1 in release mode, Eclipse Temurin JDK 17.0.20.1, and Kotlin/JVM 2.4.21 on the same Windows machine. All compilers emitted Java 17 class files. The three implementations returned the same checksum.

| Measurement | Mammoth median | Java median | Kotlin median |
| --- | ---: | ---: | ---: |
| Fresh compiler process, clean output | 10.22 ms | 372.74 ms | 2,828.44 ms |
| Fresh JVM process, 1,000 iterations | 55.17 ms | 61.76 ms | 61.89 ms |
| Warm JVM, about one million iterations per call | 3.94 ms | 3.91 ms | 3.91 ms |

The compile row includes launching each compiler. It uses five runs and a new output directory each time. The cold row uses nine JVM launches. The warmed row uses three JVM processes per language, 40 warm-up calls per process, and ten batches of eight calls per process. Cold and warmed runs use the same Java harness. The cold ranges overlap. This workload exercises integer arithmetic and a loop, so it gives limited evidence about larger builds or other workloads.

The cold checksum was `729977` and the warmed aggregate was `60862356` for all three languages. See `baseline.csv` for individual measurements.

## Run again

Install a standalone Kotlin/JVM compiler and place Java 17 or later on `PATH`. From the Mammoth project folder, run the following in PowerShell.

```powershell
cargo build --release
.\benchmarks\checksum\run.ps1 -KotlinCompiler C:\path\to\kotlinc\bin\kotlinc.bat
```

The script writes fresh output and raw samples under `build/benchmarks`.
