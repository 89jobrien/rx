use rx_core::conformance;
use rx_registry_json::{FsScriptReader, FsScriptWriter, JsonRegistryStore, WalkdirScanner};
use std::fs;

#[test]
fn conformance_json_registry_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry_path = dir.path().join("registry.json");
    let mut store = JsonRegistryStore::new(registry_path);
    conformance::assert_registry_store_contract(&mut store);
}

#[test]
fn conformance_fs_script_writer() {
    conformance::assert_script_writer_contract(&FsScriptWriter);
}

#[test]
fn conformance_walkdir_scanner() {
    conformance::assert_directory_scanner_contract(&WalkdirScanner);
}

#[test]
fn conformance_fs_script_reader() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("test.sh");
    fs::write(&file, "#!/usr/bin/env bash\necho hello\n").expect("write test file");
    conformance::assert_script_reader_contract(&FsScriptReader, &file);
}
