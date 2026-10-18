# Third-Party Licenses (`THIRD_PARTY_LICENSES.md`)

Kestryx is licensed under **GPL-3.0-or-later**. Below is the inventory of open-source dependencies used across the Rust workspace and Frontend SPA, verified via `deny.toml`.

## Rust Workspace Dependencies (`Cargo.toml`)

| Crate | Version | License | Purpose |
| :--- | :--- | :--- | :--- |
| `tokio` | `1.x` | `MIT` | Async runtime and process supervision |
| `reqwest` | `0.12.x` | `MIT OR Apache-2.0` | HTTP/2 + `rustls` client |
| `serde` / `serde_json` | `1.0.x` | `MIT OR Apache-2.0` | JSON serialization & lockfiles |
| `toml` | `0.8.x` | `MIT OR Apache-2.0` | Human-editable `settings.toml` & `instance.toml` |
| `rusqlite` | `0.32.x` | `MIT` | SQLite WAL cache & FTS5 search index |
| `blake3` | `1.x` | `CC0-1.0 OR Apache-2.0` | Content-Addressed Store (`store/blake3/`) hashing |
| `sha1` / `sha2` / `md-5` | `0.10.x` | `MIT OR Apache-2.0` | Upstream hash verification & Offline UUID v3 |
| `uuid` | `1.x` | `Apache-2.0 OR MIT` | RFC 4122 v3 & v4 UUID representation |
| `chacha20poly1305` | `0.10.x` | `Apache-2.0 OR MIT` | Encrypted credential vault fallback |
| `zip` | `9.x` | `MIT` | `.jar`, `.mrpack`, and resourcepack archive handling |
| `flate2` | `1.x` | `MIT OR Apache-2.0` | Gzip decompression for `level.dat` NBT |
| `zstd` | `0.13.x` | `MIT` | Rolling world backups |
| `image` | `0.25.x` | `MIT OR Apache-2.0` | Skin `64x32` -> `64x64` conversion & WebP icons |
| `clap` | `4.x` | `MIT OR Apache-2.0` | Headless CLI (`kestryx-cli`) argument parser |
| `proptest` | `1.x` | `MIT OR Apache-2.0` | Property-based testing (Offline UUID v3 & parsers) |

## Frontend Dependencies (`apps/desktop/package.json`)

| Package | License | Purpose |
| :--- | :--- | :--- |
| `svelte` (`^5.0.0`) | `MIT` | Fine-grained reactive UI framework |
| `vite` | `MIT` | Frontend bundler & code-splitting |
| `typescript` | `Apache-2.0` | Static type checking |
| `@playwright/test` | `Apache-2.0` | E2E, performance budget & screenshot regression testing |
| `@axe-core/playwright` | `MPL-2.0` | WCAG 2.2 AA automated accessibility auditing |
