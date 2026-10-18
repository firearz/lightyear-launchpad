# Changelog

All notable changes to Lightyear are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-10-19

The first public open-source release. 🎉

### Added
- Frosted-glass PySide6 interface with Home, Installed and Help pages
- Isolated per-instance game directories with shared `versions/`, `libraries/` and `assets/`
- Vanilla, Fabric, Forge and Quilt support, including snapshots, beta and alpha versions
- Smart Download Decision Matrix that reuses game files already on disk
- 5-stage background installation pipeline with SHA-1 / ZIP integrity checks, retries and cancellation
- Instance context menu: rename, duplicate, edit settings, repair, add mods / resource packs / shaderpacks, delete
- Per-instance RAM, Java path and JVM arguments
- Automatic Java version detection, with Adoptium guidance when Java is missing
- Offline UUID v3 that matches Java's `OfflinePlayer:<name>` exactly
- Two-step confirmation before deleting instances or uninstalling game versions
- Built-in searchable Help & FAQ center
- 10 pixel-art instance icons plus custom image support
- Automated Windows builds and GitHub Releases through GitHub Actions

### Fixed
- The "Mods & Shaders" FAQ filter label lost its ampersand

[Unreleased]: https://github.com/firearz/lightyear-launchpad/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/firearz/lightyear-launchpad/releases/tag/v1.0.0
