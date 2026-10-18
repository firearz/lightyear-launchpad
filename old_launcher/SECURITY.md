# Security Policy (`SECURITY.md`)

## Supported Versions

| Version | Supported |
| :--- | :--- |
| `0.1.x` | Yes |

## Security Architecture & Threat Model

Kestryx treats all remote metadata, mod archives, server ping responses, and crash logs as untrusted input:

1. **Transport & Host Allow-List (`kestryx-net`)**:
   - All network requests enforce `https://` with `rustls` TLS 1.2/1.3.
   - All upstream URLs are checked against `DEFAULT_ALLOWED_HOSTS` (Mojang, Microsoft/Xbox Live, Modrinth, CurseForge, Fabric, Quilt, NeoForge, Forge, Adoptium, Azul, GitHub).
   - Every downloaded artifact (client jar, library, native, asset object, Java runtime, mod jar) is hash-verified (`SHA-1`, `SHA-256`, `SHA-512`, and `BLAKE3`) in a single streaming pass before entering the Content-Addressed Store (`store/blake3/`).

2. **Archive & Path Traversal Hardening (`kestryx-import`)**:
   - Every `.zip`, `.mrpack`, `.jar`, and backup archive entry is validated by `validate_safe_archive_entry_path`.
   - Entries containing `..` (Zip-Slip), leading `/`, Windows drive prefixes (`C:`), null bytes, Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`), or symlinks escaping the extraction root are rejected before touching disk.
   - Fuzz targets in `fuzz/` continuously test archive path validation and JSON/NBT parsers.

3. **Credential Protection & Log Redaction (`kestryx-auth`, `kestryx-diagnostics`)**:
   - Microsoft OAuth / Xbox Live / XSTS / Minecraft access and refresh tokens are stored in the OS Keychain or a ChaCha20-Poly1305 AEAD encrypted vault (`EncryptedTokenVault`).
   - `SecretRedactor` automatically strips `--accessToken <token>` values and registered account secrets from console logs, launch command previews, and "Copy Diagnostics" bundles.

4. **Webview Isolation (`apps/desktop`)**:
   - Strict Content Security Policy (`default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'`).
   - Least-privilege Tauri 2 capabilities: the frontend webview has zero direct filesystem, shell, or arbitrary network access; all operations go through typed Rust IPC handlers.
   - Remote Markdown/HTML in Modrinth and CurseForge descriptions is sanitized before rendering.

5. **Zero Telemetry by Default**:
   - Kestryx sends zero analytics or telemetry by default (`telemetry.enabled = false`).

## Reporting a Vulnerability

Please report suspected security vulnerabilities privately via GitHub Security Advisories (`https://github.com/kestryx/kestryx/security/advisories/new`) or email `security@kestryx.dev`. We acknowledge reports within 24 hours and aim to ship verified fixes within 72 hours.
