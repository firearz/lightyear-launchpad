//! Cross-checks `kestryx_auth::compute_offline_uuid` against a real Java Virtual Machine (`java`)
//! executing `java.util.UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8))`.

use std::fs;
use std::process::Command;

use kestryx_auth::compute_offline_uuid;

#[test]
fn test_offline_uuid_matches_real_jdk_name_uuid_from_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let java_file = dir.path().join("UuidOracle.java");

    let java_source = r#"
import java.nio.charset.StandardCharsets;
import java.util.UUID;

public class UuidOracle {
    public static void main(String[] args) {
        for (String name : args) {
            byte[] bytes = ("OfflinePlayer:" + name).getBytes(StandardCharsets.UTF_8);
            UUID uuid = UUID.nameUUIDFromBytes(bytes);
            System.out.println(name + "=" + uuid.toString());
        }
    }
}
"#;
    fs::write(&java_file, java_source).expect("write UuidOracle.java");

    let test_names = [
        "Notch",
        "notch",
        "jeb_",
        "Dinnerbone",
        "Kestryx",
        "Player_01",
        "1234567890123456",
        "abc",
        "A_B_C_1_2_3",
        "CaseSensitiveCheck",
        "casesensitivecheck",
    ];

    let output = Command::new("java")
        .arg(&java_file)
        .args(test_names)
        .output()
        .expect("Failed to execute local JDK `java` for UUID cross-check");

    assert!(
        output.status.success(),
        "Java oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut verified_count = 0;
    for line in stdout.lines() {
        if let Some((name, jdk_uuid)) = line.trim().split_once('=') {
            let rust_uuid = compute_offline_uuid(name).hyphenated().to_string();
            assert_eq!(
                rust_uuid, jdk_uuid,
                "Offline UUID mismatch for username '{name}': Rust={rust_uuid}, JDK={jdk_uuid}"
            );
            verified_count += 1;
        }
    }

    assert_eq!(verified_count, test_names.len());
}
