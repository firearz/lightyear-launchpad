//! `kestryx-auth`: Multi-account management (Offline & Microsoft OAuth 2.0 Device/PKCE ->
//! Xbox Live -> XSTS -> Minecraft Services), exact vanilla-compatible Offline UUID v3
//! (`UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))`), XSTS remediation
//! mapping, encrypted credential storage, and log/argument secret redaction.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use md5::{Digest as Md5Digest, Md5};
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Computes the exact vanilla Minecraft offline UUID (`UUID.nameUUIDFromBytes` in Java):
/// 1. Concatenate `"OfflinePlayer:"` + `username` (case-sensitive UTF-8 bytes)
/// 2. Compute 16-byte MD5 digest
/// 3. Set version 3 bits: `bytes[6] = (bytes[6] & 0x0f) | 0x30`
/// 4. Set IETF RFC 4122 variant bits: `bytes[8] = (bytes[8] & 0x3f) | 0x80`
pub fn compute_offline_uuid(username: &str) -> Uuid {
    let mut hasher = Md5::new();
    hasher.update(b"OfflinePlayer:");
    hasher.update(username.as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();

    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    Uuid::from_bytes(bytes)
}

/// Returns `(dashed_uuid, undashed_uuid)` strings for live UI preview and launch argument substitution.
pub fn format_offline_uuid_pair(username: &str) -> (String, String) {
    let uuid = compute_offline_uuid(username);
    (
        uuid.hyphenated().to_string(),
        uuid.simple().to_string(),
    )
}

/// Validates a standard Minecraft username (`3..=16` ASCII `[A-Za-z0-9_]`), unless `allow_advanced_override` is true.
pub fn validate_username(username: &str, allow_advanced_override: bool) -> Result<()> {
    if username.trim().is_empty() {
        bail!("Username cannot be empty");
    }
    if allow_advanced_override {
        if username.len() > 64 {
            bail!("Advanced override username cannot exceed 64 bytes");
        }
        return Ok(());
    }
    if !(3..=16).contains(&username.len()) {
        bail!("Username must be between 3 and 16 characters long (enable Advanced Override for non-standard names)");
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("Username may only contain letters, digits, and underscores [A-Za-z0-9_]");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountKind {
    Offline,
    Microsoft {
        xuid: String,
        msa_expires_at: i64,
        mc_expires_at: i64,
        owns_minecraft_java: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountProfile {
    pub id: String,
    pub username: String,
    pub uuid_dashed: String,
    pub uuid_undashed: String,
    pub kind: AccountKind,
    pub active_skin_url: Option<String>,
    pub active_cape_id: Option<String>,
    pub last_used_at: String,
}

impl AccountProfile {
    pub fn new_offline(username: &str, allow_advanced_override: bool) -> Result<Self> {
        validate_username(username, allow_advanced_override)?;
        let (dashed, undashed) = format_offline_uuid_pair(username);
        Ok(Self {
            id: format!("offline-{undashed}"),
            username: username.to_string(),
            uuid_dashed: dashed,
            uuid_undashed: undashed,
            kind: AccountKind::Offline,
            active_skin_url: None,
            active_cape_id: None,
            last_used_at: chrono::Utc::now().to_rfc3339(),
        })
    }
}

// ============================================================================
// Microsoft OAuth 2.0 / Xbox Live / XSTS Error Remediation Mapper
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XstsErrorRemediation {
    pub xerr_code: u64,
    pub title: String,
    pub cause: String,
    pub fix_action_label: String,
    pub fix_url: String,
}

pub fn map_xsts_error_code(xerr: u64) -> XstsErrorRemediation {
    match xerr {
        2148916233 => XstsErrorRemediation {
            xerr_code: xerr,
            title: "No Xbox Profile Found on Microsoft Account".to_string(),
            cause: "This Microsoft account has not yet created an Xbox Live gamertag profile required for Minecraft.".to_string(),
            fix_action_label: "Create Xbox Profile on xbox.com".to_string(),
            fix_url: "https://signup.live.com/signup?wa=wsignin1.0&rpsnv=13&rver=7.3.6963.0&wp=MBI_SSL&wreply=https%3a%2f%2fwww.xbox.com%2fen-US%2f".to_string(),
        },
        2148916235 => XstsErrorRemediation {
            xerr_code: xerr,
            title: "Xbox Live Unavailable in Region".to_string(),
            cause: "Xbox Live authentication services are not currently available in your account's registered region.".to_string(),
            fix_action_label: "Check Xbox Region Support".to_string(),
            fix_url: "https://www.xbox.com/regions".to_string(),
        },
        2148916236 | 2148916237 => XstsErrorRemediation {
            xerr_code: xerr,
            title: "Adult Verification Required".to_string(),
            cause: "Regional regulations require age/adult verification on xbox.com before signing in.".to_string(),
            fix_action_label: "Complete Verification on xbox.com".to_string(),
            fix_url: "https://account.xbox.com/settings".to_string(),
        },
        2148916238 => XstsErrorRemediation {
            xerr_code: xerr,
            title: "Child Account Requires Microsoft Family Approval".to_string(),
            cause: "Accounts under 18 must be added to a Microsoft Family group by an adult organizer with multiplayer enabled.".to_string(),
            fix_action_label: "Manage Microsoft Family Settings".to_string(),
            fix_url: "https://account.microsoft.com/family/".to_string(),
        },
        _ => XstsErrorRemediation {
            xerr_code: xerr,
            title: format!("Xbox Live Authentication Error ({xerr})"),
            cause: "Xbox Security Token Service (XSTS) rejected the authentication request.".to_string(),
            fix_action_label: "Review Minecraft Account Status".to_string(),
            fix_url: "https://www.minecraft.net/msaprofile".to_string(),
        },
    }
}

// ============================================================================
// Encrypted Credential Vault (ChaCha20-Poly1305) & Token Redactor
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedTokenVault {
    nonce_hex: String,
    ciphertext_hex: String,
}

impl EncryptedTokenVault {
    pub fn seal(key_bytes: &[u8; 32], tokens: &BTreeMap<String, String>) -> Result<Self> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key_bytes));
        let mut nonce_raw = [0u8; 12];
        for b in &mut nonce_raw {
            *b = rand::random::<u8>();
        }
        let nonce = Nonce::from_slice(&nonce_raw);
        let plaintext = serde_json::to_vec(tokens)?;
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {e}"))?;

        Ok(Self {
            nonce_hex: hex::encode(nonce_raw),
            ciphertext_hex: hex::encode(ciphertext),
        })
    }

    pub fn open(&self, key_bytes: &[u8; 32]) -> Result<BTreeMap<String, String>> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key_bytes));
        let nonce_vec = hex::decode(&self.nonce_hex)?;
        if nonce_vec.len() != 12 {
            bail!("Invalid vault nonce length");
        }
        let nonce = Nonce::from_slice(&nonce_vec);
        let ciphertext = hex::decode(&self.ciphertext_hex)?;
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| anyhow::anyhow!("Vault decryption failed or invalid key"))?;
        Ok(serde_json::from_slice(&plaintext)?)
    }
}

/// Redacts access tokens, refresh tokens, XUIDs, and OS usernames from logs,
/// launch argument displays, and diagnostic bundles.
pub struct SecretRedactor {
    exact_secrets: Vec<String>,
    token_flag_re: Regex,
}

impl SecretRedactor {
    pub fn new(secrets: Vec<String>) -> Self {
        let filtered = secrets
            .into_iter()
            .filter(|s| s.len() >= 4)
            .collect::<Vec<_>>();
        let token_flag_re = Regex::new(r"(--accessToken\s+)(\S+)").expect("valid regex");
        Self {
            exact_secrets: filtered,
            token_flag_re,
        }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut out = self
            .token_flag_re
            .replace_all(input, "${1}[REDACTED_TOKEN]")
            .to_string();
        for secret in &self.exact_secrets {
            out = out.replace(secret, "[REDACTED]");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_offline_uuid_matches_known_vanilla_vectors() {
        // Known Java `UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))`
        let notch_uuid = compute_offline_uuid("Notch");
        assert_eq!(notch_uuid.get_version_num(), 3);
        assert_eq!(
            notch_uuid.hyphenated().to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );

        // Case-sensitivity check: "notch" != "Notch"
        let lower_notch = compute_offline_uuid("notch");
        assert_ne!(notch_uuid, lower_notch);
    }

    proptest! {
        #[test]
        fn prop_offline_uuid_is_always_rfc4122_v3_and_deterministic(name in "\\PC{1,48}") {
            let u1 = compute_offline_uuid(&name);
            let u2 = compute_offline_uuid(&name);
            prop_assert_eq!(u1, u2);
            // Version MUST be 3 (name-based MD5), never 4 (online licensed UUID)
            prop_assert_eq!(u1.get_version_num(), 3);
            prop_assert_eq!(u1.get_variant(), uuid::Variant::RFC4122);
        }
    }

    #[test]
    fn test_encrypted_vault_and_redactor() -> Result<()> {
        let key = [42u8; 32];
        let mut map = BTreeMap::new();
        map.insert("msa_token".to_string(), "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secret123".to_string());
        let vault = EncryptedTokenVault::seal(&key, &map)?;
        let opened = vault.open(&key)?;
        assert_eq!(opened, map);

        let redactor = SecretRedactor::new(vec!["eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secret123".to_string()]);
        let log_line = "Launching with --accessToken eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secret123 --username Player";
        let redacted = redactor.redact(log_line);
        assert!(!redacted.contains("secret123"));
        assert!(redacted.contains("[REDACTED_TOKEN]"));
        Ok(())
    }
}
