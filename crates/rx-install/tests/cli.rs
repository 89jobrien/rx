use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn rx_cmd() -> Command {
    Command::cargo_bin("rx").unwrap()
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
fn install_local_file_appears_in_registry_and_list() {
    let tmp = tempdir().unwrap();
    let xdg = tmp.path().join("config");
    let bin_dir = tmp.path().join("bin");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).unwrap();

    let script = make_script(
        &scripts,
        "greet.sh",
        "#!/usr/bin/env bash\necho \"hello $1\"\n",
    );

    rx_cmd()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("install")
        .arg(script.to_str().unwrap())
        .arg("--install-dir")
        .arg(bin_dir.to_str().unwrap())
        .assert()
        .success();

    assert!(bin_dir.join("greet").exists());

    let registry_path = xdg.join("rx").join("registry.json");
    let output = rx_cmd()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("list")
        .arg("--registry-path")
        .arg(registry_path.to_str().unwrap())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("greet"), "list output: {stdout}");
}

#[test]
fn install_directory_installs_compatible_skips_incompatible() {
    let tmp = tempdir().unwrap();
    let xdg = tmp.path().join("config");
    let bin_dir = tmp.path().join("bin");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).unwrap();

    make_script(&scripts, "good.sh", "#!/usr/bin/env bash\necho good\n");
    make_script(&scripts, "bad.txt", "just some text\n");

    let output = rx_cmd()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("install")
        .arg(scripts.to_str().unwrap())
        .arg("--install-dir")
        .arg(bin_dir.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("installed good"), "stdout: {stdout}");
    assert!(stderr.contains("skipped"), "stderr: {stderr}");
    assert!(bin_dir.join("good").exists());
}

#[test]
fn run_executes_installed_script() {
    let tmp = tempdir().unwrap();
    let xdg = tmp.path().join("config");
    let bin_dir = tmp.path().join("bin");
    let scripts = tmp.path().join("scripts");
    fs::create_dir_all(&scripts).unwrap();

    let script = make_script(
        &scripts,
        "say-hi.sh",
        "#!/usr/bin/env bash\necho 'hi from rx'\n",
    );

    rx_cmd()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("install")
        .arg(script.to_str().unwrap())
        .arg("--install-dir")
        .arg(bin_dir.to_str().unwrap())
        .assert()
        .success();

    let registry_path = xdg.join("rx").join("registry.json");
    let output = rx_cmd()
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("run")
        .arg("say-hi")
        .arg("--registry-path")
        .arg(registry_path.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("hi from rx"), "stdout: {stdout}");
}
