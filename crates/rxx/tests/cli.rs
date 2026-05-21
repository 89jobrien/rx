use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn rxx_cmd() -> Command {
    Command::cargo_bin("rxx").unwrap()
}

fn make_script(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
fn direct_run_compatible_script_succeeds() {
    let tmp = tempdir().unwrap();
    let script = make_script(
        tmp.path(),
        "hello.sh",
        "#!/usr/bin/env bash\necho 'hello from rxx'\n",
    );

    let output = rxx_cmd().arg(script.to_str().unwrap()).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("hello from rxx"), "stdout: {stdout}");
}

#[test]
fn direct_run_incompatible_file_fails() {
    let tmp = tempdir().unwrap();
    let script = make_script(tmp.path(), "bad.txt", "no shebang here\n");

    rxx_cmd().arg(script.to_str().unwrap()).assert().failure();
}
