# Security Policy

## Supported versions

Only the latest release receives security fixes.

| Version | Supported |
| --- | --- |
| Latest release | ✅ |
| Older releases | ❌ |

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report vulnerabilities privately through
[**GitHub Security Advisories**](https://github.com/firearz/lightyear-launchpad/security/advisories/new).

Please include:

- A description of the issue and its impact
- Steps to reproduce, or a proof of concept
- The affected version / commit
- Any suggested fix, if you have one

### What to expect

- **Acknowledgement** within **72 hours**
- **Initial assessment** within **7 days**
- A fix and coordinated disclosure as soon as reasonably possible. You'll be credited unless you'd rather stay anonymous.

## Scope

In scope:

- Path traversal or arbitrary file writes during download / extraction
- Unsafe handling of downloaded metadata (version JSON, loader profiles)
- Command injection through JVM arguments, instance names or file paths
- Tampering with release binaries

Out of scope:

- Vulnerabilities in Minecraft itself, Java, or third-party mods
- Issues that require an attacker to already have local admin access

## Verifying downloads

Every release includes a `SHA256SUMS.txt`. To verify a download:

```powershell
Get-FileHash .\Lightyear-v1.0.0-windows-x64.exe -Algorithm SHA256
```

Compare the result with the value in `SHA256SUMS.txt`.
