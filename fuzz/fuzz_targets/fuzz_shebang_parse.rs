#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Convert arbitrary bytes to a string; skip if not valid UTF-8 since
    // detect_runtime takes &str. A separate target could test the boundary
    // between raw bytes and string conversion if needed.
    if let Ok(contents) = std::str::from_utf8(data) {
        // Must never panic regardless of input -- only Ok or Err.
        let _ = rx_script_core::detect_runtime(contents, "fuzz-input");
    }
});
