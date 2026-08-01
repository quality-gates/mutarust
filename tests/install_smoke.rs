use std::env;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SMOKE_ROOT: AtomicU64 = AtomicU64::new(0);

struct SmokeRoot(PathBuf);

impl Deref for SmokeRoot {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for SmokeRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for_file(path: &Path, message: &str) {
    for _ in 0..100 {
        if path.is_file() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{message}");
}

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
}

#[test]
fn installed_command_lists_builtin_mutators() {
    let root = smoke_root();
    let install = install_command(&root);

    let output = Command::new(command_path(&install))
        .arg("--list-mutators")
        .output()
        .expect("installed mutarust must start");

    assert!(output.status.success(), "--list-mutators must succeed");
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("mutator list must be UTF-8")
            .trim(),
        "conditional/bool-literal",
        "the built-in mutator list must be stable and sorted"
    );
}

#[test]
fn installed_command_classifies_killed_and_escaped_mutants() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let user_target = root.join("user-target");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&*root)
        .env("CARGO_TARGET_DIR", &user_target)
        .output()
        .expect("installed mutarust must start");

    assert!(output.status.success(), "mutation run must succeed");
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("killed ") && stdout.contains("escaped "),
        "one mutant must be killed and one must escape: {stdout}"
    );
    assert!(
        stdout.contains("Killed: 1") && stdout.contains("Escaped: 1"),
        "the final counts must use mutation result terms: {stdout}"
    );
    assert_eq!(
        fs::read_to_string(source).expect("fixture source must remain readable"),
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\n",
        "the mutation run must not change user source"
    );
    assert!(
        !user_target.exists(),
        "the mutation run must not write Cargo output to the user target directory"
    );
}

#[test]
fn installed_command_reports_errored_mutants_when_baseline_tests_fail() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    fs::write(
        fixture.join("checked").join("tests").join("baseline.rs"),
        "#[test]\nfn fails_before_mutation() {\n    assert!(false);\n}\n",
    )
    .expect("failing baseline test must be written");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(output.status.success(), "mutation run must complete");
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Killed: 0") && stdout.contains("Errored: 2"),
        "a failing baseline must not kill mutants: {stdout}"
    );
    assert!(
        stdout.contains("unmodified cargo test did not pass"),
        "the baseline failure must be reported: {stdout}"
    );
}

#[test]
fn installed_command_runs_external_source_with_local_dependency_and_configuration() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_external_mutation_fixture(&root);
    let source = fixture.join("external").join("lib.rs");
    let project = fixture.join("project");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&project)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "external source mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Killed:") && stdout.contains("Escaped: 1"),
        "the external source must use its package tests: {stdout}"
    );
    assert!(
        !project.join("Cargo.lock").exists(),
        "source discovery must not create a user Cargo lock file"
    );
    assert_eq!(
        fs::read_to_string(source).expect("external source must remain readable"),
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\npub fn configured() -> bool { cfg!(config_check) }\npub fn local_value() -> u8 { local_support::value() }\n",
        "the mutation run must not change the external user source"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_rewrites_an_absolute_cargo_runner_path() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let runner = root.join("outside-runner");
    fs::write(
        &runner,
        "#!/bin/sh\nif [ \"$0\" = \"$MUTARUST_SOURCE_RUNNER\" ]; then\n  exit 75\nfi\nexec \"$@\"\n",
    )
    .expect("Cargo runner must be written");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("Cargo runner must be executable");
    fs::create_dir_all(fixture.join(".cargo"))
        .expect("Cargo configuration directory must be created");
    fs::write(
        fixture.join(".cargo").join("config.toml"),
        format!("[target.'cfg(unix)']\nrunner = \"{}\"\n", runner.display()),
    )
    .expect("Cargo configuration must be written");
    let source = fixture.join("checked").join("src").join("lib.rs");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("MUTARUST_SOURCE_RUNNER", &runner)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "the copied Cargo configuration must use the copied runner: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Killed: 1")
            && stdout.contains("Escaped: 1")
            && stdout.contains("Errored: 0"),
        "the copied runner must run the mutation tests: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_stops_test_at_timeout() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let fake_cargo = root.join("fake-cargo");
    let child_identifier = root.join("timed-out-child");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nsleep 30 &\necho $! > \"$MUTARUST_TIMED_OUT_CHILD\"\nwait\n",
            env!("CARGO")
        ),
    )
    .expect("fake cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("fake cargo command must be executable");

    let started = std::time::Instant::now();
    let output = Command::new(command_path(&install))
        .args(["--timeout", "1"])
        .arg(&source)
        .current_dir(&*root)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_TIMED_OUT_CHILD", &child_identifier)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "timeout result must not stop the run"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "each timed out test must stop promptly"
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Errored: 2") && stdout.contains("cargo test timed out after 1 seconds"),
        "timeouts must produce errored results with detail: {stdout}"
    );
    let child = fs::read_to_string(child_identifier)
        .expect("timed out child identifier must be written")
        .trim()
        .to_owned();
    let child_status = Command::new("kill")
        .args(["-0", &child])
        .status()
        .expect("process check must start");
    assert!(
        !child_status.success(),
        "the timeout must stop the test child process"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_stops_test_at_interrupt() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let fake_cargo = root.join("interrupted-cargo");
    let cargo_identifier = root.join("interrupted-cargo-identifier");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\necho $$ > \"$MUTARUST_INTERRUPTED_CARGO\"\nsleep 30\n",
            env!("CARGO")
        ),
    )
    .expect("fake cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("fake cargo command must be executable");

    let mut command = Command::new(command_path(&install));
    let mut process = command
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_INTERRUPTED_CARGO", &cargo_identifier)
        .spawn()
        .expect("installed mutarust must start");
    wait_for_file(&cargo_identifier, "interrupted Cargo process must start");
    let signal = Command::new("kill")
        .args(["-INT", &process.id().to_string()])
        .status()
        .expect("interrupt command must start");
    assert!(signal.success(), "interrupt command must succeed");

    let status = process.wait().expect("installed mutarust must stop");
    assert!(!status.success(), "an interrupted mutation run must fail");
    let cargo = fs::read_to_string(&cargo_identifier)
        .expect("interrupted Cargo identifier must be written")
        .trim()
        .to_owned();
    let cargo_status = Command::new("kill")
        .args(["-0", &cargo])
        .status()
        .expect("Cargo process check must start");
    assert!(
        !cargo_status.success(),
        "the interrupt must stop the Cargo process"
    );
}

#[test]
fn packaged_library_builds_a_custom_mutator() {
    let root = smoke_root();
    let package = package_crate(&root.join("package-target"));
    let downstream = root.join("downstream");
    fs::create_dir_all(downstream.join("src"))
        .expect("downstream source directory must be created");
    fs::write(
        downstream.join("Cargo.toml"),
        format!(
            "[package]\nname = \"downstream\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmutarust = {{ path = \"{}\" }}\n",
            package.display()
        ),
    )
    .expect("downstream manifest must be written");
    fs::write(
        downstream.join("src").join("main.rs"),
        "use mutarust::{Mutation, Mutator, Registry, RegistryBuilder};\n\nstruct Custom;\nstruct Invalid;\n\nimpl Mutator for Custom {\n    fn name(&self) -> &str { \"custom/no-op\" }\n\n    fn mutations(&self, _source: &str) -> Vec<Mutation> { Vec::new() }\n}\n\nimpl Mutator for Invalid {\n    fn name(&self) -> &str { \"Custom\" }\n\n    fn mutations(&self, _source: &str) -> Vec<Mutation> { Vec::new() }\n}\n\nfn mutate(registry: &Registry, source: &str) -> String {\n    let mutation = registry.get(\"conditional/bool-literal\").expect(\"built-in mutator must exist\").mutations(source).pop().expect(\"boolean must mutate\");\n    mutation.apply(source).expect(\"mutation must apply\")\n}\n\nfn main() {\n    let registry = RegistryBuilder::with_builtins().register(Custom).expect(\"custom mutator must register\").build();\n    assert_eq!(registry.names().collect::<Vec<_>>(), vec![\"conditional/bool-literal\", \"custom/no-op\"]);\n    let duplicate = RegistryBuilder::new().register(Custom).expect(\"first custom mutator must register\").register(Custom).err().expect(\"duplicate must fail\");\n    assert_eq!(duplicate.to_string(), \"duplicate mutator name: custom/no-op\");\n    let invalid = RegistryBuilder::new().register(Invalid).err().expect(\"invalid name must fail\");\n    assert_eq!(invalid.to_string(), \"invalid mutator name: Custom\");\n    assert_eq!(mutate(&registry, \"fn enabled() -> bool { true }\"), \"fn enabled() -> bool { false }\");\n    assert_eq!(mutate(&registry, \"fn enabled() -> bool { let label = \\\"é\\\"; true }\"), \"fn enabled() -> bool { let label = \\\"é\\\"; false }\");\n    assert_eq!(mutate(&registry, \"fn check() { assert!(true); }\"), \"fn check() { assert!(false); }\");\n    println!(\"custom mutator works\");\n}\n",
    )
    .expect("downstream source must be written");

    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet"])
        .current_dir(&downstream)
        .output()
        .expect("downstream command must start");

    assert!(
        output.status.success(),
        "downstream command must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("downstream output must be UTF-8")
            .trim(),
        "custom mutator works"
    );
}

#[test]
fn packaged_library_runs_one_duplicate_custom_mutation() {
    let root = smoke_root();
    let package = package_crate(&root.join("package-target"));
    let downstream = root.join("downstream");
    fs::create_dir_all(downstream.join("src"))
        .expect("downstream source directory must be created");
    fs::write(
        downstream.join("Cargo.toml"),
        format!(
            "[package]\nname = \"duplicate-downstream\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmutarust = {{ path = \"{}\" }}\n",
            package.display()
        ),
    )
    .expect("downstream manifest must be written");
    fs::write(
        downstream.join("src").join("main.rs"),
        "use mutarust::{Mutation, Mutator, RegistryBuilder};\n\nstruct Duplicate;\n\nimpl Mutator for Duplicate {\n    fn name(&self) -> &str { \"custom/duplicate\" }\n\n    fn mutations(&self, _source: &str) -> Vec<Mutation> {\n        vec![Mutation::new(0..0, \"\"), Mutation::new(0..0, \"\")]\n    }\n}\n\nfn main() {\n    let source = concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/main.rs\").to_owned();\n    let registry = RegistryBuilder::new().register(Duplicate).expect(\"mutator must register\").build();\n    let run = mutarust::run_mutation_tests(&[source], &registry).expect(\"mutation run must work\");\n    assert_eq!(run.results().len(), 1);\n    println!(\"duplicate mutation runs once\");\n}\n",
    )
    .expect("downstream source must be written");

    let output = Command::new(env!("CARGO"))
        .args(["run", "--quiet"])
        .current_dir(&downstream)
        .output()
        .expect("downstream command must start");

    assert!(
        output.status.success(),
        "downstream command must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("downstream output must be UTF-8")
            .trim(),
        "duplicate mutation runs once"
    );
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
    let explicit_test_package = write_explicit_test_package(&fixture);
    let explicit_test_package_target = explicit_test_package.join("...");
    let from_explicit_test_package = list_files(
        &install,
        &fixture,
        &[explicit_test_package_target.as_os_str()],
    );
    assert_eq!(
        from_explicit_test_package,
        format!(
            "{}\n{}",
            explicit_test_package.join("src").join("lib.rs").display(),
            explicit_test_package
                .join("tests")
                .join("integration.rs")
                .display()
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
    let windows_source = cfg!(windows)
        .then(|| {
            format!(
                "\n{}",
                alpha.join("source").join("windows_only.rs").display()
            )
        })
        .unwrap_or_default();
    let alpha_source_count = 10 + usize::from(cfg!(windows));
    let alpha_recursive_source_count = 7 + usize::from(cfg!(windows));
    let workspace_direct = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}{}\n{}\n{}\n{}",
        alpha.join("bin").join("cli_test.rs").display(),
        alpha.join("source").join("debug_only.rs").display(),
        alpha.join("source").join("entry.rs").display(),
        alpha.join("source").join("helper.rs").display(),
        alpha
            .join("source")
            .join("nested")
            .join("custom.rs")
            .display(),
        alpha
            .join("source")
            .join("nested")
            .join("production_feature.rs")
            .display(),
        alpha.join("source").join("switch.rs").display(),
        windows_source,
        alpha
            .join("tests")
            .join("nested")
            .join("helper.rs")
            .display(),
        alpha
            .join("tests")
            .join("nested")
            .join("selected.rs")
            .display(),
        beta.join("src").join("lib.rs").display(),
    );
    let from_workspace = list_files(&install, &workspace, &[workspace.as_os_str()]);
    assert_eq!(from_workspace, workspace_direct);
    assert!(
        !alpha.join("build-script-ran").exists(),
        "source listing must not run the package build script"
    );

    let workspace_recursive = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}{}\n{}\n{}\n{}",
        alpha.join("bin").join("cli_test.rs").display(),
        alpha.join("source").join("debug_only.rs").display(),
        alpha.join("source").join("entry.rs").display(),
        alpha.join("source").join("helper.rs").display(),
        alpha
            .join("source")
            .join("nested")
            .join("custom.rs")
            .display(),
        alpha
            .join("source")
            .join("nested")
            .join("inside.rs")
            .display(),
        alpha
            .join("source")
            .join("nested")
            .join("production_feature.rs")
            .display(),
        alpha.join("source").join("switch.rs").display(),
        windows_source,
        alpha
            .join("tests")
            .join("nested")
            .join("helper.rs")
            .display(),
        alpha
            .join("tests")
            .join("nested")
            .join("selected.rs")
            .display(),
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
            .take(alpha_recursive_source_count)
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
            .take(alpha_source_count)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let from_custom_package = list_files(&install, &workspace, &["alpha...".as_ref()]);
    assert_eq!(
        from_custom_package,
        workspace_recursive
            .lines()
            .take(alpha_source_count)
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

    let configured_target = write_configured_target_fixture(&root);
    let configured_library_source = configured_target.join("src").join("lib.rs");
    let windows_source = configured_target.join("src").join("windows.rs");
    let from_configured_target = list_files(&install, &configured_target, &[]);
    assert_eq!(
        from_configured_target,
        format!(
            "{}\n{}",
            configured_library_source.display(),
            windows_source.display()
        ),
        "Cargo build target must select the target source"
    );

    let configured_target_precedence = write_target_precedence_fixture(&root);
    let precedence_library_source = configured_target_precedence.join("src").join("lib.rs");
    let precedence_source = configured_target_precedence.join("src").join("windows.rs");
    let from_precedence_fixture = list_files(&install, &configured_target_precedence, &[]);
    assert_eq!(
        from_precedence_fixture,
        format!(
            "{}\n{}",
            precedence_library_source.display(),
            precedence_source.display()
        ),
        "extensionless Cargo config must take precedence"
    );

    let configured_target_array = write_target_array_fixture(&root);
    let array_fallback_source = configured_target_array
        .join("src")
        .join("fallback_platform.rs");
    let array_unix_source = configured_target_array.join("src").join("unix.rs");
    let array_unix_path_source = configured_target_array.join("src").join("unix_path.rs");
    let array_windows_source = configured_target_array.join("src").join("windows.rs");
    let array_windows_default_source = configured_target_array
        .join("src")
        .join("windows_default.rs");
    let array_windows_fallback_source = configured_target_array
        .join("src")
        .join("windows_fallback_path.rs");
    let array_windows_path_source = configured_target_array.join("src").join("windows_path.rs");
    let from_array_fixture = list_files(&install, &configured_target_array, &[]);
    assert_eq!(
        from_array_fixture,
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            array_fallback_source.display(),
            configured_target_array
                .join("src")
                .join("impossible.rs")
                .display(),
            configured_target_array
                .join("src")
                .join("impossible_attributes.rs")
                .display(),
            configured_target_array.join("src").join("lib.rs").display(),
            configured_target_array
                .join("src")
                .join("unavailable_child.rs")
                .display(),
            array_unix_source.display(),
            array_unix_path_source.display(),
            array_windows_source.display(),
            array_windows_default_source.display(),
            array_windows_fallback_source.display(),
            configured_target_array
                .join("src")
                .join("windows_parent.rs")
                .display(),
            array_windows_path_source.display()
        ),
        "each configured Cargo target must select module paths"
    );

    let host_tuple_target = write_host_tuple_target_fixture(&root);
    let host_tuple_library_source = host_tuple_target.join("src").join("lib.rs");
    let host_tuple_windows_source = host_tuple_target.join("src").join("windows.rs");
    let from_host_tuple_target = list_files(&install, &host_tuple_target, &[]);
    let host_tuple_expected = if cfg!(windows) {
        format!(
            "{}\n{}",
            host_tuple_library_source.display(),
            host_tuple_windows_source.display()
        )
    } else {
        format!(
            "{}\n{}\n{}",
            host_tuple_library_source.display(),
            host_tuple_target.join("src").join("unix.rs").display(),
            host_tuple_windows_source.display()
        )
    };
    assert_eq!(
        from_host_tuple_target, host_tuple_expected,
        "host-tuple must preserve other configured targets"
    );

    let included_target = write_included_target_fixture(&root);
    let included_library_source = included_target.join("src").join("lib.rs");
    let included_windows_source = included_target.join("src").join("windows.rs");
    let from_included_target = list_files(&install, &included_target, &[]);
    assert_eq!(
        from_included_target,
        format!(
            "{}\n{}",
            included_library_source.display(),
            included_windows_source.display()
        ),
        "included Cargo configuration must select the target source"
    );

    let from_external_directory = list_files(&install, &root, &[included_target.as_os_str()]);
    assert_eq!(
        from_external_directory,
        format!(
            "{}\n{}",
            included_library_source.display(),
            included_windows_source.display()
        ),
        "a selected project must use its Cargo configuration"
    );

    let diamond_include = write_diamond_include_fixture(&root);
    let diamond_library_source = diamond_include.join("src").join("lib.rs");
    let diamond_windows_source = diamond_include.join("src").join("windows.rs");
    let from_diamond_include = list_files(&install, &diamond_include, &[]);
    assert_eq!(
        from_diamond_include,
        format!(
            "{}\n{}",
            diamond_library_source.display(),
            diamond_windows_source.display()
        ),
        "Cargo configuration includes may share an included file"
    );

    let invalid_include = write_invalid_include_fixture(&root);
    let invalid_include_error = list_files_error(&install, &invalid_include);
    assert!(
        invalid_include_error.contains("include path must end in .toml"),
        "non-TOML Cargo includes must fail discovery"
    );
    let environment_invalid_include_error =
        list_files_error_with_build_target(&install, &invalid_include);
    assert!(
        environment_invalid_include_error.contains("include path must end in .toml"),
        "CARGO_BUILD_TARGET must not skip Cargo configuration validation"
    );

    let missing_include = write_missing_include_fixture(&root, false);
    let missing_include_error = list_files_error(&install, &missing_include);
    assert!(
        missing_include_error.contains("cannot read Cargo configuration"),
        "required missing Cargo includes must fail discovery"
    );

    let optional_include = write_missing_include_fixture(&root, true);
    let optional_library_source = optional_include.join("src").join("lib.rs");
    let optional_host_source = optional_include.join("src").join(if cfg!(windows) {
        "windows.rs"
    } else {
        "unix.rs"
    });
    let from_optional_include = list_files(&install, &optional_include, &[]);
    assert_eq!(
        from_optional_include,
        format!(
            "{}\n{}",
            optional_library_source.display(),
            optional_host_source.display()
        ),
        "optional missing Cargo includes must not set a target"
    );

    let optional_directory_include = write_optional_directory_include_fixture(&root);
    let optional_directory_error = list_files_error(&install, &optional_directory_include);
    assert!(
        optional_directory_error.contains("cannot read Cargo configuration"),
        "an optional Cargo include directory must fail discovery"
    );

    for invalid_target in [
        write_target_configuration_fixture(&root, "invalid-target", "not-a-real-target"),
        write_target_configuration_fixture(&root, "missing-custom-target", "targets/missing.json"),
    ] {
        let invalid_target_error = list_files_error(&install, &invalid_target);
        assert!(
            invalid_target_error.contains("cannot read Rust compiler configuration for target"),
            "invalid Cargo targets must fail discovery"
        );
    }

    let target_type_conflict = write_target_type_conflict_fixture(&root);
    let target_type_conflict_error = list_files_error(&install, &target_type_conflict);
    assert!(
        target_type_conflict_error.contains("build.target must have one type"),
        "mixed Cargo target configuration types must fail discovery"
    );

    let workspace_configuration = write_workspace_configuration_fixture(&root);
    let workspace_member = workspace_configuration.join("member");
    let workspace_library_source = workspace_member.join("src").join("lib.rs");
    let workspace_source = workspace_member.join("src").join(if cfg!(windows) {
        "unix.rs"
    } else {
        "windows.rs"
    });
    let from_configured_workspace = list_files(
        &install,
        &workspace_configuration,
        &[workspace_configuration.as_os_str()],
    );
    assert_eq!(
        from_configured_workspace,
        format!(
            "{}\n{}",
            workspace_library_source.display(),
            workspace_source.display()
        ),
        "workspace source selection must use the workspace Cargo configuration"
    );
    let from_configured_workspace_package = list_files(
        &install,
        &workspace_configuration,
        &["workspace-config-member".as_ref()],
    );
    assert_eq!(
        from_configured_workspace_package,
        format!(
            "{}\n{}",
            workspace_library_source.display(),
            workspace_source.display()
        ),
        "workspace package selection must use the workspace Cargo configuration"
    );

    let custom_target = write_ancestor_custom_target_fixture(&root);
    let custom_library_source = custom_target.join("src").join("lib.rs");
    let custom_windows_source = custom_target.join("src").join("windows.rs");
    let custom_windows_family_source = custom_target.join("src").join("windows_family.rs");
    let custom_atomic_source = custom_target.join("src").join("atomic.rs");
    let from_custom_target = list_files(&install, &custom_target, &[]);
    assert_eq!(
        from_custom_target,
        format!(
            "{}\n{}\n{}\n{}",
            custom_atomic_source.display(),
            custom_library_source.display(),
            custom_windows_source.display(),
            custom_windows_family_source.display()
        ),
        "Cargo target paths must resolve from their configuration"
    );

    let minimal_custom_target = write_minimal_custom_target_fixture(&root);
    let minimal_custom_target_sources = [
        "empty_abi.rs",
        "empty_environment.rs",
        "lib.rs",
        "no_os.rs",
        "unwind.rs",
        "vendor.rs",
    ]
    .map(|source| minimal_custom_target.join("src").join(source));
    assert_eq!(
        list_files(&install, &minimal_custom_target, &[]),
        minimal_custom_target_sources
            .iter()
            .map(|source| source.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        "valid custom target files must not require optional configuration fields"
    );

    let fuchsia_custom_target = write_fuchsia_custom_target_fixture(&root);
    let fuchsia_library_source = fuchsia_custom_target.join("src").join("lib.rs");
    let fuchsia_unix_source = fuchsia_custom_target.join("src").join("unix.rs");
    assert_eq!(
        list_files(&install, &fuchsia_custom_target, &[]),
        format!(
            "{}\n{}",
            fuchsia_library_source.display(),
            fuchsia_unix_source.display()
        ),
        "Fuchsia custom targets must select Unix sources"
    );

    let cargo_home = root.join("cargo-home");
    fs::create_dir_all(&cargo_home).expect("Cargo home directory must be created");
    fs::write(
        cargo_home.join("config.toml"),
        "[build]\ntarget = [\"x86_64-pc-windows-gnu\"]\n",
    )
    .expect("Cargo home configuration must be written");
    let cargo_home_target = write_target_fixture(&root, "cargo-home-target");
    fs::create_dir_all(cargo_home_target.join(".cargo"))
        .expect("local Cargo configuration directory must be created");
    fs::write(
        cargo_home_target.join(".cargo").join("config.toml"),
        "[build]\ntarget = [\"x86_64-unknown-linux-gnu\"]\n",
    )
    .expect("local Cargo configuration must be written");
    let cargo_home_unix_source = cargo_home_target.join("src").join("unix.rs");
    let cargo_home_windows_source = cargo_home_target.join("src").join("windows.rs");
    let from_cargo_home = list_files_with_cargo_home(&install, &cargo_home_target, &cargo_home);
    assert_eq!(
        from_cargo_home,
        format!(
            "{}\n{}\n{}",
            cargo_home_target.join("src").join("lib.rs").display(),
            cargo_home_unix_source.display(),
            cargo_home_windows_source.display()
        ),
        "Cargo target arrays from configuration files must merge"
    );

    let ignored_toml_fixture = write_extensionless_config_fixture(&root);
    let ignored_toml_library_source = ignored_toml_fixture.join("src").join("lib.rs");
    let ignored_toml_source = ignored_toml_fixture.join("src").join(if cfg!(windows) {
        "windows.rs"
    } else {
        "unix.rs"
    });
    let from_ignored_toml = list_files(&install, &ignored_toml_fixture, &[]);
    assert_eq!(
        from_ignored_toml,
        format!(
            "{}\n{}",
            ignored_toml_library_source.display(),
            ignored_toml_source.display()
        ),
        "an extensionless Cargo configuration must suppress config.toml"
    );
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
        .args(["--locked", "--force"])
        .env("CARGO_TARGET_DIR", target)
        .status()
        .expect("cargo install must start");

    assert!(install_status.success(), "cargo install must succeed");
    install
}

fn write_explicit_test_package(fixture: &Path) -> PathBuf {
    let package = fixture.join("tests").join("fixture-crate");
    fs::create_dir_all(package.join("src"))
        .expect("explicit test package source directory must be created");
    fs::create_dir_all(package.join("tests"))
        .expect("explicit test package test directory must be created");
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = \"fixture-crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("explicit test package manifest must be written");
    fs::write(package.join("src").join("lib.rs"), "pub fn fixture() {}\n")
        .expect("explicit test package library must be written");
    fs::write(
        package.join("tests").join("integration.rs"),
        "#[test] fn test() {}\n",
    )
    .expect("explicit test package integration test must be written");

    package
}

fn write_mutation_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("mutation-fixture");
    fs::create_dir_all(fixture.join("checked").join("src"))
        .expect("checked mutation fixture source must be created");
    fs::create_dir_all(fixture.join("checked").join("tests"))
        .expect("checked mutation fixture tests must be created");
    fs::create_dir_all(fixture.join("other").join("src"))
        .expect("other mutation fixture source must be created");
    fs::create_dir_all(fixture.join("other").join("tests"))
        .expect("other mutation fixture tests must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[workspace]\nmembers = [\"checked\", \"other\"]\nresolver = \"2\"\n",
    )
    .expect("mutation fixture manifest must be written");
    fs::write(
        fixture.join("checked").join("Cargo.toml"),
        "[package]\nname = \"mutation-checked\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("checked mutation fixture manifest must be written");
    fs::write(
        fixture.join("checked").join("src").join("lib.rs"),
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\n",
    )
    .expect("mutation fixture source must be written");
    fs::write(
        fixture.join("checked").join("tests").join("mutation.rs"),
        "#[test]\nfn detects_checked_value() {\n    assert!(mutation_checked::checked());\n}\n",
    )
    .expect("mutation fixture test must be written");
    fs::write(
        fixture.join("other").join("Cargo.toml"),
        "[package]\nname = \"mutation-other\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("other mutation fixture manifest must be written");
    fs::write(
        fixture.join("other").join("src").join("lib.rs"),
        "pub fn value() {}\n",
    )
    .expect("other mutation fixture source must be written");
    fs::write(
        fixture.join("other").join("tests").join("failing.rs"),
        "#[test]\nfn is_unrelated_and_failing() {\n    assert!(false);\n}\n",
    )
    .expect("other mutation fixture test must be written");
    fixture
}

fn write_external_mutation_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("external-mutation-fixture");
    let project = fixture.join("project");
    let application = project.join("application");
    let external = fixture.join("external");
    let support = fixture.join("support");
    fs::create_dir_all(application.join("tests")).expect("external fixture tests must be created");
    fs::create_dir_all(&external).expect("external fixture source must be created");
    fs::create_dir_all(support.join("src")).expect("local dependency source must be created");
    fs::create_dir_all(fixture.join(".cargo"))
        .expect("ancestor Cargo configuration must be created");
    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"application\"]\nresolver = \"2\"\n",
    )
    .expect("external fixture workspace manifest must be written");
    fs::write(
        application.join("Cargo.toml"),
        "[package]\nname = \"external-mutation-application\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[package.metadata]\nreport_dir = \"/tmp/mutarust-user-report\"\n\n[package.metadata.tool.dependencies]\npath = \"/tmp/mutarust-user-report\"\n\n[lib]\npath = \"../../external/lib.rs\"\n\n[dependencies]\nlocal-support = { path = \"../../support\" }\n",
    )
    .expect("external fixture package manifest must be written");
    fs::write(
        external.join("lib.rs"),
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\npub fn configured() -> bool { cfg!(config_check) }\npub fn local_value() -> u8 { local_support::value() }\n",
    )
    .expect("external fixture source must be written");
    fs::write(
        application.join("tests").join("mutation.rs"),
        "#[test]\nfn detects_required_values() {\n    assert!(external_mutation_application::checked());\n    assert!(external_mutation_application::configured());\n    assert_eq!(external_mutation_application::local_value(), 7);\n}\n",
    )
    .expect("external fixture test must be written");
    fs::write(
        support.join("Cargo.toml"),
        "[package]\nname = \"local-support\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("local dependency manifest must be written");
    fs::write(
        support.join("src").join("lib.rs"),
        "pub fn value() -> u8 { 7 }\n",
    )
    .expect("local dependency source must be written");
    fs::write(
        fixture.join(".cargo").join("config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"config_check\"]\n",
    )
    .expect("ancestor Cargo configuration must be written");
    fixture
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
        "mod math;\n#[cfg(test)] mod tests { include!(\"test_support.rs\"); }\n#[cfg(test)] mod shared_tests { #[path = \"../math.rs\"] mod shared_math; }\n#[cfg(test)] mod external_tests;\n#[cfg_attr(test, path = \"cfg_attr_test_support.rs\")] mod cfg_attr_switch;\n#[cfg(not(not(test)))] mod double_test_support;\n",
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
        fixture.join("src").join("cfg_attr_test_support.rs"),
        "pub fn cfg_attr_test_support() {}\n",
    )
    .expect("fixture cfg_attr test support must be written");
    fs::write(
        fixture.join("src").join("double_test_support.rs"),
        "pub fn double_test_support() {}\n",
    )
    .expect("fixture double test support must be written");
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

fn write_configured_target_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "configured-target");
    fs::create_dir_all(fixture.join(".cargo")).expect("target configuration directory must exist");
    fs::write(
        fixture.join(".cargo").join("config.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("Cargo target configuration must be written");
    fixture
}

fn write_target_precedence_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "target-precedence");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("extensionless Cargo configuration must be written");
    fs::write(
        configuration_directory.join("config.toml"),
        "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n",
    )
    .expect("TOML Cargo configuration must be written");
    fixture
}

fn write_target_array_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "target-array");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "[build]\ntarget = [\"x86_64-pc-windows-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
    )
    .expect("Cargo target array configuration must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "#[cfg(windows)] mod windows;\n#[cfg(unix)] mod unix;\n#[cfg_attr(windows, path = \"windows_path.rs\")]\n#[cfg_attr(unix, path = \"unix_path.rs\")]\nmod platform;\n#[cfg_attr(windows, path = \"windows_fallback_path.rs\")]\nmod fallback_platform;\n#[cfg(windows)]\n#[cfg_attr(unix, path = \"unix_impossible.rs\")]\nmod windows_default;\n#[cfg(windows)] mod windows_parent;\n",
    )
    .expect("target array library source must be written");
    fs::write(
        fixture.join("src").join("windows_path.rs"),
        "pub fn windows_path() {}\n",
    )
    .expect("Windows path source must be written");
    fs::write(
        fixture.join("src").join("unix_path.rs"),
        "pub fn unix_path() {}\n",
    )
    .expect("Unix path source must be written");
    fs::write(
        fixture.join("src").join("fallback_platform.rs"),
        "pub fn fallback_platform() {}\n",
    )
    .expect("fallback path source must be written");
    fs::write(
        fixture.join("src").join("windows_fallback_path.rs"),
        "pub fn windows_fallback_path() {}\n",
    )
    .expect("Windows fallback path source must be written");
    fs::write(
        fixture.join("src").join("windows_default.rs"),
        "pub fn windows_default() {}\n",
    )
    .expect("Windows default source must be written");
    fs::write(
        fixture.join("src").join("unix_impossible.rs"),
        "pub fn unix_impossible() {}\n",
    )
    .expect("impossible Unix path source must be written");
    fs::write(
        fixture.join("src").join("windows_parent.rs"),
        "#[cfg(unix)] mod unavailable_child;\npub fn windows_parent() {}\n",
    )
    .expect("Windows parent source must be written");
    fs::write(
        fixture.join("src").join("unavailable_child.rs"),
        "pub fn unavailable_child() {}\n",
    )
    .expect("unavailable child source must be written");
    fixture
}

fn write_host_tuple_target_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "host-tuple-target");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "[build]\ntarget = [\"host-tuple\", \"x86_64-pc-windows-gnu\"]\n",
    )
    .expect("host tuple Cargo configuration must be written");
    fixture
}

fn write_included_target_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "included-target");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "include = [\"targets.toml\"]\n",
    )
    .expect("Cargo include configuration must be written");
    fs::write(
        configuration_directory.join("targets.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("included Cargo target must be written");
    fixture
}

fn write_diamond_include_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "diamond-include");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "include = [\"first.toml\", \"second.toml\"]\n",
    )
    .expect("root Cargo configuration must be written");
    fs::write(
        configuration_directory.join("first.toml"),
        "include = [\"shared.toml\"]\n",
    )
    .expect("first Cargo configuration must be written");
    fs::write(
        configuration_directory.join("second.toml"),
        "include = [\"shared.toml\"]\n",
    )
    .expect("second Cargo configuration must be written");
    fs::write(
        configuration_directory.join("shared.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("shared Cargo configuration must be written");
    fixture
}

fn write_ancestor_custom_target_fixture(root: &Path) -> PathBuf {
    let fixture_root = root.join("custom-target-config");
    let fixture = fixture_root.join("project");
    let configuration_directory = fixture_root.join(".cargo");
    fs::create_dir_all(fixture.join("src"))
        .expect("custom target source directory must be created");
    fs::create_dir_all(fixture_root.join("targets"))
        .expect("custom target specification directory must be created");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "[build]\ntarget = \"targets/windows.json\"\n",
    )
    .expect("custom target Cargo configuration must be written");
    fs::write(
        fixture_root.join("targets").join("windows.json"),
        "{\"llvm-target\":\"x86_64-pc-windows-gnu\",\"data-layout\":\"e-m:w-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128\",\"arch\":\"x86_64\",\"target-endian\":\"little\",\"target-pointer-width\":\"64\",\"target-c-int-width\":\"32\",\"max-atomic-width\":64,\"os\":\"windows\",\"env\":\"gnu\",\"vendor\":\"pc\",\"linker-flavor\":\"gnu\",\"executables\":true}\n",
    )
    .expect("custom target specification must be written");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"custom-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("custom target manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "#[cfg(windows)] mod windows;\n#[cfg(target_family = \"windows\")] mod windows_family;\n#[cfg(target_has_atomic = \"64\")] mod atomic;\n#[cfg(unix)] mod unix;\n",
    )
    .expect("custom target library source must be written");
    fs::write(
        fixture.join("src").join("windows.rs"),
        "pub fn windows() {}\n",
    )
    .expect("custom target Windows source must be written");
    fs::write(
        fixture.join("src").join("windows_family.rs"),
        "pub fn windows_family() {}\n",
    )
    .expect("custom target family source must be written");
    fs::write(
        fixture.join("src").join("atomic.rs"),
        "pub fn atomic() {}\n",
    )
    .expect("custom target atomic source must be written");
    fs::write(fixture.join("src").join("unix.rs"), "pub fn unix() {}\n")
        .expect("custom target Unix source must be written");
    fs::canonicalize(fixture).expect("custom target fixture must resolve")
}

fn write_minimal_custom_target_fixture(root: &Path) -> PathBuf {
    let fixture_root = root.join("minimal-custom-target-config");
    let fixture = fixture_root.join("project");
    fs::create_dir_all(fixture.join("src"))
        .expect("minimal custom target source directory must be created");
    fs::create_dir_all(fixture_root.join(".cargo"))
        .expect("minimal custom target configuration directory must be created");
    fs::write(
        fixture_root.join(".cargo").join("config.toml"),
        "[build]\ntarget = \"thumb.json\"\n",
    )
    .expect("minimal custom target Cargo configuration must be written");
    fs::write(
        fixture_root.join("thumb.json"),
        "{\"llvm-target\":\"thumbv6-none-eabi\",\"data-layout\":\"e-m:e-p:32:32-Fi8-i64:64-v128:64:128-a:0:32-n32-S64\",\"arch\":\"arm\",\"target-pointer-width\":32}\n",
    )
    .expect("minimal custom target specification must be written");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"minimal-custom-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("minimal custom target manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "#[cfg(target_os = \"none\")] mod no_os;\n#[cfg(target_vendor = \"unknown\")] mod vendor;\n#[cfg(target_env = \"\")] mod empty_environment;\n#[cfg(target_abi = \"\")] mod empty_abi;\n#[cfg(panic = \"unwind\")] mod unwind;\n",
    )
        .expect("minimal custom target library must be written");
    for source in [
        "no_os",
        "vendor",
        "empty_environment",
        "empty_abi",
        "unwind",
    ] {
        fs::write(
            fixture.join("src").join(format!("{source}.rs")),
            "pub fn source() {}\n",
        )
        .expect("minimal custom target conditional source must be written");
    }
    fs::canonicalize(fixture).expect("minimal custom target fixture must resolve")
}

fn write_fuchsia_custom_target_fixture(root: &Path) -> PathBuf {
    let fixture_root = root.join("fuchsia-custom-target-config");
    let fixture = fixture_root.join("project");
    fs::create_dir_all(fixture.join("src"))
        .expect("Fuchsia custom target source directory must be created");
    fs::create_dir_all(fixture_root.join(".cargo"))
        .expect("Fuchsia custom target configuration directory must be created");
    fs::write(
        fixture_root.join(".cargo").join("config.toml"),
        "[build]\ntarget = \"fuchsia.json\"\n",
    )
    .expect("Fuchsia custom target Cargo configuration must be written");
    fs::write(
        fixture_root.join("fuchsia.json"),
        "{\"llvm-target\":\"x86_64-unknown-fuchsia\",\"data-layout\":\"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128\",\"arch\":\"x86_64\",\"target-endian\":\"little\",\"target-pointer-width\":64,\"os\":\"fuchsia\"}\n",
    )
    .expect("Fuchsia custom target specification must be written");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"fuchsia-custom-target\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("Fuchsia custom target manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "#[cfg(unix)] mod unix;\n",
    )
    .expect("Fuchsia custom target library must be written");
    fs::write(fixture.join("src").join("unix.rs"), "pub fn unix() {}\n")
        .expect("Fuchsia custom target Unix source must be written");
    fs::canonicalize(fixture).expect("Fuchsia custom target fixture must resolve")
}

fn write_invalid_include_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "invalid-include");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "include = [\"targets.json\"]\n",
    )
    .expect("invalid Cargo include configuration must be written");
    fs::write(
        configuration_directory.join("targets.json"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("invalid Cargo include target must be written");
    fixture
}

fn write_missing_include_fixture(root: &Path, optional: bool) -> PathBuf {
    let name = if optional {
        "optional-include"
    } else {
        "missing-include"
    };
    let fixture = write_target_fixture(root, name);
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    let include = if optional {
        "{ path = \"missing.toml\", optional = true }"
    } else {
        "\"missing.toml\""
    };
    fs::write(
        configuration_directory.join("config.toml"),
        format!("include = [{include}]\n"),
    )
    .expect("missing Cargo include configuration must be written");
    fixture
}

fn write_optional_directory_include_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "optional-directory-include");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(configuration_directory.join("directory.toml"))
        .expect("optional Cargo include directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        "include = [{ path = \"directory.toml\", optional = true }]\n",
    )
    .expect("optional Cargo include configuration must be written");
    fixture
}

fn write_target_configuration_fixture(root: &Path, name: &str, target: &str) -> PathBuf {
    let fixture = write_target_fixture(root, name);
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config.toml"),
        format!("[build]\ntarget = \"{target}\"\n"),
    )
    .expect("Cargo target configuration must be written");
    fixture
}

fn write_target_type_conflict_fixture(root: &Path) -> PathBuf {
    let parent = root.join("target-type-conflict");
    let fixture = write_target_fixture(&parent, "project");
    fs::create_dir_all(parent.join(".cargo"))
        .expect("ancestor target configuration directory must be created");
    fs::create_dir_all(fixture.join(".cargo"))
        .expect("project target configuration directory must be created");
    fs::write(
        parent.join(".cargo").join("config.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("ancestor Cargo target configuration must be written");
    fs::write(
        fixture.join(".cargo").join("config.toml"),
        "[build]\ntarget = [\"x86_64-pc-windows-gnu\"]\n",
    )
    .expect("project Cargo target configuration must be written");
    fixture
}

fn write_workspace_configuration_fixture(root: &Path) -> PathBuf {
    let workspace = root.join("workspace-configuration");
    let member = workspace.join("member");
    let workspace_target = if cfg!(windows) {
        "x86_64-unknown-linux-gnu"
    } else {
        "x86_64-pc-windows-gnu"
    };
    let member_target = if cfg!(windows) {
        "x86_64-pc-windows-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    fs::create_dir_all(workspace.join(".cargo"))
        .expect("workspace configuration directory must be created");
    fs::create_dir_all(member.join(".cargo"))
        .expect("member configuration directory must be created");
    fs::create_dir_all(member.join("src"))
        .expect("workspace member source directory must be created");
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n",
    )
    .expect("workspace manifest must be written");
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"workspace-config-member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("workspace member manifest must be written");
    fs::write(
        workspace.join(".cargo").join("config.toml"),
        format!("[build]\ntarget = \"{workspace_target}\"\n"),
    )
    .expect("workspace Cargo configuration must be written");
    fs::write(
        member.join(".cargo").join("config.toml"),
        format!("[build]\ntarget = \"{member_target}\"\n"),
    )
    .expect("member Cargo configuration must be written");
    fs::write(
        member.join("src").join("lib.rs"),
        "#[cfg(windows)] mod windows;\n#[cfg(unix)] mod unix;\n",
    )
    .expect("workspace member library source must be written");
    fs::write(
        member.join("src").join("windows.rs"),
        "pub fn windows() {}\n",
    )
    .expect("workspace member Windows source must be written");
    fs::write(member.join("src").join("unix.rs"), "pub fn unix() {}\n")
        .expect("workspace member Unix source must be written");
    fs::canonicalize(workspace).expect("workspace configuration fixture must resolve")
}

fn write_extensionless_config_fixture(root: &Path) -> PathBuf {
    let fixture = write_target_fixture(root, "extensionless-config");
    let configuration_directory = fixture.join(".cargo");
    fs::create_dir_all(&configuration_directory)
        .expect("target configuration directory must be created");
    fs::write(
        configuration_directory.join("config"),
        "[term]\nquiet = true\n",
    )
    .expect("extensionless Cargo configuration must be written");
    fs::write(
        configuration_directory.join("config.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-gnu\"\n",
    )
    .expect("suppressed TOML Cargo configuration must be written");
    fixture
}

fn write_target_fixture(root: &Path, name: &str) -> PathBuf {
    let fixture = root.join(name);
    fs::create_dir_all(fixture.join("src")).expect("target source directory must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )
    .expect("target manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "#[cfg(windows)] mod windows;\n#[cfg(unix)] mod unix;\n#[cfg(all(windows, unix))] mod impossible;\n#[cfg(windows)]\n#[cfg(unix)]\nmod impossible_attributes;\n",
    )
    .expect("target library source must be written");
    fs::write(
        fixture.join("src").join("windows.rs"),
        "pub fn windows() {}\n",
    )
    .expect("Windows source must be written");
    fs::write(fixture.join("src").join("unix.rs"), "pub fn unix() {}\n")
        .expect("Unix source must be written");
    fs::write(
        fixture.join("src").join("impossible.rs"),
        "pub fn impossible() {}\n",
    )
    .expect("impossible target source must be written");
    fs::write(
        fixture.join("src").join("impossible_attributes.rs"),
        "pub fn impossible_attributes() {}\n",
    )
    .expect("impossible target attribute source must be written");
    fs::canonicalize(fixture).expect("target fixture path must resolve")
}

fn write_workspace_fixture(root: &Path) -> PathBuf {
    let workspace = root.join("workspace");
    let alpha = workspace.join("alpha");
    let beta = workspace.join("beta");
    fs::create_dir_all(alpha.join("bin")).expect("workspace binary directory must be created");
    fs::create_dir_all(alpha.join("source").join("nested"))
        .expect("workspace library directory must be created");
    fs::create_dir_all(alpha.join("tests").join("nested").join("support"))
        .expect("workspace test directory must be created");
    fs::create_dir_all(beta.join("src")).expect("workspace member directory must be created");
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"3\"\n",
    )
    .expect("workspace manifest must be written");
    fs::write(
        alpha.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[features]\ndefault = [\"enabled\"]\nenabled = []\n\n[lib]\npath = \"source/entry.rs\"\n\n[[bin]]\nname = \"alpha-tool\"\npath = \"bin/cli_test.rs\"\n\n[[bin]]\nname = \"alpha-selected\"\npath = \"tests/nested/selected.rs\"\n\n[[bin]]\nname = \"alpha-disabled\"\npath = \"bin/disabled.rs\"\nrequired-features = [\"experimental\"]\n\n[[test]]\nname = \"alpha-check\"\npath = \"source/check.rs\"\n",
    )
    .expect("alpha manifest must be written");
    fs::write(
        alpha.join("build.rs"),
        "std::fs::write(\"build-script-ran\", \"ran\").unwrap();\n",
    )
    .expect("workspace build script must be written");
    fs::write(
        beta.join("Cargo.toml"),
        "[package]\nname = \"beta\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("beta manifest must be written");
    fs::write(alpha.join("bin").join("cli_test.rs"), "fn main() {}\n")
        .expect("workspace binary source must be written");
    fs::write(
        alpha.join("tests").join("nested").join("selected.rs"),
        "mod helper;\nfn main() {}\n",
    )
    .expect("workspace selected source must be written");
    fs::write(
        alpha.join("tests").join("nested").join("helper.rs"),
        "pub fn selected_helper() {}\n",
    )
    .expect("workspace selected helper must be written");
    fs::write(
        alpha
            .join("tests")
            .join("nested")
            .join("support")
            .join("unused.rs"),
        "pub fn unused() {}\n",
    )
    .expect("workspace unused test source must be written");
    fs::write(alpha.join("bin").join("disabled.rs"), "fn main() {}\n")
        .expect("workspace disabled source must be written");
    fs::write(
        alpha.join("source").join("entry.rs"),
        "#[cfg(debug_assertions)] mod debug_only;\nmod helper;\n#[cfg(local_mode)]\n#[path = \"nested/custom.rs\"]\nmod custom;\n#[cfg(feature = \"extra\")] mod extra;\n#[cfg_attr(feature = \"extra\", path = \"feature_extra.rs\")] mod switch;\n#[cfg_attr(feature = \"enabled\", cfg(test))] mod cfg_attr_test;\n#[cfg(windows)] mod windows_only;\n#[cfg_attr(not(test), path = \"nested/production_feature.rs\")]\n#[cfg_attr(test, path = \"nested/test_feature.rs\")]\nmod configured;\n",
    )
    .expect("workspace library source must be written");
    fs::write(
        alpha.join("source").join("debug_only.rs"),
        "pub fn debug_only() {}\n",
    )
    .expect("workspace debug source must be written");
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
    fs::write(
        alpha.join("source").join("nested").join("custom.rs"),
        "pub fn custom() {}\n",
    )
    .expect("workspace custom configuration source must be written");
    fs::write(
        alpha
            .join("source")
            .join("nested")
            .join("production_feature.rs"),
        "#[cfg(test)] #[path = \"test_only_helper.rs\"] mod test_only;\npub fn production_feature() {}\n",
    )
    .expect("workspace production cfg_attr source must be written");
    fs::write(alpha.join("source").join("extra.rs"), "pub fn extra() {}\n")
        .expect("workspace inactive feature source must be written");
    fs::write(
        alpha.join("source").join("switch.rs"),
        "pub fn switch() {}\n",
    )
    .expect("workspace default cfg_attr source must be written");
    fs::write(
        alpha.join("source").join("feature_extra.rs"),
        "pub fn feature_extra() {}\n",
    )
    .expect("workspace inactive cfg_attr source must be written");
    fs::write(
        alpha.join("source").join("cfg_attr_test.rs"),
        "pub fn cfg_attr_test() {}\n",
    )
    .expect("workspace cfg_attr test source must be written");
    fs::write(
        alpha.join("source").join("windows_only.rs"),
        "pub fn windows_only() {}\n",
    )
    .expect("workspace target-only source must be written");
    fs::write(
        alpha
            .join("source")
            .join("nested")
            .join("test_only_helper.rs"),
        "pub fn test_only_helper() {}\n",
    )
    .expect("workspace cfg_attr test helper must be written");
    fs::write(
        alpha.join("source").join("nested").join("test_feature.rs"),
        "pub fn test_feature() {}\n",
    )
    .expect("workspace test cfg_attr source must be written");
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

    assert!(
        output.status.success(),
        "--list-files must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("file list must be UTF-8")
        .trim()
        .to_owned()
}

fn list_files_with_cargo_home(install: &Path, fixture: &Path, cargo_home: &Path) -> String {
    let output = Command::new(command_path(install))
        .arg("--list-files")
        .current_dir(fixture)
        .env("CARGO_HOME", cargo_home)
        .output()
        .expect("installed mutarust must list files");

    assert!(output.status.success(), "--list-files must succeed");
    String::from_utf8(output.stdout)
        .expect("file list must be UTF-8")
        .trim()
        .to_owned()
}

fn list_files_error(install: &Path, fixture: &Path) -> String {
    let output = Command::new(command_path(install))
        .arg("--list-files")
        .current_dir(fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(!output.status.success(), "--list-files must fail");
    String::from_utf8(output.stderr)
        .expect("error output must be UTF-8")
        .trim()
        .to_owned()
}

fn list_files_error_with_build_target(install: &Path, fixture: &Path) -> String {
    let output = Command::new(command_path(install))
        .arg("--list-files")
        .current_dir(fixture)
        .env("CARGO_BUILD_TARGET", "x86_64-pc-windows-gnu")
        .output()
        .expect("installed mutarust must start");

    assert!(
        !output.status.success(),
        "--list-files must reject invalid Cargo configuration"
    );
    String::from_utf8(output.stderr)
        .expect("error output must be UTF-8")
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

    let package_artifact = package_target
        .join("package")
        .join(format!("mutarust-{}.crate", env!("CARGO_PKG_VERSION")));
    let package_root = package_target.join("package-root");
    fs::create_dir_all(&package_root).expect("package root must be created");
    let unpack_status = Command::new("tar")
        .args(["-xzf"])
        .arg(&package_artifact)
        .args(["-C"])
        .arg(&package_root)
        .arg("--strip-components=1")
        .status()
        .expect("package extractor must start");

    assert!(unpack_status.success(), "package artifact must unpack");
    package_root
}

fn smoke_root() -> SmokeRoot {
    loop {
        let root = smoke_root_name();

        match fs::create_dir(&root) {
            Ok(()) => return SmokeRoot(root),
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
