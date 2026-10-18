# ADR 0003: Vanilla-Exact Offline UUID v3 & Opt-In Offline Skin Bridge

- **Status**: Accepted
- **Date**: 2026-10-16

## Context
Offline accounts must generate the exact same UUID as vanilla Minecraft's offline mode on every OS and locale, never collide with online licensed accounts, and provide an honest, opt-in way to preview and see custom skins in-game.

## Decision
1. Implement `compute_offline_uuid(username)` as the MD5 digest of `"OfflinePlayer:" + username` (UTF-8, case-sensitive) with RFC 4122 version 3 (`byte[6] = (byte[6] & 0x0f) | 0x30`) and IETF variant (`byte[8] = (byte[8] & 0x3f) | 0x80`) bits, verified in CI against a live JDK executing `UUID.nameUUIDFromBytes`.
2. Provide `generate_offline_skin_resourcepack` (`kestryx-skins`) which converts legacy `64x32` skins to `64x64` and generates an instance resource pack (`kestryx-offline-skin.zip`) overriding default entity textures, paired with explicit UI disclosure explaining what other players see on servers.
