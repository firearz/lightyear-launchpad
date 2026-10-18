#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<kestryx_meta::VersionProfile>(s);
        let _ = serde_json::from_str::<kestryx_meta::VersionManifestV2>(s);
        let _ = kestryx_meta::VersionEra::classify(s, "release");
    }
});
