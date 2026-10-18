# ADR 0001: Tauri 2 + Svelte 5 + Typed IPC & Browser Mock Adapter

- **Status**: Accepted
- **Date**: 2026-10-16

## Context
We must meet strict desktop performance budgets (`<= 400 ms` cold start p50, `<= 150 MB` open window RAM, `<= 40 MB` tray RAM, `<= 150 KB` gzip initial JS, `60 fps` scroll on 1,000 instances and 2,000 mods) while also allowing the entire UI to run in a standard browser for automated Playwright E2E and visual regression testing.

## Decision
1. Use **Rust** for all core crates (`crates/kestryx-*`) with zero UI dependencies.
2. Use **Svelte 5** (Runes fine-grained reactivity) + **Vite** + single-file CSS Custom Properties (`tokens.css`) for the frontend SPA (`apps/desktop`), avoiding Virtual DOM diffing overhead and runtime CSS-in-JS.
3. Abstract all host calls behind `src/lib/ipc/adapter.ts`, which dispatches to Tauri 2 IPC in the native shell and automatically switches to deterministic fixtures (`empty`, `typical`, `stress`) in a plain browser.

## Consequences
- Initial JS bundle stays far below the `150 KB` gzip gate.
- Playwright can test and screenshot every route, theme, viewport size, and 1,000-instance stress scenario headlessly in CI.
