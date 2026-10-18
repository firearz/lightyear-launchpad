#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = kestryx_import::validate_safe_archive_entry_path(s);
    }
    if let Ok(dir) = tempfile::tempdir() {
        let _ = kestryx_import::extract_zip_safely(data, dir.path());
    }
});
