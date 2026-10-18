//! Local account management and offline UUID generation for Light Yield Launcher.
//! Implements Java-exact RFC 4122 v3 UUIDs (`OfflinePlayer:<username>`).

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAccount {
    pub username: String,
    pub uuid: Uuid,
    pub access_token: String,
    pub user_type: String,
}

impl LocalAccount {
    /// Creates a local offline account with a valid RFC 4122 v3 UUID.
    /// Matches `java.util.UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(StandardCharsets.UTF_8))`.
    pub fn new(username: &str) -> Self {
        let clean_name = sanitize_username(username);
        let uuid = offline_uuid_v3(&clean_name);
        Self {
            username: clean_name,
            uuid,
            access_token: "0".to_string(),
            user_type: "mojang".to_string(),
        }
    }

    /// Formatted dashed UUID string (e.g. `b50ad385-829d-3141-a216-7e7d7539ba7f`).
    pub fn uuid_dashed(&self) -> String {
        self.uuid.hyphenated().to_string()
    }

    /// Undashed hex representation (e.g. `b50ad385829d3141a2167e7d7539ba7f`).
    pub fn uuid_undashed(&self) -> String {
        self.uuid.simple().to_string()
    }
}

/// Generates the exact RFC 4122 v3 UUID for an offline player.
pub fn offline_uuid_v3(username: &str) -> Uuid {
    let payload = format!("OfflinePlayer:{username}");
    let mut hasher = Md5::new();
    hasher.update(payload.as_bytes());
    let mut hash = hasher.finalize();

    // RFC 4122 version 3: clear bits 4-7, set to 0011
    hash[6] = (hash[6] & 0x0f) | 0x30;
    // RFC 4122 variant: clear bits 6-7, set to 10
    hash[8] = (hash[8] & 0x3f) | 0x80;

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash);
    Uuid::from_bytes(bytes)
}

/// Sanitizes a username to ensure safe Minecraft arguments (1-16 chars, alphanumeric/underscore).
pub fn sanitize_username(raw: &str) -> String {
    let trimmed = raw.trim();
    let filtered: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(16)
        .collect();

    if filtered.is_empty() {
        "LightYieldPlayer".to_string()
    } else {
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_offline_uuid_vectors() {
        // Known Java vectors generated from UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))
        let notch_uuid = offline_uuid_v3("Notch");
        assert_eq!(notch_uuid.to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(notch_uuid.get_version_num(), 3);

        let jeb_uuid = offline_uuid_v3("jeb_");
        assert_eq!(jeb_uuid.to_string(), "a762f560-4fce-3236-812a-b80efff0b62b");
        assert_eq!(jeb_uuid.get_version_num(), 3);
    }

    #[test]
    fn test_sanitize_username() {
        assert_eq!(sanitize_username("Steve"), "Steve");
        assert_eq!(sanitize_username("  Alex_99  "), "Alex_99");
        assert_eq!(sanitize_username("ThisUsernameIsWayTooLongForMinecraft"), "ThisUsernameIsWa");
        assert_eq!(sanitize_username("   "), "LightYieldPlayer");
        assert_eq!(sanitize_username("Invalid!@#Chars"), "InvalidChars");
    }
}
