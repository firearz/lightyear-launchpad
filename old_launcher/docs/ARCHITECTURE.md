# Kestryx Architecture (`docs/ARCHITECTURE.md`)

## 1. High-Level Crate & Module Topology

**Kestryx** is structured as a strict clean-room monorepo where all file, network, authentication, loader, and process logic resides in a zero-UI-dependency Rust workspace (`crates/*`), shared identically between the headless CLI (`kestryx-cli`) and the desktop application (`apps/desktop`).

```mermaid
flowchart LR
    subgraph Entrypoints["Entrypoints"]
        CLI["kestryx-cli\n(Headless CLI)"]
        App["apps/desktop\n(Tauri 2 + Svelte 5 SPA)"]
    end

    subgraph Domain["Domain Crates"]
        Launch["kestryx-launch\n(Pipeline, @argfiles, Supervisor, Fast Start)"]
        Instances["kestryx-instances\n(Isolation, NBT, Backups, Quick Play)"]
        Loaders["kestryx-loaders\n(Fabric, Quilt, NeoForge, Forge)"]
        Mods["kestryx-mods\n(Modrinth, CurseForge, Solver, .mrpack)"]
        Java["kestryx-java\n(Scanner, Provisioner, RAM Planner)"]
        Auth["kestryx-auth\n(Offline UUID v3, MSA/XBL/XSTS, Vault)"]
        Skins["kestryx-skins\n(64x32 Converter, Offline Skin Bridge)"]
        Import["kestryx-import\n(Custom Root, Multi-Launcher Importers)"]
        Diag["kestryx-diagnostics\n(Crash Assistant, Doctor, Redactor)"]
    end

    subgraph Foundation["Foundation Crates"]
        Meta["kestryx-meta\n(Mojang Manifest v2, 26.x Era Classifier, Rules)"]
        Net["kestryx-net\n(HTTP/2 rustls, ETag Cache, Streaming Hash)"]
        Store["kestryx-store\n(BLAKE3 CAS, Hardlinks, SQLite WAL + FTS5, TOML)"]
    end

    CLI & App --> Launch & Instances & Loaders & Mods & Java & Auth & Skins & Import & Diag
    Launch & Instances & Loaders & Mods & Java & Skins & Import --> Meta & Net & Store
```

## 2. Launch Pipeline State Machine

```mermaid
stateDiagram-v2
    [*] --> ResolveProfile: Read Version JSON + Merge Loader `inheritsFrom`
    ResolveProfile --> EvaluateRules: Filter by OS (windows/osx/linux), Arch (x86_64/arm64), Features
    EvaluateRules --> ResolveLibraries: Map Maven coords, Native classifiers, ARM64 LWJGL replacements
    ResolveLibraries --> DeduplicateClasspath: Group by `group:artifact:classifier` (Loader Wins)
    DeduplicateClasspath --> VerifyAndStageAssets: Fast `size + mtime` check -> Stage Modern or `virtual`/`map_to_resources`
    VerifyAndStageAssets --> ExtractNatives: Skip extraction if `.natives/.hash == library_set_blake3`
    ExtractNatives -->WorldDowngradeCheck: Parse `saves/*/level.dat` DataVersion vs target `world_version`
    WorldDowngradeCheck --> BuildArguments: Substitute `${auth_*}`, `${classpath}`, Log4j fix, Fast Start CDS/AOT, `@argfile`
    BuildArguments --> SpawnAndSupervise: Run Pre-Hook -> Spawn JVM -> Track Startup Profiler & Ring Buffer
    SpawnAndSupervise --> PostExit: Run Post-Hook -> Rolling zstd Backup -> Crash Assistant (if exit != 0)
    PostExit --> [*]
```

## 3. Fast Startup & Tray Memory Architecture

1. **Cold Start (`<= 400 ms` p50)**:
   - On launch, the host reads `state_snapshot.json` synchronously and serves the initial instance list to the Svelte 5 SPA before any network request is made.
   - Background hydration (`SQLite WAL`, ETag refresh, disk mtime verification) runs asynchronously on Tokio workers and pushes incremental diffs.
2. **Hidden to Tray (`<= 40 MB` RAM)**:
   - When closed/minimized to the system tray (`close_behavior = "tray"`), Kestryx destroys the OS webview window handle while keeping the lightweight Rust background daemon alive to monitor running game processes or downloads.
