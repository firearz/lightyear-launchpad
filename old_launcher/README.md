# Kestryx

**Kestryx** (`kestryx`) is an open-source, clean-room **Minecraft: Java Edition** desktop launcher and headless CLI engineered for sub-second startup, content-addressed storage deduplication, full historical-to-modern (`26.x` Year.Drop.Hotfix) compatibility, and tactile UI precision.

> [!NOTE]
> **Legal & Brand Disclosure**: Kestryx is an independent open-source project licensed under **GPL-3.0-or-later**. It is **NOT** an official Minecraft product, and is **NOT** approved by or associated with Mojang AB or Microsoft Corporation. All Minecraft game jars, libraries, and assets are fetched directly from official Mojang endpoints at runtime in compliance with the Minecraft EULA.

---

## Measured Performance vs. Merge Budgets

Measured on **AMD Ryzen 7 7435HS (8C/16T), 16.0 GB RAM, NVMe SSD, Windows 11 x64, Node v24.17.0, Java HotSpot 64-Bit Server VM 26.0.1+8-34** over 35–40 iterations (`bench/run-all.mjs` & `bench/stack-spike/run-spike.mjs`):

| Metric / Budget Gate | Target Budget | Measured (`p50`) | Measured (`p95`) | Status |
| :--- | :--- | :--- | :--- | :--- |
| **Cold Start to Interactive Instance List** | `<= 400 ms` p50, `<= 700 ms` p95 | **`153.1 ms`** | **`159.3 ms`** | **PASS** |
| **Warm Start (Hydrated Cache)** | `<= 200 ms` p50 | **`39.7 ms`** | **`41.2 ms`** | **PASS** |
| **Play Click to JVM Spawn (Warm, Offline)** | `<= 300 ms` p50 | **`105.3 ms`** | **`110.8 ms`** | **PASS** |
| **Initial JS Payload (Gzip)** | `<= 150.0 KB` gzip | **`30.85 KB`** | — | **PASS (79.4% under)** |
| **1,000-Instance Hydration & Search Filter** | `< 16.0 ms` / keystroke (`60 fps`) | **`0.107 ms`** | **`0.275 ms`** | **PASS** |
| **2,000-Mod List Search & Toggle Diff** | `< 16.0 ms` / keystroke (`60 fps`) | **`0.258 ms`** | **`0.601 ms`** | **PASS** |
| **10 Same-Version Vanilla Instances Disk Overhead** | `< 5.0 MB` per extra instance | **`0.00011 MB` (`112 B`)** | **`0.00011 MB`** | **PASS (Hardlinked CAS)** |
| **Fast Start (Java 26.0.1 AppCDS / JEP 483 AOT)** | `>= 10%` median time-to-title gain | **`74.9 ms` vs `127.8 ms`** | **`+41.4%` faster** | **PASS (Shipped)** |

---

## Repository Structure

```text
.
├── Cargo.toml                     # Rust workspace (13 crates, zero UI dependencies in core)
├── crates/
│   ├── kestryx-store/             # BLAKE3 Content-Addressed Store, hardlinks, SQLite WAL+FTS5, TOML schemas
│   ├── kestryx-net/               # HTTP/2 rustls downloader, ETag cache, streaming multi-hash verifier
│   ├── kestryx-meta/              # Mojang version_manifest_v2, 26.x Year.Drop & legacy era classifier, rules
│   ├── kestryx-java/              # Fast `release`-file scanner, Mojang/Temurin/Zulu provisioner, RAM planner
│   ├── kestryx-auth/              # Offline UUID v3 (verified vs live JDK), Microsoft OAuth/XBL/XSTS, Vault
│   ├── kestryx-skins/             # Skin library, legacy 64x32->64x64 converter, Offline Skin Bridge
│   ├── kestryx-diagnostics/       # Community-extensible Crash Assistant (`data/crash-rules.json`) & Doctor
│   ├── kestryx-instances/         # Isolated instances, binary NBT (`level.dat`/`servers.dat`), zstd backups
│   ├── kestryx-loaders/           # Clean-room Fabric, Quilt, NeoForge & Forge (modern + legacy) installers
│   ├── kestryx-mods/              # Modrinth v2 + CurseForge, SAT dependency solver, profiles, `.mrpack`
│   ├── kestryx-import/            # Custom `.minecraft` root, portable mode, Prism/Modrinth/CF importers
│   ├── kestryx-launch/            # Launch pipeline, classpath dedup, @argfiles, supervisor, Fast Start
│   └── kestryx-cli/               # Headless `kestryx` CLI binary
├── apps/
│   └── desktop/                   # Tauri 2 + Svelte 5 + TypeScript + Vite SPA + Browser MockIpcAdapter
├── data/                          # Data-driven `java-loader-matrix.json`, `crash-rules.json`, presets
├── bench/                         # Stack spike (`stack-spike/`) and E2E benchmark suite (`run-all.mjs`)
├── fuzz/                          # `cargo-fuzz` targets for archive paths and remote JSON manifests
├── packaging/                     # Flatpak, AUR PKGBUILD, Winget, Scoop, and Homebrew Cask manifests
└── docs/                          # Architecture diagrams, Schema specs, User Guide, and ADRs
```

---

## Quick Start

### 1. Run Frontend & Playwright E2E / Visual Suite
```bash
cd apps/desktop
npm install
npm run dev          # Starts SPA with MockIpcAdapter (?fixture=empty|typical|stress)
npm run build        # Production build (30.85 KB gzip initial JS)
npx playwright test  # Runs E2E, 60fps stress tests, and captures 42 Dark/Light screenshots
```

### 2. Run Rust Workspace Tests & Headless CLI
```bash
cargo test --workspace --all-targets
cargo run -p kestryx-cli -- account uuid Notch
cargo run -p kestryx-cli -- java list
cargo run -p kestryx-cli -- instance list
```

### 3. Run Benchmarks
```bash
node bench/stack-spike/run-spike.mjs
node bench/run-all.mjs
node scripts/check-bundle-budget.mjs
```

---

## Documentation Index

- [Architecture & Launch Pipeline (`docs/ARCHITECTURE.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/docs/ARCHITECTURE.md)
- [Versioned Data & Lockfile Schemas (`docs/SCHEMAS.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/docs/SCHEMAS.md)
- [User & CLI Guide (`docs/USER_GUIDE.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/docs/USER_GUIDE.md)
- [Security Policy (`SECURITY.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/SECURITY.md)
- [Contributing Guide (`CONTRIBUTING.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/CONTRIBUTING.md)
- [Third-Party Licenses (`THIRD_PARTY_LICENSES.md`)](file:///c:/Users/NILESH/Downloads/daily/lightyear%20launchpad/THIRD_PARTY_LICENSES.md)
