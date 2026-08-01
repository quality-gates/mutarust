use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn installed_command_prints_help() {
    let root = smoke_root();
    let package_target = root.join("package-target");
    let target = root.join("target");
    let install = root.join("install");
    let package = package_crate(&package_target);

    let install_status = Command::new(env!("CARGO"))
        .args(["install", "--path"])
        .arg(package)
        .arg("--root")
        .arg(&install)
        .args(["--debug", "--locked", "--force"])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .expect("cargo install must start");

    assert!(install_status.success(), "cargo install must succeed");

    let output = Command::new(command_path(&install))
        .arg("--help")
        .output()
        .expect("installed mutarust must start");

    assert!(output.status.success(), "--help must succeed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Mutation testing for Rust"),
        "help must identify the command purpose"
    );

    let version = Command::new(command_path(&install))
        .arg("--version")
        .output()
        .expect("installed mutarust version command must start");

    assert!(version.status.success(), "--version must succeed");
    assert_eq!(
        String::from_utf8(version.stdout)
            .expect("version output must be UTF-8")
            .trim(),
        "mutarust 0.1.0",
        "version output must identify the package"
    );

    fs::remove_dir_all(root).expect("smoke test files must be removed");
}

fn package_crate(package_target: &Path) -> PathBuf {
    let package_status = Command::new(env!("CARGO"))
        .args([
            "package",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "--allow-dirty",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", package_target)
        .status()
        .expect("cargo package must start");

    assert!(package_status.success(), "cargo package must succeed");

    package_target.join("package").join("mutarust-0.1.0")
}

fn smoke_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after the Unix epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "mutarust-install-smoke-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("smoke test root must be created");
    root
}

fn command_path(install: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "mutarust.exe"
    } else {
        "mutarust"
    };
    install.join("bin").join(name)
}
