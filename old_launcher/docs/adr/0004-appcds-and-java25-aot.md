# ADR 0004: Fast Start via Java 19+ AppCDS and Java 24+/25 AOT Cache (JEP 483/514)

- **Status**: Accepted
- **Date**: 2026-10-16

## Context
Minecraft startup is bottlenecked by class loading and bytecode verification across thousands of library and mod classes. Modern JVMs (Java 19+ and Java 25 used by Minecraft `26.x`) include Application Class-Data Sharing (`-XX:+AutoCreateSharedArchive`) and Ahead-of-Time class loading/linking caches (`JEP 483` / `JEP 514`).

## Decision
1. In `kestryx-java::build_fast_start_jvm_args`, automatically configure per-instance `-XX:+AutoCreateSharedArchive -XX:SharedArchiveFile=<instance>/.cache/kestryx-appcds.jsa -Xshare:auto` on Java 19+, and `-XX:AOTCache=<instance>/.cache/kestryx-faststart.aot` when a trained AOT cache is present on Java 24+/25.
2. Expose this behind a per-instance and global **Fast Start** toggle with `-Xshare:auto` automatic fallback so class-path mutations never fail a launch.
