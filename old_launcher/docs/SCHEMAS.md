# Kestryx Data & Lockfile Schemas (`docs/SCHEMAS.md`)

All configuration files in Kestryx carry an explicit `schema_version` field and preserve unknown user keys via `#[serde(flatten)]` so forward/backward migrations never strip custom settings.

## 1. Global Settings (`settings.toml` — Schema v1)
- Location: `<DATA_ROOT>/settings.toml`
- Format: TOML
- Preserves unknown keys: Yes (`GlobalSettings.unknown_fields`)

## 2. Instance Configuration (`instance.toml` — Schema v1)
- Location: `<DATA_ROOT>/instances/<id>/instance.toml`
- Format: TOML
- Preserves unknown keys: Yes (`InstanceConfig.unknown_fields`)
- Sections:
  - `[game]`: `mc_version` (opaque string, e.g., `"26.3"`, `"26.4-snapshot-4"`, `"1.21.4"`, `"a1.2.6"`), `loader` (`"vanilla"`, `"fabric"`, `"quilt"`, `"neoforge"`, `"forge"`), `loader_version`, `quick_play_target`.
  - `[java]`: `selection_mode`, `custom_path`, `memory_mode`, `min_memory_mb`, `max_memory_mb`, `jvm_preset`, `extra_jvm_args`, `fast_start_mode` (`"auto"`, `"appcds"`, `"aot"`, `"off"`).
  - `[process]`: `wrapper_command`, `pre_launch_hook`, `post_exit_hook`, `window_width`, `window_height`, `fullscreen`, `prefer_discrete_gpu`, `[process.env]`.
  - `[shared_folders]`: `saves`, `resourcepacks`, `shaderpacks`, `options_txt`, `servers_dat`.

## 3. Instance Lockfile (`instance.lock.json` — Schema v1)
- Location: `<DATA_ROOT>/instances/<id>/instance.lock.json`
- Format: Deterministic JSON
- Purpose: Pins every mod, resourcepack, shaderpack, and library to its exact `blake3`, `sha512`, and `sha1` digests, upstream provenance (`modrinth`, `curseforge`, `github`, `direct_url`, `local`), release channel, and dependency graph edges.

## 4. SQLite Cache & Search Index (`db/kestryx.sqlite3` — Schema v1)
- Pragmas: `journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`.
- Tables:
  - `schema_migrations`: Tracks applied SQLite schema versions.
  - `http_cache`: Stores `etag`, `last_modified`, `body_blake3`, `expires_at`, and `status_code` for conditional HTTP/2 `If-None-Match` requests.
  - `cas_entries`: Tracks `blake3`, `sha1`, `sha256`, `sha512`, `size`, `ref_count`, and `last_verified_mtime`.
  - `java_runtimes`: Caches discovered and managed JVMs (`path`, `mtime`, `valid_flags_cache_json`).
  - `instances_index` & `instances_fts` (FTS5 virtual table): Sub-millisecond full-text search across 1,000+ instances.
  - `mods_index` & `mods_fts` (FTS5 virtual table): Sub-millisecond full-text search across 2,000+ mods.
