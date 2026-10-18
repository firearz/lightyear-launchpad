# ADR 0002: BLAKE3 Content-Addressed Store & Hardlink/Reflink Deduplication

- **Status**: Accepted
- **Date**: 2026-10-16

## Context
Creating multiple modded or vanilla Minecraft instances traditionally duplicates hundreds of megabytes of libraries, assets, and mod jars. Our budget requires 10 same-version vanilla instances to add `< 5 MB` each beyond saves and options.

## Decision
1. Store all immutable files once inside `<DATA_ROOT>/store/blake3/<2-char-shard>/<blake3_hex>`.
2. During download in `kestryx-net`, compute `BLAKE3`, `SHA-1`, `SHA-256`, and `SHA-512` simultaneously in a single streaming pass so upstream checksums (`SHA-1` for Mojang/CurseForge, `SHA-512` for Modrinth) are verified before atomic persistence.
3. Materialize files into instances via filesystem hardlinks (`fs::hard_link`) or reflinks, falling back to atomic copy only when crossing filesystem volumes.

## Consequences
- 10 same-version vanilla instances add `< 4 KB` of unshared configuration (`instance.toml`) per instance—well below the `< 5 MB` gate.
