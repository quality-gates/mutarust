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

fn wait_for_file_lines(path: &Path, count: usize, message: &str) {
    for _ in 0..1_000 {
        let lines = fs::read_to_string(path)
            .map(|content| content.lines().count())
            .unwrap_or(0);
        if lines >= count {
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
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("--git-diff-base REF  Set Git base; default origin/HEAD, then master"),
        "help must document the Git default and fallback"
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
fn installed_command_filters_mutator_and_source_candidates() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let excluded = fixture
        .join("checked")
        .join("src")
        .join("excluded")
        .join("mod.rs");
    fs::create_dir_all(
        excluded
            .parent()
            .expect("excluded source must have a parent directory"),
    )
    .expect("excluded source directory must be created");
    fs::write(&excluded, "pub fn excluded() -> bool { true }\n")
        .expect("excluded source must be written");
    fs::write(
        &source,
        "pub fn checked() -> bool { true }\npub fn ignored_by_line() -> bool { true }\n",
    )
    .expect("source filtering fixture must be written");
    let configuration = fixture.join("filter.yml");
    fs::write(
        &configuration,
        "enable_mutators:\n  - conditional/*\nexclude_dirs:\n  - checked/src/excluded\nignore_source_lines:\n  - ignored_by_line\n",
    )
    .expect("source filter configuration must be written");

    let filtered = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args([&source, &excluded])
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with source filters");
    assert!(
        filtered.status.success(),
        "source filters must succeed: {}",
        String::from_utf8_lossy(&filtered.stderr)
    );
    let filtered_output =
        String::from_utf8(filtered.stdout).expect("source filter output must be UTF-8");
    assert!(
        filtered_output.contains("Killed: 1") && filtered_output.contains("Total: 1"),
        "the directory and source-line filters must remove candidates: {filtered_output}"
    );

    let external_directory = root.join("excluded-external");
    fs::create_dir_all(&external_directory).expect("external source directory must be created");
    let external_source = external_directory.join("generated.rs");
    fs::write(
        &external_source,
        "pub fn excluded_external() -> bool { true }\n",
    )
    .expect("external source must be written");
    let external_configuration = fixture.join("external-filter.yml");
    fs::write(
        &external_configuration,
        format!("exclude_dirs:\n  - {}\n", external_directory.display()),
    )
    .expect("external source configuration must be written");
    let external_filtered = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&external_configuration)
        .arg(&external_source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must skip an excluded external source");
    assert!(
        external_filtered.status.success(),
        "an excluded external source must not require a Cargo workspace: {}",
        String::from_utf8_lossy(&external_filtered.stderr)
    );
    assert!(
        String::from_utf8_lossy(&external_filtered.stdout).contains("Total: 0"),
        "an excluded external source must have no mutation candidates"
    );

    let matched = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args(["--match", "^checked$"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a function filter");
    assert!(
        matched.status.success(),
        "a valid function filter must succeed: {}",
        String::from_utf8_lossy(&matched.stderr)
    );
    assert!(
        String::from_utf8_lossy(&matched.stdout).contains("Total: 1"),
        "the function filter must select only the matching function"
    );

    let nested = fixture.join("checked").join("src").join("nested.rs");
    fs::write(
        &nested,
        "pub fn outer() -> bool {\n    fn inner() -> bool { true }\n    true\n}\n",
    )
    .expect("nested function source must be written");
    let nested_match = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args(["--match", "^outer$"])
        .arg(&nested)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a nested function filter");
    assert!(
        nested_match.status.success(),
        "a nested function filter must succeed: {}",
        String::from_utf8_lossy(&nested_match.stderr)
    );
    assert!(
        String::from_utf8_lossy(&nested_match.stdout).contains("Total: 1"),
        "a function filter must not select a nested function with another name"
    );

    let disabled_configuration = fixture.join("disabled-filter.yml");
    fs::write(
        &disabled_configuration,
        "enable_mutators:\n  - conditional/*\ndisable_mutators:\n  - conditional/bool-literal\n",
    )
    .expect("disabled filter configuration must be written");
    let configured_denylist = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&disabled_configuration)
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a configuration denylist");
    assert!(configured_denylist.status.success());
    assert!(
        String::from_utf8_lossy(&configured_denylist.stdout).contains("Total: 0"),
        "a configuration denylist must remove an allowed mutator"
    );
    let command_denylist = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .args(["--disable", "conditional/*"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with a command denylist");
    assert!(command_denylist.status.success());
    assert!(
        String::from_utf8_lossy(&command_denylist.stdout).contains("Total: 0"),
        "a command denylist must remove an allowed mutator"
    );

    let annotations = fixture.join("checked").join("src").join("annotations.rs");
    fs::write(
        &annotations,
        "pub fn allowed() -> bool { true }\n\n// mutator-disable-func\npub fn function_all() -> bool { true }\n\n// mutator-disable-func conditional/bool-literal\n#[inline]\npub fn function_selected() -> bool { true }\n\n// mutator-disable-next-line\npub fn next_line_all() -> bool { true }\n\n// mutator-disable-next-line conditional/bool-literal\npub fn next_line_selected() -> bool { true }\n\n// mutator-disable-regexp regexp_all\npub fn regexp_all() -> bool { true }\n\n// mutator-disable-regexp regexp_selected conditional/bool-literal\npub fn regexp_selected() -> bool { true }\n",
    )
    .expect("annotation fixture must be written");
    let annotations_elsewhere = fixture
        .join("checked")
        .join("src")
        .join("annotations_elsewhere.rs");
    fs::write(
        &annotations_elsewhere,
        "pub fn regexp_all_elsewhere() -> bool { true }\n",
    )
    .expect("second annotation fixture must be written");
    let annotated = Command::new(command_path(&install))
        .args(["--config"])
        .arg(&configuration)
        .arg(&annotations)
        .arg(&annotations_elsewhere)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start with annotations");
    assert!(
        annotated.status.success(),
        "valid annotations must succeed: {}",
        String::from_utf8_lossy(&annotated.stderr)
    );
    assert!(
        String::from_utf8_lossy(&annotated.stdout).contains("Total: 2"),
        "the three annotation forms must remove marked candidates only in their file"
    );

    fs::write(
        &annotations,
        "// mutator-disable-next-line unknown/mutator\npub fn invalid_annotation() -> bool { true }\n",
    )
    .expect("invalid annotation fixture must be written");
    let invalid_annotation = Command::new(command_path(&install))
        .arg(&annotations)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject an invalid annotation");
    assert_eq!(
        invalid_annotation.status.code(),
        Some(3),
        "an invalid annotation must return the source error value"
    );
    assert!(
        String::from_utf8_lossy(&invalid_annotation.stderr).contains("unknown annotation mutator"),
        "an invalid annotation must identify the bad mutator"
    );

    for (contents, expected) in [
        (
            "// mutator-disable-regexp ( *\npub fn invalid_annotation() -> bool { true }\n",
            "invalid annotation regular expression",
        ),
        (
            "// mutator-disable-func\n\npub fn invalid_annotation() -> bool { true }\n",
            "function annotation must be directly before a function",
        ),
    ] {
        fs::write(&annotations, contents).expect("invalid annotation fixture must be written");
        let invalid_annotation = Command::new(command_path(&install))
            .arg(&annotations)
            .current_dir(&fixture)
            .output()
            .expect("installed mutarust must reject an invalid annotation");
        assert_eq!(invalid_annotation.status.code(), Some(3));
        assert!(
            String::from_utf8_lossy(&invalid_annotation.stderr).contains(expected),
            "an invalid annotation must have a clear diagnostic"
        );
    }

    let invalid_match = Command::new(command_path(&install))
        .args(["--match", "("])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject an invalid function filter");
    assert_eq!(
        invalid_match.status.code(),
        Some(3),
        "an invalid function filter must return the source error value"
    );
    assert!(
        String::from_utf8_lossy(&invalid_match.stderr)
            .contains("invalid --match regular expression"),
        "an invalid function filter must have a clear diagnostic"
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

    let sequential = Command::new(command_path(&install))
        .args(["--workers", "1"])
        .arg(&source)
        .current_dir(&*root)
        .env("CARGO_TARGET_DIR", &user_target)
        .output()
        .expect("installed mutarust must start");
    let parallel = Command::new(command_path(&install))
        .args(["--workers", "2"])
        .arg(&source)
        .current_dir(&*root)
        .env("CARGO_TARGET_DIR", &user_target)
        .output()
        .expect("installed mutarust must start");

    assert!(
        sequential.status.success(),
        "sequential mutation run must succeed"
    );
    assert!(
        parallel.status.success(),
        "parallel mutation run must succeed"
    );
    let sequential_stdout =
        String::from_utf8(sequential.stdout).expect("sequential mutation output must be UTF-8");
    let parallel_stdout =
        String::from_utf8(parallel.stdout).expect("parallel mutation output must be UTF-8");
    assert!(
        parallel_stdout.contains("killed ") && parallel_stdout.contains("escaped "),
        "one mutant must be killed and one must escape: {parallel_stdout}"
    );
    assert!(
        parallel_stdout.contains("Killed: 1") && parallel_stdout.contains("Escaped: 1"),
        "the final counts must use mutation result terms: {parallel_stdout}"
    );
    assert_eq!(
        parallel_stdout, sequential_stdout,
        "parallel workers must keep mutation result records and summaries in plan order"
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

#[cfg(unix)]
#[test]
fn installed_command_keeps_parallel_cargo_output_out_of_result_records() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("source must be readable");
    let fake_cargo = root.join("parallel-output-cargo");
    let markers = root.join("parallel-output-markers");
    let temporary_root = root.join("parallel-output-temporary");
    fs::create_dir(&markers).expect("parallel output markers must be created");
    fs::create_dir(&temporary_root).expect("parallel output root must be created");
    fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif ! grep -q false checked/src/lib.rs 2>/dev/null; then\n  exit 0\nfi\nif [ \"$(grep -c false checked/src/lib.rs)\" -ne 1 ]; then\n  exit 91\nfi\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\ntouch \"$MUTARUST_OUTPUT_MARKERS/$$\"\nfor _ in $(seq 1 100); do\n  [ \"$(find \"$MUTARUST_OUTPUT_MARKERS\" -type f | wc -l)\" -ge 2 ] && break\n  sleep 0.01\ndone\nif [ \"$(find \"$MUTARUST_OUTPUT_MARKERS\" -type f | wc -l)\" -ne 2 ]; then\n  exit 92\nfi\nprintf 'parallel worker output: begin'\nsleep 0.05\nprintf ': complete\\n'\nexit 1\n",
            env!("CARGO")
        ),
    )
    .expect("parallel output Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("parallel output Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .args(["--workers", "2"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_OUTPUT_MARKERS", &markers)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start");

    assert!(
        output.status.success(),
        "parallel Cargo run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("mutation output must be UTF-8");
    assert!(
        stdout.contains("Killed: 2"),
        "both concurrent Cargo workers must report complete results: {stdout}"
    );
    assert!(
        !stdout.contains("parallel worker output"),
        "concurrent Cargo output must not mix with mutation result records: {stdout}"
    );
    assert_eq!(
        fs::read_dir(&markers)
            .expect("parallel output markers must be readable")
            .count(),
        2,
        "two Cargo workers must run at the same time"
    );
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "parallel workspaces must be removed after the run"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
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
fn installed_command_dry_run_does_not_write_or_test() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("source must be readable");
    let temporary_root = root.join("dry-run-temporary");
    let fake_cargo = root.join("dry-run-cargo");
    fs::create_dir(&temporary_root).expect("temporary root must be created");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nprintf 'cargo test ran\\n' > \"$MUTARUST_CARGO_RECORD\"\nexit 1\n",
    )
    .expect("dry-run Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("dry-run Cargo command must be executable");
    let record = root.join("dry-run-cargo-record");

    let output = Command::new(command_path(&install))
        .args(["--dry-run"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_CARGO_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start dry run");

    assert!(
        output.status.success(),
        "dry run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("dry-run output must be UTF-8"),
        "Total: 2 mutation(s) would be generated. No files written, no tests run.\n"
    );
    assert!(!record.exists(), "dry run must not start Cargo tests");
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "dry run must not create mutation workspaces"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_no_exec_keeps_generated_mutants() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("source must be readable");
    let temporary_root = root.join("no-exec-temporary");
    let fake_cargo = root.join("no-exec-cargo");
    fs::create_dir(&temporary_root).expect("temporary root must be created");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nprintf 'cargo test ran\\n' > \"$MUTARUST_CARGO_RECORD\"\nexit 1\n",
    )
    .expect("no-exec Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("no-exec Cargo command must be executable");
    let record = root.join("no-exec-cargo-record");

    let output = Command::new(command_path(&install))
        .args(["--no-exec"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_CARGO_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start no-exec run");

    assert!(
        output.status.success(),
        "no-exec run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("no-exec output must be UTF-8");
    assert!(
        stdout.contains("Generated: 2") && stdout.contains("mutation area:"),
        "no-exec output must report generated areas: {stdout}"
    );
    assert!(!record.exists(), "no-exec must not start Cargo tests");
    assert_eq!(fs::read(&source).unwrap(), source_before);
    let entries = mutarust_temp_entries(&temporary_root);
    assert_eq!(
        entries.len(),
        2,
        "no-exec must keep each mutation workspace"
    );
    assert!(
        entries
            .iter()
            .all(|path| path.join("checked/src/lib.rs").is_file()),
        "each generated mutation area must contain the selected source"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_keeps_requested_mutation_workspaces() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("source must be readable");
    let temporary_root = root.join("keep-temporary-root");
    fs::create_dir(&temporary_root).expect("temporary root must be created");

    let output = Command::new(command_path(&install))
        .args(["--do-not-remove-tmp-folder"])
        .arg(&source)
        .current_dir(&fixture)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start retained run");

    assert!(
        output.status.success(),
        "retained run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("mutation area:"),
        "retained run must report mutation areas"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(
        mutarust_temp_entries(&temporary_root).len(),
        2,
        "retained run must keep only mutation workspaces"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_reports_retained_area_after_a_test_command_error() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let temporary_root = root.join("retained-error-temporary");
    let fake_cargo = root.join("retained-error-cargo");
    fs::create_dir(&temporary_root).expect("temporary root must be created");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\ncase \" $* \" in\n  *\" --no-run \"*) chmod 000 \"$0\"; exit 0 ;;\nesac\nexit 0\n",
    )
    .expect("retained-error Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("retained-error Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .args(["--do-not-remove-tmp-folder"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start retained error run");

    assert!(
        output.status.success(),
        "retained error run must complete: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("retained output must be UTF-8");
    assert!(
        stdout.contains("Errored: 2") && stdout.contains("mutation area:"),
        "retained errors must report mutation areas: {stdout}"
    );
    assert_eq!(
        mutarust_temp_entries(&temporary_root).len(),
        2,
        "retained error run must preserve mutation workspaces"
    );
}

#[cfg(unix)]
#[test]
fn installed_command_applies_adaptive_cargo_controls() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let fake_cargo = root.join("adaptive-cargo");
    let record = root.join("adaptive-cargo-record");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nprintf '%s\\n' \"$*\" >> \"$MUTARUST_CARGO_RECORD\"\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\nif grep -q false checked/src/lib.rs; then\n  sleep 3\nelse\n  sleep 1\nfi\n",
    )
    .expect("adaptive Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("adaptive Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .args([
            "--timeout-coefficient",
            "1.5",
            "--test-flags",
            "--package mutation-checked",
            "--test-recursive",
        ])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_CARGO_RECORD", &record)
        .output()
        .expect("installed mutarust must start adaptive run");

    assert!(
        output.status.success(),
        "adaptive run must complete: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("adaptive output must be UTF-8");
    assert!(
        stdout.contains("Errored: 2") && stdout.contains("timed out after 2 seconds"),
        "adaptive timeout must use the clean duration: {stdout}"
    );
    let record = fs::read_to_string(record).expect("Cargo record must be readable");
    assert!(
        record.lines().all(|line| {
            line.contains("--workspace") && line.contains("--package mutation-checked")
        }),
        "Cargo controls must apply to each Cargo command: {record}"
    );
}

#[test]
fn installed_command_tests_workspace_packages_recursively() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");

    let output = Command::new(command_path(&install))
        .args(["--test-recursive"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start recursive test run");

    assert_eq!(
        output.status.code(),
        Some(3),
        "the failing workspace package must fail the clean test run"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("clean cargo test failed"),
        "recursive test selection must include the full workspace"
    );
}

#[test]
fn installed_command_rejects_incompatible_execution_controls() {
    let root = smoke_root();
    let install = install_command(&root);
    for (arguments, error) in [
        (
            vec!["--dry-run", "--no-exec"],
            "--dry-run and --no-exec cannot be used together",
        ),
        (
            vec!["--no-exec", "--exec", "true"],
            "--no-exec cannot be used with --exec",
        ),
        (
            vec!["--timeout", "1", "--timeout-coefficient", "1.5"],
            "--timeout-coefficient cannot be used with --timeout",
        ),
        (
            vec!["--exec", "true", "--timeout-coefficient", "1.5"],
            "--timeout-coefficient requires the Cargo test command",
        ),
        (
            vec!["--dry-run", "--test-recursive"],
            "--dry-run cannot be used with --test-recursive",
        ),
        (
            vec!["--dry-run", "--do-not-remove-tmp-folder"],
            "--dry-run cannot be used with --do-not-remove-tmp-folder",
        ),
        (
            vec!["--no-exec", "--timeout", "1"],
            "--no-exec cannot be used with --timeout",
        ),
        (
            vec!["--workers", "0"],
            "--workers requires a positive whole number",
        ),
        (
            vec!["--workers", "two"],
            "--workers requires a positive whole number",
        ),
        (
            vec!["--workers", "1", "--workers", "2"],
            "--workers can be supplied only once",
        ),
        (
            vec!["--dry-run", "--workers", "2"],
            "--dry-run cannot be used with --workers",
        ),
        (
            vec!["--dry-run", "--coverage"],
            "--dry-run cannot be used with --coverage",
        ),
        (
            vec!["--no-exec", "--per-test"],
            "--per-test cannot be used with --no-exec",
        ),
        (
            vec!["--exec", "true", "--coverage"],
            "--coverage requires the Cargo test command",
        ),
    ] {
        let output = Command::new(command_path(&install))
            .args(arguments)
            .output()
            .expect("installed mutarust must reject invalid controls");
        assert_eq!(output.status.code(), Some(3));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(error),
            "control error must explain the invalid combination"
        );
    }
}

#[cfg(unix)]
#[test]
fn installed_command_collects_coverage_and_selects_covering_tests() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_coverage_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("coverage fixture source must be readable");
    let fake_cargo = root.join("coverage-cargo");
    let record = root.join("coverage-test-record");
    let temporary_root = root.join("coverage-temporary");
    fs::create_dir(&temporary_root).expect("coverage temporary root must be created");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nif [ \"$1\" = \"llvm-cov\" ]; then\n  output=\n  while [ \"$#\" -gt 0 ]; do\n    if [ \"$1\" = \"--output-path\" ]; then\n      output=$2\n      break\n    fi\n    shift\n  done\n  if [ \"$MUTARUST_COVERAGE_MODE\" = \"missing\" ]; then\n    exit 0\n  fi\n  if [ \"$MUTARUST_COVERAGE_MODE\" = \"invalid\" ]; then\n    printf 'SF:%s\\nDA:zero,one\\nend_of_record\\n' \"$MUTARUST_COVERAGE_SOURCE\" > \"$output\"\n    exit 0\n  fi\n  case \" $* \" in\n    *\" --exact detects_detected \"*) data='DA:1,1' ;;\n    *\" --exact detects_escaped \"*) data='DA:2,1' ;;\n    *) data='DA:1,1\\nDA:2,1\\nDA:3,0' ;;\n  esac\n  printf 'SF:%s\\n%b\\nend_of_record\\n' \"$MUTARUST_COVERAGE_SOURCE\" \"$data\" > \"$output\"\n  exit 0\nfi\ncase \" $* \" in\n  *\" --list \"*) printf 'detects_detected: test\\ndetects_escaped: test\\n'; exit 0 ;;\nesac\nif ! grep -q false checked/src/lib.rs 2>/dev/null; then\n  exit 0\nfi\nif grep -q 'pub fn detected() -> bool { false }' checked/src/lib.rs; then\n  mutant=detected\nelif grep -q 'pub fn escaped() -> bool { false }' checked/src/lib.rs; then\n  mutant=escaped\nelif grep -q 'pub fn uncovered() -> bool { false }' checked/src/lib.rs; then\n  mutant=uncovered\nelse\n  exit 94\nfi\nprintf '%s|%s\\n' \"$mutant\" \"$*\" >> \"$MUTARUST_COVERAGE_RECORD\"\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\n  *\" --exact detects_detected \"*) exit 1 ;;\n  *\" --exact detects_escaped \"*) exit 0 ;;\n  *) exit 93 ;;\nesac\n",
    )
    .expect("coverage Cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("coverage Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .args(["--coverage", "--per-test", "--workers", "1"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_COVERAGE_SOURCE", &source)
        .env("MUTARUST_COVERAGE_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must collect coverage");

    assert!(
        output.status.success(),
        "coverage mutation run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("coverage output must be UTF-8");
    assert!(
        stdout.contains("Killed: 1")
            && stdout.contains("Escaped: 1")
            && stdout.contains("Not covered: 1")
            && stdout.contains("Mutation score: 33.33%")
            && stdout.contains("Covered-code mutation score: 50.00%"),
        "coverage results must keep total and covered scores separate: {stdout}"
    );
    let records = fs::read_to_string(&record).expect("selected test records must exist");
    assert!(
        records.contains("detected|")
            && records.contains("--exact detects_detected")
            && records.contains("escaped|")
            && records.contains("--exact detects_escaped")
            && !records.contains("uncovered|"),
        "only mapped covered tests must run: {records}"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "coverage and mutation temporary data must be removed"
    );

    let failed_gate = Command::new(command_path(&install))
        .args(["--coverage", "--per-test", "--min-covered-msi", "51"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_COVERAGE_SOURCE", &source)
        .env("MUTARUST_COVERAGE_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must check covered score");
    assert_eq!(failed_gate.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&failed_gate.stderr).contains("covered-code mutation score"),
        "covered score gate must explain failure"
    );

    let _ = fs::remove_file(&record);
    let invalid = Command::new(command_path(&install))
        .arg("--coverage")
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_COVERAGE_MODE", "invalid")
        .env("MUTARUST_COVERAGE_SOURCE", &source)
        .env("MUTARUST_COVERAGE_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must reject invalid coverage");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&invalid.stderr).contains("LLVM coverage"),
        "invalid coverage must have a clear diagnostic"
    );
    assert!(
        !record.exists(),
        "invalid coverage must stop before any mutant test can escape"
    );
    assert!(mutarust_temp_entries(&temporary_root).is_empty());

    let missing = Command::new(command_path(&install))
        .arg("--coverage")
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_COVERAGE_MODE", "missing")
        .env("MUTARUST_COVERAGE_SOURCE", &source)
        .env("MUTARUST_COVERAGE_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must reject missing coverage");
    assert_eq!(missing.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("could not read LLVM coverage data"),
        "missing coverage must have a clear diagnostic"
    );
    assert!(!record.exists());
    assert!(mutarust_temp_entries(&temporary_root).is_empty());
}

#[cfg(unix)]
#[test]
fn installed_command_isolates_full_and_per_test_coverage() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_shared_coverage_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("shared coverage source must be readable");
    let fake_cargo = root.join("shared-coverage-cargo");
    let record = root.join("shared-coverage-record");
    let temporary_root = root.join("shared-coverage-temporary");
    fs::create_dir(&temporary_root).expect("shared coverage temporary root must be created");
    fs::write(
        &fake_cargo,
        "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec \"$MUTARUST_REAL_CARGO\" \"$@\"\nfi\nall=\"$*\"\nif [ \"$1\" = \"llvm-cov\" ]; then\n  case \" $all \" in\n    *\" --exact \"*) [ \"$2\" = test ] || exit 95 ;;\n  esac\n  output=\n  while [ \"$#\" -gt 0 ]; do\n    if [ \"$1\" = \"--output-path\" ]; then\n      output=$2\n      break\n    fi\n    shift\n  done\n  touch Cargo.lock\n  case \" $all \" in\n    *\" --test left \"*\" --exact shared \"*) data='DA:1,1' ;;\n    *\" --test right \"*\" --exact shared \"*) data='DA:2,1' ;;\n    *\" --exact detects_left \"*) data='DA:3,1' ;;\n    *\" --exact detects_right \"*) data='DA:3,1' ;;\n    *) data='DA:1,1\\nDA:2,1\\nDA:3,1' ;;\n  esac\n  printf 'SF:%s\\n%b\\nend_of_record\\n' \"$MUTARUST_COVERAGE_SOURCE\" \"$data\" > \"$output\"\n  exit 0\nfi\ncase \" $all \" in\n  *\" --test left \"*\" --list \"*) printf 'shared: test\\ndetects_left: test\\n'; exit 0 ;;\n  *\" --test right \"*\" --list \"*) printf 'shared: test\\ndetects_right: test\\n'; exit 0 ;;\n  *\" --list \"*) exit 0 ;;\nesac\ncase \" $all \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\nif grep -q 'pub fn first() -> bool { false }' checked/src/lib.rs; then\n  mutant=first\nelif grep -q 'pub fn second() -> bool { false }' checked/src/lib.rs; then\n  mutant=second\nelif grep -q 'pub fn shared() -> bool { false }' checked/src/lib.rs; then\n  mutant=shared\nelse\n  exit 94\nfi\nprintf '%s|%s\\n' \"$mutant\" \"$all\" >> \"$MUTARUST_COVERAGE_RECORD\"\ncase \"$mutant|$all\" in\n  first*\" --test left \"*\" --exact shared \"*) exit 1 ;;\n  second*\" --test right \"*\" --exact shared \"*) exit 1 ;;\n  shared*\" --exact detects_left \"*) exit 0 ;;\n  shared*\" --exact detects_right \"*) exit 0 ;;\n  *) exit 93 ;;\nesac\n",
    )
    .expect("shared coverage Cargo command must be written");
    let fake_cargo_script =
        fs::read_to_string(&fake_cargo).expect("shared coverage Cargo command must be readable");
    let fake_cargo_script = fake_cargo_script
        .replacen(
            "if grep -q 'pub fn first() -> bool { false }' checked/src/lib.rs; then",
            "case \" $all \" in\n  *\" --exact \"*) ;;\n  *) exit 0 ;;\nesac\nif grep -q 'pub fn first() -> bool { false }' checked/src/lib.rs; then",
            1,
        )
        .replace(
            "shared*\" --exact detects_left \"*)",
            "shared*\" --exact detects_left\"*)",
        )
        .replace(
            "shared*\" --exact detects_right \"*)",
            "shared*\" --exact detects_right\"*)",
        );
    fs::write(&fake_cargo, fake_cargo_script)
        .expect("shared coverage Cargo command must accept the clean test");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("shared coverage Cargo command must be executable");

    let output = Command::new(command_path(&install))
        .args(["--coverage", "--per-test", "--workers", "1"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_REAL_CARGO", env!("CARGO"))
        .env("MUTARUST_COVERAGE_SOURCE", &source)
        .env("MUTARUST_COVERAGE_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must collect isolated shared coverage");

    assert!(
        output.status.success(),
        "shared coverage run must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("shared coverage output must be UTF-8");
    assert!(
        stdout.contains("Killed: 2")
            && stdout.contains("Escaped: 1")
            && stdout.contains("Not covered: 0")
            && stdout.contains("Mutation score: 66.67%")
            && stdout.contains("Covered-code mutation score: 66.67%"),
        "fully covered source must keep both scores: {stdout}"
    );
    let records = fs::read_to_string(&record).expect("shared coverage records must exist");
    let first = coverage_record(&records, "first");
    let second = coverage_record(&records, "second");
    let shared = records
        .lines()
        .filter(|line| line.starts_with("shared|"))
        .collect::<Vec<_>>();
    assert!(
        first.contains("--test left")
            && first.contains("--exact shared")
            && second.contains("--test right")
            && second.contains("--exact shared")
            && shared.len() == 2
            && shared
                .iter()
                .any(|line| line.contains("--exact detects_left"))
            && shared
                .iter()
                .any(|line| line.contains("--exact detects_right")),
        "per-test coverage must keep duplicate names separate and run all shared coverage tests: {records}"
    );
    assert!(
        !first.contains("--test right") && !second.contains("--test left"),
        "per-test coverage must not run a duplicate name from another target: {records}"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert!(
        !fixture.join("Cargo.lock").exists(),
        "coverage collection must not write a lock file in the user workspace"
    );
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "isolated coverage data must be removed"
    );
}

#[cfg(unix)]
fn coverage_record<'a>(records: &'a str, mutant: &str) -> &'a str {
    records
        .lines()
        .find(|line| line.starts_with(&format!("{mutant}|")))
        .expect("coverage test record must exist")
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
#[test]
fn installed_command_runs_custom_commands_with_a_stable_contract() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("custom-command source must be readable");
    let command = root.join("custom-command");
    let record = root.join("custom-command-record");
    let child_identifier = root.join("custom-command-child");
    let temporary_root = root.join("custom-command-temporary");
    fs::create_dir(&temporary_root).expect("custom-command temporary root must be created");
    fs::write(
        &command,
        "#!/bin/sh\nif [ ! -f \"$MUTATE_ORIGINAL\" ] || [ ! -f \"$MUTATE_CHANGED\" ]; then\n  exit 9\nfi\nif cmp -s \"$MUTATE_ORIGINAL\" \"$MUTATE_CHANGED\"; then\n  exit 10\nfi\nprintf '%s\\n' \"$MUTATE_ORIGINAL\" \"$MUTATE_CHANGED\" \"$MUTATE_PACKAGE\" \"$MUTATE_TIMEOUT\" \"$TEST_RECURSIVE\" \"$MUTATE_VERBOSE\" \"$MUTATE_DEBUG\" \"$PWD\" > \"$MUTARUST_CUSTOM_RECORD\"\nif [ \"$MUTARUST_CUSTOM_WAIT\" = true ]; then\n  sleep 30 &\n  echo $! > \"$MUTARUST_CUSTOM_CHILD\"\n  wait\nfi\nexit \"${MUTARUST_CUSTOM_EXIT:-1}\"\n",
    )
    .expect("custom command must be written");
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755))
        .expect("custom command must be executable");

    for (exit, state) in [
        ("0", "Killed"),
        ("1", "Escaped"),
        ("2", "Skipped"),
        ("3", "Errored"),
    ] {
        let output = Command::new(command_path(&install))
            .args([
                "--exec",
                command.to_str().expect("custom command path must be UTF-8"),
                "--exec-timeout",
                "1",
                "--test-recursive",
                "--verbose",
                "--debug",
            ])
            .arg(&source)
            .current_dir(&fixture)
            .env("MUTARUST_CUSTOM_EXIT", exit)
            .env("MUTARUST_CUSTOM_RECORD", &record)
            .env("TMPDIR", &temporary_root)
            .output()
            .expect("installed mutarust must start custom command");
        assert!(
            output.status.success(),
            "custom command status {exit} must complete: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("custom command output must be UTF-8");
        assert!(
            stdout.contains(&format!("{state}: 2")),
            "custom command status {exit} must map to {state}: {stdout}"
        );
        if exit == "3" {
            assert!(
                stdout.contains("custom command exited with status 3"),
                "unknown custom status must have clear detail: {stdout}"
            );
        }
    }

    let values = fs::read_to_string(&record)
        .expect("custom command must record its environment")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(
        values.len(),
        8,
        "custom command must receive all contract values"
    );
    assert_ne!(values[0], source.display().to_string());
    assert_ne!(values[1], source.display().to_string());
    assert_eq!(values[2], "mutation-checked");
    assert_eq!(values[3], "1");
    assert_eq!(values[4], "true");
    assert_eq!(values[5], "true");
    assert_eq!(values[6], "true");
    let temporary_root =
        fs::canonicalize(&temporary_root).expect("custom-command temporary root must resolve");
    assert!(
        values[7].starts_with(
            temporary_root
                .to_str()
                .expect("temporary path must be UTF-8")
        ),
        "custom command must run in an isolated workspace: {}",
        values[7]
    );

    let missing = Command::new(command_path(&install))
        .args([
            "--exec",
            root.join("missing-custom-command")
                .to_str()
                .expect("missing command path must be UTF-8"),
        ])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject a missing custom command");
    assert_eq!(missing.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("custom command"),
        "missing custom command must have a clear error"
    );

    let invalid = Command::new(command_path(&install))
        .args(["--exec", "'"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject an invalid custom command");
    assert_eq!(invalid.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&invalid.stderr).contains("custom command"),
        "invalid custom command must have a clear error"
    );

    let non_executable = root.join("non-executable-custom-command");
    fs::write(&non_executable, "#!/bin/sh\nexit 1\n")
        .expect("non-executable custom command must be written");
    fs::set_permissions(&non_executable, fs::Permissions::from_mode(0o600))
        .expect("non-executable custom command permissions must be set");
    let non_executable = Command::new(command_path(&install))
        .args([
            "--exec",
            non_executable
                .to_str()
                .expect("non-executable command path must be UTF-8"),
        ])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject a non-executable custom command");
    assert_eq!(non_executable.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&non_executable.stderr).contains("custom command"),
        "a non-executable command must have a clear error"
    );

    let started = std::time::Instant::now();
    let timed_out = Command::new(command_path(&install))
        .args([
            "--exec",
            command.to_str().expect("custom command path must be UTF-8"),
            "--exec-timeout",
            "1",
        ])
        .arg(&source)
        .current_dir(&fixture)
        .env("MUTARUST_CUSTOM_WAIT", "true")
        .env("MUTARUST_CUSTOM_CHILD", &child_identifier)
        .env("MUTARUST_CUSTOM_RECORD", &record)
        .env("TMPDIR", &temporary_root)
        .output()
        .expect("installed mutarust must start timed custom command");
    assert!(timed_out.status.success());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "custom-command timeout must stop promptly"
    );
    let stdout = String::from_utf8(timed_out.stdout).expect("timeout output must be UTF-8");
    assert!(
        stdout.contains("Errored: 2")
            && stdout.contains("custom command timed out after 1 seconds"),
        "custom-command timeout must produce errored results: {stdout}"
    );
    let child = fs::read_to_string(&child_identifier)
        .expect("timed custom command child must be written")
        .trim()
        .to_owned();
    assert!(
        process_has_stopped(&child),
        "custom-command timeout must stop child processes"
    );
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "custom-command workspaces must be removed"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
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
fn installed_command_interrupts_custom_command_children() {
    use std::os::unix::fs::PermissionsExt;

    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_mutation_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");
    let source_before = fs::read(&source).expect("interrupt source must be readable");
    let custom_command = root.join("interrupt-custom-command");
    let command_identifier = root.join("interrupt-custom-command-identifier");
    let child_identifier = root.join("interrupt-custom-command-child");
    let temporary_root = root.join("interrupt-custom-command-temporary");
    fs::create_dir(&temporary_root).expect("mutation temporary root must be created");
    fs::write(
        &custom_command,
        "#!/bin/sh\necho $$ > \"$MUTARUST_INTERRUPTED_CUSTOM_COMMAND\"\nsleep 30 &\necho $! > \"$MUTARUST_INTERRUPTED_CUSTOM_CHILD\"\nwait\n",
    )
    .expect("custom command must be written");
    fs::set_permissions(&custom_command, fs::Permissions::from_mode(0o755))
        .expect("custom command must be executable");

    let mut command = Command::new(command_path(&install));
    let process = command
        .args([
            "--exec",
            custom_command
                .to_str()
                .expect("custom command path must be UTF-8"),
        ])
        .arg(&source)
        .current_dir(&fixture)
        .env("MUTARUST_INTERRUPTED_CUSTOM_COMMAND", &command_identifier)
        .env("MUTARUST_INTERRUPTED_CUSTOM_CHILD", &child_identifier)
        .env("TMPDIR", &temporary_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("installed mutarust must start");
    wait_for_file(&command_identifier, "interrupted custom command must start");
    wait_for_file(&child_identifier, "interrupted custom child must start");
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
    let custom_command = fs::read_to_string(&command_identifier)
        .expect("interrupted custom command identifier must be written")
        .trim()
        .to_owned();
    let child = fs::read_to_string(&child_identifier)
        .expect("interrupted custom child identifier must be written")
        .trim()
        .to_owned();
    assert!(
        process_has_stopped(&custom_command),
        "the interrupt must stop the custom command"
    );
    assert!(
        process_has_stopped(&child),
        "the interrupt must stop the custom command child"
    );
    assert!(
        mutarust_temp_entries(&temporary_root).is_empty(),
        "the interrupt must remove each mutation workspace"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
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
            "#!/bin/sh\nif [ \"$1\" = \"metadata\" ]; then\n  exec '{}' \"$@\"\nfi\nif ! grep -q false checked/src/lib.rs 2>/dev/null; then\n  exit 0\nfi\nif [ \"$(grep -c false checked/src/lib.rs)\" -ne 1 ]; then\n  exit 93\nfi\ncase \" $* \" in\n  *\" --no-run \"*) exit 0 ;;\nesac\necho $$ >> \"$MUTARUST_INTERRUPTED_CARGO\"\nsleep 30\n",
            env!("CARGO")
        ),
    )
    .expect("fake cargo command must be written");
    fs::set_permissions(&fake_cargo, fs::Permissions::from_mode(0o755))
        .expect("fake cargo command must be executable");

    let mut command = Command::new(command_path(&install));
    let process = command
        .args(["--workers", "2"])
        .arg(&source)
        .current_dir(&fixture)
        .env("CARGO", &fake_cargo)
        .env("MUTARUST_INTERRUPTED_CARGO", &cargo_identifier)
        .env("TMPDIR", &temporary_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("installed mutarust must start");
    wait_for_file_lines(
        &cargo_identifier,
        2,
        "parallel interrupted Cargo processes must start",
    );
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
    let cargo_processes = fs::read_to_string(&cargo_identifier)
        .expect("interrupted Cargo identifiers must be written");
    for cargo in cargo_processes.lines() {
        assert!(
            process_has_stopped(cargo),
            "the interrupt must stop each Cargo process"
        );
    }
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

#[test]
fn installed_command_selects_git_merge_base_and_uncommitted_changes() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = git_mutation_fixture(&root, "merge-base", "main");
    let a = write_git_source(&fixture, "src/a.rs", "pub fn a() -> bool { true }\n");
    let b = write_git_source(&fixture, "src/b.rs", "pub fn b() -> bool { true }\n");
    let c = write_git_source(&fixture, "src/c.rs", "pub fn c() -> bool { true }\n");
    let d = write_git_source(&fixture, "src/d.rs", "pub fn d() -> bool { true }\n");
    commit_all(&fixture, "base");
    run_git(&fixture, &["switch", "-c", "feature"]);

    run_git(&fixture, &["switch", "main"]);
    write_git_source(&fixture, "src/b.rs", "pub fn b() -> bool { false }\n");
    commit_all(&fixture, "base moves ahead");
    run_git(&fixture, &["switch", "feature"]);

    write_git_source(&fixture, "src/c.rs", "pub fn c() -> bool { false }\n");
    commit_all(&fixture, "feature commit");
    write_git_source(&fixture, "src/d.rs", "pub fn d() -> bool { false }\n");
    run_git(&fixture, &["add", "src/d.rs"]);
    write_git_source(&fixture, "src/a.rs", "pub fn a() -> bool { false }\n");

    for source in [&a, &c, &d] {
        assert_dry_run_total(&git_dry_run(&install, &fixture, Some("main"), source), 1);
    }
    assert_dry_run_total(&git_dry_run(&install, &fixture, Some("main"), &b), 0);
}

#[test]
fn installed_command_uses_git_remote_default_and_master_fallback() {
    let root = smoke_root();
    let install = install_command(&root);
    let remote = root.join("remote.git");
    let remote_text = remote.to_str().expect("remote path must be UTF-8");
    run_git(
        &root,
        &["init", "--bare", "--initial-branch=trunk", remote_text],
    );

    let seed = git_mutation_fixture(&root, "remote-seed", "trunk");
    write_git_source(&seed, "src/lib.rs", "pub fn value() -> bool { true }\n");
    commit_all(&seed, "seed");
    run_git(&seed, &["remote", "add", "origin", remote_text]);
    run_git(&seed, &["push", "--set-upstream", "origin", "trunk"]);

    let clone = root.join("remote-clone");
    let clone_text = clone.to_str().expect("clone path must be UTF-8");
    run_git(&root, &["clone", remote_text, clone_text]);
    run_git(&clone, &["switch", "-c", "feature"]);
    let remote_source =
        write_git_source(&clone, "src/lib.rs", "pub fn value() -> bool { false }\n");
    assert_dry_run_total(&git_dry_run(&install, &clone, None, &remote_source), 1);

    let fallback = git_mutation_fixture(&root, "master-fallback", "master");
    let fallback_source =
        write_git_source(&fallback, "src/lib.rs", "pub fn value() -> bool { true }\n");
    commit_all(&fallback, "base");
    run_git(&fallback, &["switch", "-c", "feature"]);
    write_git_source(
        &fallback,
        "src/lib.rs",
        "pub fn value() -> bool { false }\n",
    );
    assert_dry_run_total(&git_dry_run(&install, &fallback, None, &fallback_source), 1);
}

#[test]
fn installed_command_selects_added_and_renamed_git_source_files() {
    let root = smoke_root();
    let install = install_command(&root);

    let added = git_mutation_fixture(&root, "added", "main");
    write_git_source(&added, "src/lib.rs", "pub fn base() -> bool { true }\n");
    commit_all(&added, "base");
    run_git(&added, &["switch", "-c", "feature"]);
    let added_source =
        write_git_source(&added, "src/added.rs", "pub fn added() -> bool { true }\n");
    run_git(&added, &["add", "src/added.rs"]);
    assert_dry_run_total(
        &git_dry_run(&install, &added, Some("main"), &added_source),
        1,
    );

    let renamed = git_mutation_fixture(&root, "renamed", "main");
    let old = write_git_source(
        &renamed,
        "src/old.rs",
        "pub fn changed() -> bool { true }\npub fn one() -> bool { true }\npub fn two() -> bool { true }\npub fn three() -> bool { true }\npub fn four() -> bool { true }\n",
    );
    commit_all(&renamed, "base");
    run_git(&renamed, &["switch", "-c", "feature"]);
    let new = renamed.join("src/new.rs");
    run_git(
        &renamed,
        &[
            "mv",
            old.strip_prefix(&renamed)
                .expect("renamed source must be below the repository")
                .to_str()
                .expect("renamed source path must be UTF-8"),
            "src/new.rs",
        ],
    );
    assert_dry_run_total(&git_dry_run(&install, &renamed, Some("main"), &new), 0);
    fs::write(
        &new,
        "pub fn changed() -> bool { false }\npub fn one() -> bool { true }\npub fn two() -> bool { true }\npub fn three() -> bool { true }\npub fn four() -> bool { true }\n",
    )
    .expect("renamed Git source must be written");
    run_git(&renamed, &["add", "src/new.rs"]);
    assert_dry_run_total(&git_dry_run(&install, &renamed, Some("main"), &new), 1);
}

#[test]
fn installed_command_ignores_deleted_git_lines_and_selects_multiple_hunks() {
    let root = smoke_root();
    let install = install_command(&root);

    let deleted = git_mutation_fixture(&root, "deleted", "main");
    let deleted_source = write_git_source(
        &deleted,
        "src/lib.rs",
        "pub fn deleted() -> bool { true }\npub fn retained() -> bool { true }\n",
    );
    commit_all(&deleted, "base");
    run_git(&deleted, &["switch", "-c", "feature"]);
    write_git_source(
        &deleted,
        "src/lib.rs",
        "pub fn retained() -> bool { true }\n",
    );
    run_git(&deleted, &["add", "src/lib.rs"]);
    assert_dry_run_total(
        &git_dry_run(&install, &deleted, Some("main"), &deleted_source),
        0,
    );

    let hunks = git_mutation_fixture(&root, "hunks", "main");
    let hunk_source = write_git_source(
        &hunks,
        "src/lib.rs",
        "pub fn first() -> bool { true }\npub fn keep_one() -> bool { true }\npub fn keep_two() -> bool { true }\npub fn keep_three() -> bool { true }\npub fn second() -> bool { true }\n",
    );
    commit_all(&hunks, "base");
    run_git(&hunks, &["switch", "-c", "feature"]);
    write_git_source(
        &hunks,
        "src/lib.rs",
        "pub fn first() -> bool { false }\npub fn keep_one() -> bool { true }\npub fn keep_two() -> bool { true }\npub fn keep_three() -> bool { true }\npub fn second() -> bool { false }\n",
    );
    assert_dry_run_total(
        &git_dry_run(&install, &hunks, Some("main"), &hunk_source),
        2,
    );
}

#[test]
fn installed_command_succeeds_when_git_diff_has_no_mutable_lines() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = git_mutation_fixture(&root, "no-mutants", "main");
    let source = write_git_source(&fixture, "src/lib.rs", "pub fn value() -> bool { true }\n");
    commit_all(&fixture, "base");
    run_git(&fixture, &["switch", "-c", "feature"]);
    fs::write(fixture.join("README.md"), "Changed documentation.\n")
        .expect("Git fixture README must be written");
    commit_all(&fixture, "docs only");

    let output = git_dry_run(&install, &fixture, Some("main"), &source);
    assert_dry_run_total(&output, 0);
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("zero-mutant output must be UTF-8")
            .trim(),
        "Total: 0 mutation(s) would be generated. No files written, no tests run."
    );
}

#[test]
fn installed_command_rejects_invalid_git_scope() {
    let root = smoke_root();
    let install = install_command(&root);
    let non_git = root.join("non-git");
    let non_git_source = non_git.join("src/lib.rs");
    fs::create_dir_all(
        non_git_source
            .parent()
            .expect("non-Git source must have a parent"),
    )
    .expect("non-Git source directory must be created");
    fs::write(
        non_git.join("Cargo.toml"),
        "[package]\nname = \"non-git\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("non-Git manifest must be written");
    fs::write(&non_git_source, "pub fn value() -> bool { true }\n")
        .expect("non-Git source must be written");
    assert_git_scope_error(
        git_dry_run(&install, &non_git, None, &non_git_source),
        "could not find a Git repository",
    );

    let fixture = git_mutation_fixture(&root, "bad-base", "main");
    let source = write_git_source(&fixture, "src/lib.rs", "pub fn value() -> bool { true }\n");
    commit_all(&fixture, "base");
    assert_git_scope_error(
        git_dry_run(&install, &fixture, Some("does-not-exist"), &source),
        "does-not-exist",
    );

    let external = git_mutation_fixture(&root, "external-git", "main");
    let external_source =
        write_git_source(&external, "src/lib.rs", "pub fn value() -> bool { true }\n");
    commit_all(&external, "base");
    run_git(&external, &["switch", "-c", "feature"]);
    write_git_source(
        &external,
        "src/lib.rs",
        "pub fn value() -> bool { false }\n",
    );
    assert_git_scope_error(
        git_dry_run(&install, &fixture, Some("main"), &external_source),
        "outside Git repository",
    );

    let output = Command::new(command_path(&install))
        .args(["--git-diff-base", "main", "--dry-run"])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must reject a base without changed-line selection");
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--git-diff-base requires --git-diff-lines"),
        "invalid Git controls must explain the required selector"
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

fn git_mutation_fixture(root: &Path, name: &str, branch: &str) -> PathBuf {
    let fixture = root.join(name);
    fs::create_dir_all(fixture.join("src")).expect("Git fixture source directory must be created");
    fs::write(fixture.join("src/lib.rs"), "").expect("Git fixture library source must be written");
    fs::write(
        fixture.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )
    .expect("Git fixture manifest must be written");
    run_git(&fixture, &["init", "--initial-branch", branch]);
    run_git(
        &fixture,
        &["config", "user.email", "mutarust@example.invalid"],
    );
    run_git(&fixture, &["config", "user.name", "Mutarust Test"]);
    fixture
}

fn write_git_source(repository: &Path, relative: &str, text: &str) -> PathBuf {
    let source = repository.join(relative);
    fs::create_dir_all(source.parent().expect("Git source must have a parent"))
        .expect("Git source directory must be created");
    fs::write(&source, text).expect("Git source must be written");
    source
}

fn commit_all(repository: &Path, message: &str) {
    run_git(repository, &["add", "."]);
    run_git(repository, &["commit", "--message", message]);
}

fn git_dry_run(
    install: &Path,
    repository: &Path,
    base: Option<&str>,
    source: &Path,
) -> std::process::Output {
    let mut command = Command::new(command_path(install));
    command.args(["--git-diff-lines", "--dry-run"]);
    if let Some(base) = base {
        command.args(["--git-diff-base", base]);
    }
    command
        .arg(source)
        .current_dir(repository)
        .output()
        .expect("installed mutarust must select changed Git lines")
}

fn assert_dry_run_total(output: &std::process::Output, expected: usize) {
    assert!(
        output.status.success(),
        "changed-line selection must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("Total: {expected} mutation(s) would be generated")),
        "changed-line selection must report {expected} mutants: {stdout}"
    );
}

fn assert_git_scope_error(output: std::process::Output, required_text: &str) {
    assert_eq!(
        output.status.code(),
        Some(3),
        "Git scope errors must return exit value 3"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(required_text),
        "Git scope error must identify {required_text}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("Total:"),
        "Git scope errors must not select an unfiltered source scope"
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

fn write_coverage_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("coverage-fixture");
    let package = fixture.join("checked");
    fs::create_dir_all(package.join("src"))
        .expect("coverage fixture source directory must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[workspace]\nmembers = [\"checked\"]\nresolver = \"2\"\n",
    )
    .expect("coverage fixture workspace manifest must be written");
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = \"coverage-checked\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("coverage fixture package manifest must be written");
    fs::write(
        package.join("src").join("lib.rs"),
        "pub fn detected() -> bool { true }\npub fn escaped() -> bool { true }\npub fn uncovered() -> bool { true }\n",
    )
    .expect("coverage fixture source must be written");
    fixture
}

fn write_shared_coverage_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("shared-coverage-fixture");
    let package = fixture.join("checked");
    fs::create_dir_all(package.join("src"))
        .expect("shared coverage fixture source directory must be created");
    fs::create_dir_all(package.join("tests"))
        .expect("shared coverage fixture test directory must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[workspace]\nmembers = [\"checked\"]\nresolver = \"2\"\n",
    )
    .expect("shared coverage fixture workspace manifest must be written");
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname = \"shared-coverage-checked\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("shared coverage fixture package manifest must be written");
    fs::write(
        package.join("src").join("lib.rs"),
        "pub fn first() -> bool { true }\npub fn second() -> bool { true }\npub fn shared() -> bool { true }\n",
    )
    .expect("shared coverage fixture source must be written");
    fs::write(
        package.join("tests").join("left.rs"),
        "#[test]\nfn shared() {}\n",
    )
    .expect("shared coverage left test must be written");
    fs::write(
        package.join("tests").join("right.rs"),
        "#[test]\nfn shared() {}\n",
    )
    .expect("shared coverage right test must be written");
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
