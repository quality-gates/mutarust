use std::env;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SMOKE_ROOT: AtomicU64 = AtomicU64::new(0);
static INSTALLED_COMMAND: OnceLock<PathBuf> = OnceLock::new();
static INSTALL_CLEANUP_REGISTERED: OnceLock<()> = OnceLock::new();

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
    for _ in 0..1_000 {
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
fn installed_command_reads_yaml_configuration_and_command_options_take_priority() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let configuration = fixture.join("mutarust.yml");
    fs::write(
        &configuration,
        "skip_without_test: false\nskip_with_cfg: false\njson_output: false\nhtml_output: false\nsilent_mode: true\nmin_msi: 51\nmin_covered_msi: 0\nexclude_dirs: []\ndisable_mutators: []\nenable_mutators:\n  - conditional/bool-literal\nignore_source_lines:\n  - '^// generated'\n",
    )
    .expect("Mutarust configuration must be written");

    let silent = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with configuration");

    assert_eq!(
        silent.status.code(),
        Some(4),
        "a configured total-score gate must return exit value 4: {}",
        String::from_utf8_lossy(&silent.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&silent.stdout).contains("escaped "),
        "silent configuration must hide mutant status output"
    );

    let command_setting = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args(["--no-silent", "--min-msi", "50"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a command setting");

    assert!(
        command_setting.status.success(),
        "the command setting must succeed: {}",
        String::from_utf8_lossy(&command_setting.stderr)
    );
    assert!(
        String::from_utf8_lossy(&command_setting.stdout).contains("escaped "),
        "a command setting must take priority over the configuration value"
    );

    let disabled = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args(["--disable", "conditional/bool-literal", "--min-msi", "0"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a mutator command setting");

    assert!(
        disabled.status.success(),
        "the mutator command setting must succeed: {}",
        String::from_utf8_lossy(&disabled.stderr)
    );
    let disabled_output =
        String::from_utf8(disabled.stdout).expect("disabled mutator output must be UTF-8");
    assert!(
        disabled_output.contains("Killed: 0") && disabled_output.contains("Escaped: 0"),
        "a command mutator denylist must change the configuration selection: {disabled_output}"
    );
}

#[test]
fn installed_command_rejects_invalid_configuration() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let missing = fixture.join("missing.yml");
    assert!(
        !missing.exists(),
        "the missing configuration fixture must not exist"
    );
    let missing_error = configuration_error(&install, &fixture, &source, &missing);
    assert!(
        missing_error.contains("could not read configuration")
            && missing_error.contains("missing.yml"),
        "a missing configuration file must identify its path: {missing_error}"
    );

    let cases = [
        ("unknown.yml", "unknown_setting: true\n", "unknown field"),
        (
            "score.yml",
            "min_msi: 101\n",
            "min_msi must be a whole percentage",
        ),
        (
            "wrong-type.yml",
            "silent_mode: enabled\n",
            "could not parse configuration",
        ),
        (
            "empty-directory.yml",
            "exclude_dirs:\n  - ''\n",
            "exclude_dirs[0] must not be empty",
        ),
        (
            "regular-expression.yml",
            "ignore_source_lines:\n  - '('\n",
            "invalid regular expression",
        ),
        (
            "mutator.yml",
            "enable_mutators:\n  - conditional/*/wrong\n",
            "must be a mutator name or a group pattern",
        ),
        (
            "unknown-mutator.yml",
            "disable_mutators:\n  - value/does-not-exist\n",
            "does not match an available mutator",
        ),
    ];
    for (name, contents, expected) in cases {
        let configuration = fixture.join(name);
        fs::write(&configuration, contents).expect("invalid configuration must be written");
        let error = configuration_error(&install, &fixture, &source, &configuration);
        assert!(
            error.contains(expected) && error.contains(name),
            "invalid configuration must have a clear diagnostic: {error}"
        );
    }

    let output = Command::new(command_path(&install))
        .args(["--list-mutators", "--config", "mutarust.yml"])
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with incompatible options");
    assert!(
        !output.status.success(),
        "an unsupported option combination must fail"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not accept configuration"),
        "an unsupported option combination must have a clear diagnostic"
    );
}

#[test]
fn package_contains_the_configuration_contract() {
    let root = smoke_root();
    let package = package_crate(&root.join("package-target"));

    for file in [
        "docs/config.md",
        "schema/mutarust.schema.json",
        "mutarust.yml.example",
    ] {
        assert!(
            package.join(file).is_file(),
            "the package must contain {file}"
        );
    }
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
fn installed_command_reports_stable_evidence_and_enforces_total_score() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    let ids = stable_mutant_ids(&stdout);
    assert_eq!(
        ids,
        vec![
            "a4afb3df07ad704a7e118ca1f9c8ce1e".to_owned(),
            "7e44f5b6649ca4de087acb260e75a287".to_owned(),
        ],
        "each mutant must have the Mutago-compatible stable ID: {stdout}"
    );
    assert!(
        stdout.contains("--- checked/src/lib.rs")
            && stdout.contains("+++ checked/src/lib.rs")
            && stdout.contains("@@ -")
            && stdout.contains("-pub fn unchecked() -> bool { true }")
            && stdout.contains("+pub fn unchecked() -> bool { false }"),
        "an escaped mutant must show a readable source diff: {stdout}"
    );
    assert!(
        stdout.contains("Killed: 1")
            && stdout.contains("Escaped: 1")
            && stdout.contains("Errored: 0")
            && stdout.contains("Not covered: 0")
            && stdout.contains("Skipped: 0")
            && stdout.contains("Mutation score: 50.00%")
            && stdout.contains("Per-mutator results:")
            && stdout.contains("conditional/bool-literal"),
        "the summary must show result counts, score, and mutator results: {stdout}"
    );

    let original = fs::read_to_string(&source).expect("fixture source must remain readable");
    fs::write(&source, format!("\n{original}")).expect("line-only source edit must be written");
    let line_shifted = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start after a line-only edit");
    assert!(
        line_shifted.status.success(),
        "line-shifted mutation run must succeed: {}",
        String::from_utf8_lossy(&line_shifted.stderr)
    );
    assert_eq!(
        stable_mutant_ids(&stdout),
        stable_mutant_ids(
            &String::from_utf8(line_shifted.stdout)
                .expect("line-shifted mutation output must be UTF-8")
        ),
        "a line-only edit must not change stable mutant IDs"
    );

    let no_diffs = Command::new(command_path(&install))
        .arg("--no-diffs")
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start without diffs");
    assert!(
        no_diffs.status.success(),
        "no-diffs mutation run must succeed: {}",
        String::from_utf8_lossy(&no_diffs.stderr)
    );
    let no_diffs_stdout =
        String::from_utf8(no_diffs.stdout).expect("no-diffs mutation output must be UTF-8");
    assert!(
        no_diffs_stdout.contains("escaped ")
            && !no_diffs_stdout.contains("--- checked/src/lib.rs")
            && !no_diffs_stdout.contains("@@ -"),
        "--no-diffs must keep the escaped state and hide the source diff: {no_diffs_stdout}"
    );

    let failed_gate = Command::new(command_path(&install))
        .args(["--min-msi", "51"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a failing score gate");
    assert_eq!(
        failed_gate.status.code(),
        Some(4),
        "a failed total-score gate must return exit value 4: {}",
        String::from_utf8_lossy(&failed_gate.stderr)
    );

    let passed_gate = Command::new(command_path(&install))
        .args(["--min-msi", "50"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a passing score gate");
    assert!(
        passed_gate.status.success(),
        "a passed total-score gate must return exit value 0: {}",
        String::from_utf8_lossy(&passed_gate.stderr)
    );

    let selected_id = ids.first().expect("a mutant ID must exist");
    let one_mutant = Command::new(command_path(&install))
        .args(["--run-mutant-id", selected_id, "--min-msi", "100"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start for one mutant");
    assert!(
        one_mutant.status.success(),
        "one-mutant execution must ignore unrelated score gates: {}",
        String::from_utf8_lossy(&one_mutant.stderr)
    );
    let one_mutant_stdout =
        String::from_utf8(one_mutant.stdout).expect("one-mutant output must be UTF-8");
    assert!(
        one_mutant_stdout.matches("  ID: ").count() == 1
            && !one_mutant_stdout.contains("Killed:")
            && !one_mutant_stdout.contains("Mutation score:")
            && !one_mutant_stdout.contains("Per-mutator results:"),
        "one-mutant execution must show one evidence result without a summary or score gate: {one_mutant_stdout}"
    );
}

#[test]
fn installed_command_stops_when_baseline_tests_fail() {
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

    assert!(
        !output.status.success(),
        "a failed baseline must stop the mutation run"
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.is_empty(),
        "a failed baseline must not print mutant results: {stdout}"
    );
    let stderr = String::from_utf8(output.stderr).expect("mutation error output must be UTF-8");
    assert!(
        stderr.contains("clean cargo test failed") && stderr.contains("fails_before_mutation"),
        "the baseline failure must be reported: {stderr}"
    );
}

#[test]
fn installed_command_review_checks_clean_suite_without_mutants() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("other").join("src").join("lib.rs");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        !output.status.success(),
        "a failed clean suite must stop a source with no mutants"
    );
    assert!(
        output.stdout.is_empty(),
        "a failed clean suite must not print mutant results"
    );
    let stderr = String::from_utf8(output.stderr).expect("mutation error output must be UTF-8");
    assert!(
        stderr.contains("clean cargo test failed") && stderr.contains("is_unrelated_and_failing"),
        "the clean-suite failure must be clear: {stderr}"
    );
}

#[test]
fn installed_command_checks_each_selected_package_before_mutation() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let checked = fixture.join("checked").join("src").join("lib.rs");
    let other = fixture.join("other").join("src").join("lib.rs");
    fs::write(&other, "pub fn value() -> bool { true }\n")
        .expect("mutable source for the failing package must be written");

    let output = Command::new(command_path(&install))
        .args([&checked, &other])
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        !output.status.success(),
        "a selected package with a failed clean suite must stop the run"
    );
    assert!(
        output.stdout.is_empty(),
        "no mutant result can precede all clean package checks"
    );
    let stderr = String::from_utf8(output.stderr).expect("mutation error output must be UTF-8");
    assert!(
        stderr.contains("clean cargo test failed") && stderr.contains("is_unrelated_and_failing"),
        "the second package failure must be clear: {stderr}"
    );
}

#[test]
fn installed_command_skips_mutants_that_do_not_compile() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    fs::write(
        &source,
        "pub struct Marker<const ENABLED: bool>;\npub fn marker() -> Marker<true> { Marker::<true> }\n",
    )
    .expect("compile rejection source must be written");
    fs::write(
        fixture.join("checked").join("tests").join("mutation.rs"),
        "use mutation_checked::{marker, Marker};\n\n#[test]\nfn marker_is_enabled() {\n    let _: Marker<true> = marker();\n}\n",
    )
    .expect("compile rejection test must be written");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "mutation run must complete: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Killed: 0")
            && stdout.contains("Escaped: 0")
            && stdout.contains("Errored: 0")
            && stdout.contains("Skipped: 2"),
        "compiler-rejected mutants must be skipped: {stdout}"
    );
    assert!(
        stdout.matches("mutant did not compile").count() == 2,
        "each skipped result must explain the compile rejection: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_reports_a_failed_test_command_as_errored() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    fs::write(&source, "pub fn checked() -> bool { true }\n")
        .expect("single-mutant source must be written");
    let fake_cargo = root.join("vanishing-cargo");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\n\"$MUTARUST_REAL_CARGO\" \"$@\"\nstatus=$?\ncase \" $* \" in\n  *\" --no-run \"*) rm \"$0\" ;;\nesac\nexit $status\n",
    )
    .expect("temporary Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("temporary Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .output()
        .expect("installed mutarust must start");

    assert!(output.status.success(), "mutation run must complete");
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Killed: 0")
            && stdout.contains("Escaped: 0")
            && stdout.contains("Errored: 1")
            && stdout.contains("Skipped: 0")
            && stdout.contains("could not run cargo test"),
        "a command-start failure must be errored: {stdout}"
    );
}

#[test]
fn installed_command_preserves_existing_user_changes() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let tracked_test = fixture.join("checked").join("tests").join("mutation.rs");
    let untracked = fixture.join("local-notes.txt");
    run_git(&fixture, &["init"]);
    run_git(
        &fixture,
        &["config", "user.email", "mutarust@example.invalid"],
    );
    run_git(&fixture, &["config", "user.name", "Mutarust Test"]);
    run_git(&fixture, &["add", "."]);
    run_git(&fixture, &["commit", "-m", "fixture"]);
    fs::write(
        &source,
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\n// tracked local source change\n",
    )
    .expect("tracked source change must be written");
    fs::write(
        &tracked_test,
        "#[test]\nfn detects_checked_value() {\n    assert!(mutation_checked::checked());\n}\n// tracked local test change\n",
    )
    .expect("tracked test change must be written");
    fs::write(&untracked, "untracked user data\n").expect("untracked file must be written");
    let status_before = git_status(&fixture);
    let source_before = fs::read(&source).expect("changed source must be readable");
    let test_before = fs::read(&tracked_test).expect("changed test must be readable");
    let untracked_before = fs::read(&untracked).expect("untracked file must be readable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git_status(&fixture),
        status_before,
        "Git status must not change"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(fs::read(&tracked_test).unwrap(), test_before);
    assert_eq!(fs::read(&untracked).unwrap(), untracked_before);
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
fn installed_command_review_ignores_unrelated_cargo_configuration_data() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let configuration = fixture.join(".cargo");
    let unrelated = configuration.join("unrelated-cache");
    fs::create_dir_all(&configuration).expect("Cargo configuration directory must be created");
    fs::write(configuration.join("config.toml"), "[build]\n")
        .expect("Cargo configuration must be written");
    fs::write(&unrelated, vec![0_u8; 1024 * 1024]).expect("unrelated Cargo data must be written");
    let fake_cargo = root.join("cargo-copy-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif [ -e .cargo/unrelated-cache ]; then\n  echo 'unrelated Cargo data was copied' >&2\n  exit 91\nfi\nexec '{}' \"$@\"\n",
            env!("CARGO"),
            env!("CARGO")
        ),
    )
    .expect("Cargo copy check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("Cargo copy check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "unrelated Cargo data must not enter mutation workspaces: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_copies_recursive_cargo_configuration_includes() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let configuration = fixture.join(".cargo");
    fs::create_dir_all(configuration.join("nested"))
        .expect("nested Cargo configuration directory must be created");
    fs::write(
        configuration.join("config.toml"),
        "include = [\"shared.toml\"]\n",
    )
    .expect("Cargo configuration must be written");
    fs::write(
        configuration.join("shared.toml"),
        "include = [\"nested/settings.toml\"]\n",
    )
    .expect("included Cargo configuration must be written");
    fs::write(
        configuration.join("nested").join("settings.toml"),
        "[build]\n",
    )
    .expect("recursive Cargo configuration must be written");
    let fake_cargo = root.join("cargo-include-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif [ ! -f .cargo/shared.toml ] || [ ! -f .cargo/nested/settings.toml ]; then\n  echo 'Cargo configuration include was not copied' >&2\n  exit 92\nfi\nexec '{}' \"$@\"\n",
            env!("CARGO"),
            env!("CARGO")
        ),
    )
    .expect("Cargo include check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("Cargo include check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "recursive Cargo configuration includes must enter the isolated workspace: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_uses_only_the_active_cargo_configuration() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let configuration = fixture.join(".cargo");
    fs::create_dir_all(&configuration).expect("Cargo configuration directory must be created");
    fs::write(configuration.join("config"), "[build]\n")
        .expect("active Cargo configuration must be written");
    fs::write(configuration.join("config.toml"), "not valid TOML = [")
        .expect("inactive Cargo configuration must be written");
    let fake_cargo = root.join("cargo-active-configuration-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nexit 0\n",
            env!("CARGO")
        ),
    )
    .expect("Cargo configuration check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("Cargo configuration check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "an inactive Cargo configuration must not stop mutation testing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_isolates_cargo_home_configuration() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&fixture)
        .output()
        .expect("Cargo metadata must start");
    assert!(metadata.status.success(), "Cargo metadata must succeed");
    let metadata_path = root.join("cargo-home-metadata.json");
    fs::write(&metadata_path, metadata.stdout).expect("Cargo metadata must be recorded");
    let cargo_home = root.join("cargo-home");
    let included = root.join("cargo-home-shared.toml");
    let missing = root.join("missing-cargo-home-include.toml");
    let runner = root.join("cargo-home-runner");
    fs::create_dir_all(&cargo_home).expect("Cargo home must be created");
    fs::write(&included, "[build]\n").expect("included Cargo configuration must be written");
    fs::write(cargo_home.join("cache-sentinel"), b"cache")
        .expect("Cargo home cache sentinel must be written");
    fs::write(&runner, "runner").expect("Cargo home runner must be written");
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "include = [\"{}\", {{ path = \"{}\", optional = true }}]\n[target.'cfg(unix)']\nrunner = \"cargo-home-runner --flag\"\n",
            included.display(),
            missing.display()
        ),
    )
    .expect("Cargo home configuration must be written");
    let fake_cargo = root.join("cargo-home-isolation-check");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  cat \"$MUTARUST_METADATA\"\n  exit 0\nfi\nif [ \"$CARGO_HOME\" = \"$MUTARUST_ORIGINAL_CARGO_HOME\" ]; then\n  echo 'Cargo used the original Cargo home' >&2\n  exit 93\nfi\nif grep -F -q \"$MUTARUST_ORIGINAL_INCLUDE\" \"$CARGO_HOME/config.toml\"; then\n  echo 'Cargo configuration kept an absolute source include' >&2\n  exit 94\nfi\nif grep -F -q \"$MUTARUST_MISSING_INCLUDE\" \"$CARGO_HOME/config.toml\"; then\n  echo 'Cargo configuration kept an external optional include' >&2\n  exit 95\nfi\ncopied_include=$(sed -n 's/.*\"\\([^\"]*cargo-home-shared.toml\\)\".*/\\1/p' \"$CARGO_HOME/config.toml\")\nif [ ! -f \"$copied_include\" ]; then\n  echo 'copied Cargo configuration include does not exist' >&2\n  exit 96\nfi\nif [ -e \"$CARGO_HOME/cache-sentinel\" ]; then\n  echo 'unrelated Cargo home data entered the isolated workspace' >&2\n  exit 97\nfi\nif [ ! -f \"$CARGO_HOME/../cargo-home-runner\" ]; then\n  echo 'relative Cargo runner was not copied' >&2\n  exit 98\nfi\nexit 0\n",
    )
    .expect("Cargo home isolation check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("Cargo home isolation check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("CARGO_HOME", &cargo_home)
        .env("MUTARUST_ORIGINAL_CARGO_HOME", &cargo_home)
        .env("MUTARUST_ORIGINAL_INCLUDE", &included)
        .env("MUTARUST_MISSING_INCLUDE", &missing)
        .env("MUTARUST_METADATA", &metadata_path)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "Cargo home configuration must be isolated: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_isolates_cargo_configuration_patch_paths() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let patch = root.join("patch-support");
    fs::create_dir_all(patch.join("src")).expect("patch source directory must be created");
    fs::write(
        patch.join("Cargo.toml"),
        "[package]\nname = \"patch-support\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("patch manifest must be written");
    fs::write(
        patch.join("src").join("lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .expect("patch source must be written");
    let configuration = fixture.join(".cargo");
    fs::create_dir_all(&configuration).expect("Cargo configuration directory must be created");
    fs::write(
        configuration.join("config.toml"),
        "[patch.crates-io]\npatch-support = { path = \"../../patch-support\" }\n",
    )
    .expect("Cargo patch configuration must be written");
    let fake_cargo = root.join("cargo-patch-isolation-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\npatch_value=$(sed -n 's/.*path = \"\\([^\"]*\\)\".*/\\1/p' .cargo/config.toml)\ncopied_patch=$(cd .cargo/\"$patch_value\" && pwd)\nif [ \"$copied_patch\" = \"$MUTARUST_ORIGINAL_PATCH\" ]; then\n  echo 'Cargo patch kept its original path' >&2\n  exit 91\nfi\nif [ ! -f \"$copied_patch/src/lib.rs\" ]; then\n  echo 'Cargo patch source was not copied' >&2\n  exit 92\nfi\necho isolated > \"$copied_patch/marker\"\nexit 0\n",
            env!("CARGO")
        ),
    )
    .expect("Cargo patch isolation check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("Cargo patch isolation check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_ORIGINAL_PATCH", &patch)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "Cargo patch path must be isolated: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !patch.join("marker").exists(),
        "the original Cargo patch must stay unchanged"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_copies_a_source_directory_named_target() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let package = fixture.join("checked");
    let target_module = package.join("src").join("target");
    fs::create_dir_all(&target_module).expect("target module directory must be created");
    fs::create_dir_all(package.join(".cargo"))
        .expect("nested Cargo data directory must be created");
    fs::write(package.join(".cargo").join("cache"), b"cache")
        .expect("nested Cargo data must be written");
    fs::write(package.join(".git"), "gitdir: user-worktree\n")
        .expect("nested Git metadata must be written");
    fs::write(
        package.join("src").join("lib.rs"),
        "mod target;\npub use target::checked;\n",
    )
    .expect("target module root must be written");
    let source = target_module.join("mod.rs");
    fs::write(&source, "pub fn checked() -> bool { true }\n")
        .expect("target module source must be written");
    let fake_cargo = root.join("cargo-target-module-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif [ ! -f checked/src/target/mod.rs ]; then\n  echo 'source target module was not copied' >&2\n  exit 96\nfi\nif [ -e checked/.cargo ] || [ -e checked/.git ]; then\n  echo 'nested Cargo or Git metadata was copied' >&2\n  exit 99\nfi\nexit 0\n",
            env!("CARGO")
        ),
    )
    .expect("target module check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("target module check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "a source directory named target must be copied: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Escaped: 1") && stdout.contains("Errored: 0"),
        "the target module mutant must run: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_review_excludes_local_dependency_build_data() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_external_mutation_fixture(&root);
    let source = fixture.join("external").join("lib.rs");
    let project = fixture.join("project");
    let dependency_target = fixture.join("support").join("target");
    fs::create_dir_all(&dependency_target)
        .expect("local dependency target directory must be created");
    fs::write(
        dependency_target.join("large-artifact"),
        vec![0_u8; 1024 * 1024],
    )
    .expect("local dependency build data must be written");
    let fake_cargo = root.join("cargo-dependency-target-check");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif [ -e ../support/target ]; then\n  echo 'local dependency build data was copied' >&2\n  exit 100\nfi\nexit 0\n",
            env!("CARGO")
        ),
    )
    .expect("local dependency target check must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("local dependency target check must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&project)
        .env("CARGO", &fake_cargo)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "local dependency build data must not be copied: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn installed_command_review_ignores_unrelated_manifests() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let unrelated = fixture.join("checked").join("fixtures");
    fs::create_dir_all(&unrelated).expect("unrelated fixture directory must be created");
    fs::write(
        unrelated.join("Cargo.toml"),
        "this is not a Cargo manifest\n",
    )
    .expect("unrelated manifest data must be written");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "an unrelated manifest must not stop mutation testing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn installed_command_rewrites_an_absolute_cargo_target_path() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_external_mutation_fixture(&root);
    let source = fixture.join("external").join("lib.rs");
    let project = fixture.join("project");
    let manifest = project.join("application").join("Cargo.toml");
    let text = fs::read_to_string(&manifest).expect("application manifest must be readable");
    fs::write(
        &manifest,
        text.replace(
            "path = \"../../external/lib.rs\"",
            &format!("path = \"{}\"", source.display()),
        ),
    )
    .expect("absolute target manifest must be written");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&project)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "absolute Cargo target mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Killed: 1") && stdout.contains("Escaped: 1"),
        "Cargo must test the copied absolute target: {stdout}"
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
fn installed_command_tests_one_mutation_per_temporary_workspace() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let fake_cargo = root.join("recording-cargo");
    let record = root.join("mutant-tests");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\ncase \" $* \" in\n  *\" --no-run \"*) exec \"$MUTARUST_REAL_CARGO\" \"$@\" ;;\nesac\nif grep -q false checked/src/lib.rs 2>/dev/null; then\n  checked=true\n  unchecked=true\n  grep -q 'pub fn checked() -> bool { false }' checked/src/lib.rs && checked=false\n  grep -q 'pub fn unchecked() -> bool { false }' checked/src/lib.rs && unchecked=false\n  mode=$(stat -c %a \"$PWD\" 2>/dev/null || stat -f %Lp \"$PWD\")\n  printf '%s|%s|%s|%s\\n' \"$PWD\" \"$checked\" \"$unchecked\" \"$mode\" >> \"$MUTARUST_TEST_RECORD\"\nfi\nexec \"$MUTARUST_REAL_CARGO\" \"$@\"\n",
    )
    .expect("recording Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("recording Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_TEST_RECORD", &record)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "recorded mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let records = fs::read_to_string(record).expect("mutant test record must be readable");
    let records = records
        .lines()
        .map(|line| line.split('|').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2, "exactly two mutant tests must run");
    assert_ne!(records[0][0], records[1][0], "mutants need separate copies");
    let mutations = records
        .iter()
        .map(|record| (record[1], record[2]))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        mutations,
        [("false", "true"), ("true", "false")].into(),
        "each test workspace must contain exactly one mutation"
    );
    for record in records {
        assert_eq!(record[3], "700", "temporary workspaces must be private");
        assert!(
            !Path::new(record[0]).exists(),
            "each temporary workspace must be removed"
        );
    }
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
    let temporary_root = root.join("mutation-temporary");
    fs::create_dir(&temporary_root).expect("mutation temporary root must be created");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif ! grep -q false checked/src/lib.rs 2>/dev/null; then\n  exit 0\nfi\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\nsleep 30 &\necho $! > \"$MUTARUST_TIMED_OUT_CHILD\"\nwait\n",
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
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "timeout result must not stop the run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "each timed out test must stop promptly; elapsed: {elapsed:?}"
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
    assert!(
        process_has_stopped(&child),
        "the timeout must stop the test child process"
    );
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "the timeout must remove each mutation workspace"
    );
}

#[cfg(unix)]
fn process_has_stopped(identifier: &str) -> bool {
    let output = Command::new("ps")
        .args(["-o", "stat=", "-p", identifier])
        .output()
        .expect("process state check must start");
    let state = String::from_utf8_lossy(&output.stdout);
    state.trim().is_empty() || state.trim_start().starts_with('Z')
}

#[cfg(unix)]
#[test]
fn installed_command_stops_test_at_interrupt() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let tracked_test = fixture.join("checked").join("tests").join("mutation.rs");
    let untracked = fixture.join("interrupt-notes.txt");
    run_git(&fixture, &["init"]);
    run_git(
        &fixture,
        &["config", "user.email", "mutarust@example.invalid"],
    );
    run_git(&fixture, &["config", "user.name", "Mutarust Test"]);
    run_git(&fixture, &["add", "."]);
    run_git(&fixture, &["commit", "-m", "fixture"]);
    fs::write(
        &source,
        "pub fn checked() -> bool { true }\npub fn unchecked() -> bool { true }\n// interrupted source change\n",
    )
    .expect("interrupted source change must be written");
    fs::write(
        &tracked_test,
        "#[test]\nfn detects_checked_value() {\n    assert!(mutation_checked::checked());\n}\n// interrupted test change\n",
    )
    .expect("interrupted test change must be written");
    fs::write(&untracked, "untracked interrupt data\n")
        .expect("untracked interrupt data must be written");
    let status_before = git_status(&fixture);
    let source_before = fs::read(&source).expect("interrupted source must be readable");
    let test_before = fs::read(&tracked_test).expect("interrupted test must be readable");
    let untracked_before = fs::read(&untracked).expect("interrupt data must be readable");
    let fake_cargo = root.join("interrupted-cargo");
    let cargo_identifier = root.join("interrupted-cargo-identifier");
    let temporary_root = root.join("mutation-temporary");
    fs::create_dir(&temporary_root).expect("mutation temporary root must be created");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif ! grep -q false checked/src/lib.rs 2>/dev/null; then\n  exit 0\nfi\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\necho $$ > \"$MUTARUST_INTERRUPTED_CARGO\"\nsleep 30\n",
            env!("CARGO")
        ),
    )
    .expect("fake cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("fake cargo command must be executable");

    let mut command = Command::new(command_path(&install));
    let process = command
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_INTERRUPTED_CARGO", &cargo_identifier)
        .env("TMPDIR", &temporary_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("installed mutarust must start");
    wait_for_file(&cargo_identifier, "interrupted Cargo process must start");
    let signal = Command::new("kill")
        .args(["-INT", &process.id().to_string()])
        .status()
        .expect("interrupt command must start");
    assert!(signal.success(), "interrupt command must succeed");

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(process.wait_with_output());
    });
    let output = receiver
        .recv_timeout(std::time::Duration::from_secs(4))
        .expect("interrupted mutarust must stop promptly")
        .expect("installed mutarust must stop");
    assert!(
        !output.status.success(),
        "an interrupted mutation run must fail"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("mutation run interrupted"),
        "the interrupt diagnostic must be clear"
    );
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
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "the interrupt must remove each mutation workspace"
    );
    assert_eq!(git_status(&fixture), status_before);
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(fs::read(&tracked_test).unwrap(), test_before);
    assert_eq!(fs::read(&untracked).unwrap(), untracked_before);
}

#[cfg(windows)]
#[test]
fn installed_command_stops_test_at_interrupt() {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
    use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let test = fixture.join("checked").join("tests").join("mutation.rs");
    let marker = root.join("windows-interrupt-workspace");
    fs::write(&source, "pub fn checked() -> bool { true }\n")
        .expect("interrupt source must be written");
    fs::write(
        &test,
        "#[test]\nfn blocks_on_the_mutant() {\n    if !mutation_checked::checked() {\n        std::fs::write(std::env::var_os(\"MUTARUST_INTERRUPT_MARKER\").unwrap(), env!(\"CARGO_MANIFEST_DIR\")).unwrap();\n        std::thread::sleep(std::time::Duration::from_secs(30));\n    }\n}\n",
    )
    .expect("interrupt test must be written");
    let source_before = fs::read(&source).expect("interrupt source must be readable");
    let test_before = fs::read(&test).expect("interrupt test must be readable");

    let mut command = Command::new(command_path(&install));
    let process = command
        .arg(&source)
        .current_dir(&fixture)
        .env("MUTARUST_INTERRUPT_MARKER", &marker)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("installed mutarust must start");
    wait_for_file(&marker, "interrupted Cargo test must start");
    let generated = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, process.id()) };
    assert_ne!(generated, 0, "console interrupt must be generated");

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(process.wait_with_output());
    });
    let output = receiver
        .recv_timeout(std::time::Duration::from_secs(4))
        .expect("interrupted mutarust must stop promptly")
        .expect("installed mutarust must stop");
    assert!(
        !output.status.success(),
        "an interrupted mutation run must fail"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("mutation run interrupted"),
        "the interrupt diagnostic must be clear"
    );
    let workspace = PathBuf::from(
        fs::read_to_string(&marker).expect("mutation workspace marker must be readable"),
    );
    assert!(
        !workspace.exists(),
        "the interrupt must remove the mutation workspace"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(fs::read(&test).unwrap(), test_before);
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

fn install_command(_root: &Path) -> PathBuf {
    INSTALL_CLEANUP_REGISTERED.get_or_init(|| {
        let registered = unsafe { libc::atexit(clean_installed_command) };
        assert_eq!(registered, 0, "installed command cleanup must register");
    });
    INSTALLED_COMMAND
        .get_or_init(|| {
            let root = installed_command_root();
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
        })
        .clone()
}

extern "C" fn clean_installed_command() {
    let _ = fs::remove_dir_all(installed_command_root());
}

fn installed_command_root() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("installed-command-{}", std::process::id()))
}

fn run_git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("Git command must start");
    assert!(
        output.status.success(),
        "Git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_status(directory: &Path) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(directory)
        .output()
        .expect("Git status must start");
    assert!(output.status.success(), "Git status must succeed");
    String::from_utf8(output.stdout).expect("Git status must be UTF-8")
}

fn stable_mutant_ids(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("  ID: "))
        .map(str::to_owned)
        .collect()
}

#[cfg(unix)]
fn mutarust_temp_entries(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("mutation temporary root must be readable")
        .map(|entry| entry.expect("temporary entry must be readable").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("mutarust-"))
        })
        .collect()
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

fn configuration_error(
    install: &Path,
    fixture: &Path,
    source: &Path,
    configuration: &Path,
) -> String {
    let output = Command::new(command_path(install))
        .args(["--config"])
        .arg(configuration)
        .arg(source)
        .current_dir(fixture)
        .output()
        .expect("installed mutarust must start with invalid configuration");

    assert!(
        !output.status.success(),
        "invalid configuration {} must fail: stdout: {}; stderr: {}",
        configuration.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stderr)
        .expect("configuration error output must be UTF-8")
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
