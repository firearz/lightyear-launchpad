# Kestryx Benchmark Suite & Methodology (`bench/README.md`)

This directory contains the reproducible performance and resource budget harnesses for **Kestryx**.

## Running the Benchmarks

```bash
# 1. Run the M0 Stack Spike (Svelte 5 Fine-Grained vs VDOM baseline, 40 iterations)
node bench/stack-spike/run-spike.mjs

# 2. Run the Full End-to-End & JDK 26.0.1 Benchmark Suite (30+ iterations per metric)
node bench/run-all.mjs

# 3. Check Frontend Bundle Budgets (Initial JS <= 150 KB gzip)
node scripts/check-bundle-budget.mjs
```

## Methodology & Merge Gates

All latency and throughput metrics are collected over **30+ runs** on the reference machine (`AMD Ryzen 7 7435HS`, `8C/16T`, `16 GB RAM`, `NVMe SSD`, `Windows 11 x64`, `JDK 26.0.1+8-34`) and report `p50` (median), `p95`, `mean`, `min`, and `max`:

1. **Cold Start (`<= 400 ms` p50, `<= 700 ms` p95)**: Reads `state_snapshot.json`, loads the initial SPA assets (`30.85 KB` gzip), and hydrates the interactive instance list without waiting on the network.
2. **Play Click to JVM Process Spawn (`<= 300 ms` p50)**: Executes the warm-cache fast verification path (`size + mtime`), checks `.natives/.hash`, builds JVM/game arguments, and spawns a real `java` process (`MockMinecraftMain`).
3. **Download Throughput (`>= 90%` of `curl.exe`)**: Compares `curl.exe` against Kestryx's streaming downloader with inline SHA-1/BLAKE3 verification against a local HTTP loopback server.
4. **Disk Deduplication (`< 5 MB` per extra vanilla instance)**: Creates 10 same-version vanilla `26.3` instances hardlinked from the BLAKE3 CAS and measures marginal unshared disk consumption (`~125 bytes` per instance).
5. **Fast Start Evaluation (AppCDS & Java 25/26 JEP 483/514 AOT Cache)**: Measures real `java` startup time comparing `-Xshare:off` vs `-XX:+AutoCreateSharedArchive` (AppCDS) and `-XX:AOTCache` (`JEP 483/514`).
