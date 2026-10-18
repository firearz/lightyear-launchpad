# Contributing to Kestryx (`CONTRIBUTING.md`)

Thank you for contributing to **Kestryx**! Every contribution is held to strict correctness, security, and performance gates.

## Clean-Room & Legal Charter
- **Clean-Room Only**: Implement features strictly from public Mojang specifications, loader documentation, and live API responses. Never copy code or assets from proprietary or third-party launchers.
- **No Mojang Assets**: Never commit or redistribute Mojang jars, assets, or logos. All game files must be fetched from official endpoints at runtime.
- **Non-Goals**: Bedrock Edition, cheat clients, anti-cheat bypasses, paid features, or telemetry-by-default will not be accepted.

## Development Workflow

### 1. Rust Workspace (`crates/*`)
```bash
# Run all unit, property, golden launch matrix, and live JDK UUID cross-check tests
cargo test --workspace --all-targets

# Run CLI benchmarks
cargo run --release -p kestryx-cli -- bench play-to-spawn --runs 30
cargo run --release -p kestryx-cli -- bench cas-ten-instances
```

### 2. Frontend & Mock-IPC Shell (`apps/desktop`)
The entire UI runs in a standard browser against `MockIpcAdapter` (`?fixture=empty|typical|stress`) so you can develop and run Playwright visual/performance tests without building the native shell:
```bash
cd apps/desktop
npm install
npm run dev
npm run build
node ../../scripts/check-bundle-budget.mjs
npx playwright test
```

## Merge Gates
Every pull request must pass:
1. `cargo test --workspace` (including the `jdk_uuid_crosscheck` and golden launch command matrix).
2. `scripts/check-bundle-budget.mjs` (Initial JS `<= 150 KB` gzip).
3. Playwright E2E, `60 fps` scroll on `1,000` instances / `2,000` mods, `< 16 ms` search latency, and WCAG 2.2 AA accessibility checks.
4. `cargo deny check` supply-chain and license verification.
