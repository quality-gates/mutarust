use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn installed_command_prints_help() {
    let root = smoke_root();
    let install = install_command(&root);

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
        format!("mutarust {}", env!("CARGO_PKG_VERSION")),
        "version output must identify the package"
    );

    fs::remove_dir_all(root).expect("smoke test files must be removed");
}

#[test]
fn installed_command_lists_production_sources() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_fixture(&root);

    let source_file = fixture.join("src").join("math.rs");
    let from_file = list_files(&install, &fixture, &[source_file.as_os_str()]);
    assert_eq!(from_file, source_file.display().to_string());

    let from_directory = list_files(&install, &fixture, &[fixture.as_os_str()]);
    assert_eq!(
        from_directory,
        format!(
            "{}\n{}",
            fixture.join("src").join("lib.rs").display(),
            source_file.display()
        )
    );

    let from_current_directory = list_files(&install, &fixture, &[]);
    assert_eq!(from_current_directory, from_directory);

    let excluded_test = fixture.join("tests").join("integration.rs");
    assert_no_sources(&install, &fixture, &excluded_test);

    let from_package = list_files(&install, &fixture, &["sample".as_ref()]);
    assert_eq!(from_package, from_directory);

    fs::remove_dir_all(root).expect("smoke test files must be removed");
}

fn install_command(root: &Path) -> PathBuf {
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
        .env("CARGO_TARGET_DIR", target)
        .status()
        .expect("cargo install must start");

    assert!(install_status.success(), "cargo install must succeed");
    install
}

fn write_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("fixture");
    fs::create_dir_all(fixture.join("src")).expect("fixture source directory must be created");
    fs::create_dir_all(fixture.join("tests")).expect("fixture test directory must be created");
    fs::create_dir_all(fixture.join("examples"))
        .expect("fixture example directory must be created");
    fs::create_dir_all(fixture.join("fixtures")).expect("fixture data directory must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    fs::write(fixture.join("src").join("lib.rs"), "mod math;\n")
        .expect("fixture library must be written");
    fs::write(fixture.join("src").join("math.rs"), "pub fn add() {}\n")
        .expect("fixture source must be written");
    fs::write(
        fixture.join("src").join("math_test.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture unit test must be written");
    fs::write(
        fixture.join("tests").join("integration.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture integration test must be written");
    fs::write(fixture.join("examples").join("demo.rs"), "fn main() {}\n")
        .expect("fixture example must be written");
    fs::write(fixture.join("fixtures").join("input.rs"), "fn input() {}\n")
        .expect("fixture data must be written");
    fs::canonicalize(fixture).expect("fixture path must resolve")
}

fn list_files(install: &Path, fixture: &Path, targets: &[&std::ffi::OsStr]) -> String {
    let output = Command::new(command_path(install))
        .arg("--list-files")
        .args(targets)
        .current_dir(fixture)
        .output()
        .expect("installed mutarust must list files");

    assert!(output.status.success(), "--list-files must succeed");
    String::from_utf8(output.stdout)
        .expect("file list must be UTF-8")
        .trim()
        .to_owned()
}

fn assert_no_sources(install: &Path, fixture: &Path, target: &Path) {
    let output = Command::new(command_path(install))
        .arg("--list-files")
        .arg(target)
        .current_dir(fixture)
        .output()
        .expect("installed mutarust must start");

    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("could not find any suitable Rust source files"),
        "test sources must be excluded"
    );
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

    package_target
        .join("package")
        .join(format!("mutarust-{}", env!("CARGO_PKG_VERSION")))
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
