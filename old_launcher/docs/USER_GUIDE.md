# Kestryx User Guide (`docs/USER_GUIDE.md`)

## 1. First-Timer Quick Start (In-Game in Under 60 Seconds & 3 Clicks)

1. **Launch Kestryx**: On first launch, Kestryx automatically detects any existing `.minecraft`, Prism Launcher, or Modrinth App installations for one-click import.
2. **Choose Account (Click 1)**: Type an offline username (or sign in with Microsoft).
3. **Click "Play Latest (26.3)" (Click 2 -> In-Game!)**: Kestryx automatically selects or provisions the required 64-bit Java runtime (`Java 25` for `26.3`), verifies assets in parallel over HTTP/2, and launches the game.

---

## 2. Power-User Guide: Headless CLI (`kestryx`), Hooks & Lockfiles

Every feature in the Kestryx UI is available from the headless `kestryx` CLI with `--json` output for scripting:

```bash
# Create an isolated 26.3 Fabric instance
kestryx instance create my-fabric-26 --name "26.3 Fabric" --mc-version 26.3 --loader fabric --loader-version 0.20.0

# Clone an instance instantaneously via BLAKE3 hardlinks/reflinks
kestryx instance clone my-fabric-26 my-fabric-test --name "26.3 Experiment"

# Calculate the exact vanilla OfflinePlayer v3 UUID
kestryx account uuid Notch

# Discover all 64-bit Java runtimes on the system in < 10 ms
kestryx java list

# Get a hardware-aware RAM allocation recommendation
kestryx java recommend-ram --mc-version 26.3 --loader fabric --mods 120

# Dry-run the full launch command (with Quick Play into a singleplayer world)
kestryx launch my-fabric-26 --username KestryxPilot --world "Survival Base" --dry-run

# Analyze a crash log and output one-click remediation fixes
kestryx doctor analyze-log ./latest.log --current-ram-mb 4096
```

### Deep Links (`kestryx://`)
Trigger direct launches from browser links or desktop shortcuts:
- `kestryx://launch?instance=sodium-26-3&world=New%20World`
- `kestryx://launch?instance=sodium-26-3&server=mc.hypixel.net:25565`

---

## 3. Offline Accounts, UUID v3 & Offline Skin Bridge

- **Consistent Vanilla Offline UUIDs**: Kestryx computes offline UUIDs using the exact Java algorithm `UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))` (MD5 hash with RFC 4122 version 3 and IETF variant bits). Because offline UUIDs are version 3 and Microsoft online UUIDs are version 4, they can never collide.
- **Offline Skin Visibility**: When you enable the **Offline Skin Bridge** on an offline account, Kestryx generates a managed resource pack (`kestryx-offline-skin.zip`) and local skin metadata bridge so your custom skin is visible to you in Singleplayer and on LAN/compatible servers. On public servers without a server-side skin plugin, other players will see a default skin.
