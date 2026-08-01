use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SMOKE_ROOT: AtomicU64 = AtomicU64::new(0);

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

    let source_directory = fixture.join("src");
    let expected_direct = format!(
        "{}\n{}",
        source_directory.join("lib.rs").display(),
        source_file.display()
    );
    let from_directory = list_files(&install, &fixture, &[source_directory.as_os_str()]);
    assert_eq!(from_directory, expected_direct);

    let recursive_directory = source_directory.join("...");
    let expected_recursive = format!(
        "{}\n{}\n{}",
        source_directory.join("lib.rs").display(),
        source_file.display(),
        source_directory.join("nested").join("inside.rs").display()
    );
    let from_recursive_directory =
        list_files(&install, &fixture, &[recursive_directory.as_os_str()]);
    assert_eq!(from_recursive_directory, expected_recursive);

    let from_current_directory = list_files(&install, &fixture, &[]);
    assert_eq!(from_current_directory, expected_recursive);

    let vendor_source = fixture.join("vendor").join("dependency.rs");
    let from_vendor = list_files(&install, &fixture, &[fixture.join("vendor").as_os_str()]);
    assert_eq!(from_vendor, vendor_source.display().to_string());
    let vendor_test = fixture
        .join("vendor")
        .join("crate")
        .join("tests")
        .join("case.rs");
    let recursive_vendor = list_files(
        &install,
        &fixture,
        &[fixture.join("vendor").join("...").as_os_str()],
    );
    assert_eq!(
        recursive_vendor,
        format!("{}\n{}", vendor_test.display(), vendor_source.display())
    );

    let excluded_test = fixture.join("tests").join("integration.rs");
    let explicit_test = list_files(&install, &fixture, &[excluded_test.as_os_str()]);
    assert_eq!(explicit_test, excluded_test.display().to_string());
    let explicit_test_directory =
        list_files(&install, &fixture, &[fixture.join("tests").as_os_str()]);
    let explicit_test_helper = fixture.join("tests").join("example_test.rs");
    assert_eq!(
        explicit_test_directory,
        format!(
            "{}\n{}",
            explicit_test_helper.display(),
            excluded_test.display()
        )
    );
    let explicit_test_file = fixture.join("src").join("math_test.rs");
    let from_explicit_test_file = list_files(&install, &fixture, &[explicit_test_file.as_os_str()]);
    assert_eq!(
        from_explicit_test_file,
        explicit_test_file.display().to_string()
    );
    let nested_test_directory = fixture.join("tests").join("support");
    let nested_test_file = nested_test_directory.join("nested_test.rs");
    let from_nested_test_directory =
        list_files(&install, &fixture, &[nested_test_directory.as_os_str()]);
    assert_eq!(
        from_nested_test_directory,
        nested_test_file.display().to_string()
    );

    let from_package = list_files(&install, &fixture, &["sample".as_ref()]);
    assert_eq!(from_package, expected_direct);

    let from_recursive_package = list_files(&install, &fixture, &["sample...".as_ref()]);
    assert_eq!(from_recursive_package, expected_recursive);

    let workspace = write_workspace_fixture(&root);
    let alpha = workspace.join("alpha");
    let beta = workspace.join("beta");
    let workspace_direct = format!(
        "{}\n{}\n{}\n{}\n{}",
        alpha.join("bin").join("cli_test.rs").display(),
        alpha.join("source").join("entry.rs").display(),
        alpha.join("source").join("helper.rs").display(),
        alpha.join("tests").join("selected.rs").display(),
        beta.join("src").join("lib.rs").display(),
    );
    let from_workspace = list_files(&install, &workspace, &[workspace.as_os_str()]);
    assert_eq!(from_workspace, workspace_direct);

    let workspace_recursive = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        alpha.join("bin").join("cli_test.rs").display(),
        alpha.join("source").join("entry.rs").display(),
        alpha.join("source").join("helper.rs").display(),
        alpha
            .join("source")
            .join("nested")
            .join("inside.rs")
            .display(),
        alpha.join("tests").join("selected.rs").display(),
        beta.join("src").join("lib.rs").display(),
    );
    let from_recursive_workspace =
        list_files(&install, &workspace, &[workspace.join("...").as_os_str()]);
    assert_eq!(from_recursive_workspace, workspace_recursive);

    let from_alpha_source = list_files(
        &install,
        &workspace,
        &[alpha.join("source").join("...").as_os_str()],
    );
    assert_eq!(
        from_alpha_source,
        workspace_recursive
            .lines()
            .skip(1)
            .take(3)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let from_alpha_directory = list_files(
        &install,
        &workspace,
        &[workspace.join("alpha").join("...").as_os_str()],
    );
    assert_eq!(
        from_alpha_directory,
        workspace_recursive
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let from_custom_package = list_files(&install, &workspace, &["alpha...".as_ref()]);
    assert_eq!(
        from_custom_package,
        workspace_recursive
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let plain = root.join("plain");
    fs::create_dir_all(plain.join("src")).expect("plain source directory must be created");
    let plain_source_path = plain.join("src").join("plain.rs");
    fs::write(&plain_source_path, "pub fn plain() {}\n").expect("plain source must be written");
    let plain_source = fs::canonicalize(plain_source_path).expect("plain source must resolve");
    let from_plain_directory = list_files(&install, &plain, &["src".as_ref()]);
    assert_eq!(from_plain_directory, plain_source.display().to_string());

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
    fs::create_dir_all(fixture.join("src").join("nested"))
        .expect("fixture nested source directory must be created");
    fs::create_dir_all(fixture.join("src").join("shared_tests"))
        .expect("fixture inline test module directory must be created");
    fs::create_dir_all(fixture.join("tests")).expect("fixture test directory must be created");
    fs::create_dir_all(fixture.join("tests").join("support"))
        .expect("fixture nested test directory must be created");
    fs::create_dir_all(fixture.join("examples"))
        .expect("fixture example directory must be created");
    fs::create_dir_all(fixture.join("fixtures")).expect("fixture data directory must be created");
    fs::create_dir_all(fixture.join("vendor")).expect("fixture vendor directory must be created");
    fs::create_dir_all(fixture.join("vendor").join("crate").join("tests"))
        .expect("fixture nested vendor test directory must be created");
    fs::create_dir_all(fixture.join("generated"))
        .expect("fixture generated directory must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("fixture manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "mod math;\n#[cfg(test)] mod tests { include!(\"test_support.rs\"); }\n#[cfg(test)] mod shared_tests { #[path = \"../math.rs\"] mod shared_math; }\n#[cfg(test)] mod external_tests;\n",
    )
    .expect("fixture library must be written");
    fs::write(fixture.join("src").join("math.rs"), "pub fn add() {}\n")
        .expect("fixture source must be written");
    fs::write(
        fixture.join("src").join(".hidden.rs"),
        "pub fn hidden() {}\n",
    )
    .expect("fixture hidden source must be written");
    fs::write(
        fixture.join("src").join("nested").join("inside.rs"),
        "pub fn inside() {}\n",
    )
    .expect("fixture nested source must be written");
    fs::write(
        fixture.join("src").join("math_test.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture unit test must be written");
    fs::write(
        fixture.join("src").join("test_support.rs"),
        "pub fn test_support() {}\n",
    )
    .expect("fixture test support must be written");
    fs::write(
        fixture.join("src").join("external_tests.rs"),
        "#[path = \"external_test_support.rs\"] mod support;\n",
    )
    .expect("fixture external test module must be written");
    fs::write(
        fixture.join("src").join("external_test_support.rs"),
        "pub fn external_test_support() {}\n",
    )
    .expect("fixture external test support must be written");
    fs::write(
        fixture.join("tests").join("example_test.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture test helper must be written");
    fs::write(
        fixture.join("tests").join("support").join("nested_test.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture nested test helper must be written");
    fs::write(
        fixture.join("tests").join("integration.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture integration test must be written");
    fs::write(fixture.join("examples").join("demo.rs"), "fn main() {}\n")
        .expect("fixture example must be written");
    fs::write(fixture.join("fixtures").join("input.rs"), "fn input() {}\n")
        .expect("fixture data must be written");
    fs::write(
        fixture.join("vendor").join("dependency.rs"),
        "fn dep() {}\n",
    )
    .expect("fixture dependency must be written");
    fs::write(
        fixture
            .join("vendor")
            .join("crate")
            .join("tests")
            .join("case.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("fixture vendor test must be written");
    fs::write(
        fixture.join("generated").join("output.rs"),
        "fn output() {}\n",
    )
    .expect("fixture generated source must be written");
    fs::canonicalize(fixture).expect("fixture path must resolve")
}

fn write_workspace_fixture(root: &Path) -> PathBuf {
    let workspace = root.join("workspace");
    let alpha = workspace.join("alpha");
    let beta = workspace.join("beta");
    fs::create_dir_all(alpha.join("bin")).expect("workspace binary directory must be created");
    fs::create_dir_all(alpha.join("source").join("nested"))
        .expect("workspace library directory must be created");
    fs::create_dir_all(alpha.join("tests")).expect("workspace test directory must be created");
    fs::create_dir_all(beta.join("src")).expect("workspace member directory must be created");
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"3\"\n",
    )
    .expect("workspace manifest must be written");
    fs::write(
        alpha.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"source/entry.rs\"\n\n[[bin]]\nname = \"alpha-tool\"\npath = \"bin/cli_test.rs\"\n\n[[bin]]\nname = \"alpha-selected\"\npath = \"tests/selected.rs\"\n\n[[test]]\nname = \"alpha-check\"\npath = \"source/check.rs\"\n",
    )
    .expect("alpha manifest must be written");
    fs::write(
        beta.join("Cargo.toml"),
        "[package]\nname = \"beta\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("beta manifest must be written");
    fs::write(alpha.join("bin").join("cli_test.rs"), "fn main() {}\n")
        .expect("workspace binary source must be written");
    fs::write(alpha.join("tests").join("selected.rs"), "fn main() {}\n")
        .expect("workspace selected source must be written");
    fs::write(alpha.join("source").join("entry.rs"), "mod helper;\n")
        .expect("workspace library source must be written");
    fs::write(
        alpha.join("source").join("check.rs"),
        "include!(\"check_support.rs\");\n#[path = \"helper.rs\"] mod shared_helper;\n#[test] fn check() {}\n",
    )
    .expect("workspace test source must be written");
    fs::write(
        alpha.join("source").join("check_support.rs"),
        "fn check_support() {}\n",
    )
    .expect("workspace test helper must be written");
    fs::write(
        alpha.join("source").join("helper.rs"),
        "pub fn helper() {}\n",
    )
    .expect("workspace library helper must be written");
    fs::write(
        alpha.join("source").join("nested").join("inside.rs"),
        "pub fn inside() {}\n",
    )
    .expect("workspace nested source must be written");
    fs::write(beta.join("src").join("lib.rs"), "pub fn beta() {}\n")
        .expect("workspace member source must be written");
    fs::canonicalize(workspace).expect("workspace path must resolve")
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
    loop {
        let root = smoke_root_name();

        match fs::create_dir(&root) {
            Ok(()) => return root,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("smoke test root must be created: {error}"),
        }
    }
}

fn smoke_root_name() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after the Unix epoch")
        .as_nanos();
    let sequence = NEXT_SMOKE_ROOT.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "mutarust-install-smoke-{}-{nonce}-{sequence}",
        std::process::id(),
    ))
}

fn command_path(install: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "mutarust.exe"
    } else {
        "mutarust"
    };
    install.join("bin").join(name)
}
