use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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

#[test]
fn self_mutation_workflow_builds_the_release_command() {
    let workflow = mutation_workflow();
    assert!(
        workflow.contains("cargo build --release --locked"),
        "self-mutation CI must build the release command"
    );
    assert!(
        workflow.contains("./target/release/mutarust"),
        "self-mutation CI must run the release binary"
    );
}

#[test]
fn self_mutation_workflow_uses_changed_lines_on_pull_requests() {
    let workflow = mutation_workflow();
    assert!(
        workflow.contains("pull_request"),
        "self-mutation CI must run on pull requests"
    );
    assert!(
        workflow.contains("--git-diff-lines"),
        "pull requests must mutate changed lines only"
    );
    assert!(
        workflow.contains("--git-diff-base origin/main"),
        "changed-line selection must use the main merge base"
    );
    assert!(
        workflow.contains("--ignore-msi-with-no-mutations"),
        "a pull request with no mutable lines must pass"
    );
    assert!(
        workflow.contains(r#"EVENT_NAME: ${{ github.event_name }}"#),
        "pull-request scope must be event-controlled"
    );
    assert!(
        workflow.contains(r#"[[ "$EVENT_NAME" == "pull_request" ]]"#),
        "changed-line flags must apply only on pull requests"
    );
}

#[test]
fn self_mutation_workflow_keeps_full_main_scope_and_score_gates() {
    let workflow = mutation_workflow();
    assert!(
        workflow.contains("branches: [main]"),
        "self-mutation CI must run on main pushes"
    );
    assert!(
        workflow.contains("--min-msi 75"),
        "self-mutation CI must require at least 75 percent total score"
    );
    assert!(
        workflow.contains("--min-covered-msi 80"),
        "self-mutation CI must require at least 80 percent covered-code score"
    );
    assert!(
        workflow.contains("--coverage"),
        "covered-code score requires coverage collection"
    );
    assert!(
        workflow.contains("src/report/github.rs")
            && workflow.contains("src/report/gitlab.rs")
            && workflow.contains("src/evidence.rs")
            && workflow.contains("src/progress.rs"),
        "main pushes must mutate the approved production scope"
    );
    assert!(
        workflow.contains("Excluded (written technical reasons):")
            && workflow.contains("src/main.rs")
            && workflow.contains("src/execution.rs"),
        "production exclusions must have a written technical reason"
    );
    assert!(
        workflow.contains("if [[ \"$EVENT_NAME\" == \"pull_request\" ]]; then")
            && workflow.contains("scope_args=(")
            && workflow.contains("./target/release/mutarust"),
        "full-source main runs must remain active even when changed-line CI passes"
    );
}

#[test]
fn installed_command_exits_control_the_self_mutation_gates() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_gate_fixture(&root);
    let source = fixture.join("checked").join("src").join("lib.rs");

    let failed = Command::new(command_path(&install))
        .args([
            "--enable",
            "conditional/bool-literal",
            "--min-msi",
            "75",
            "--min-covered-msi",
            "80",
        ])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start for score gates");
    assert_eq!(
        failed.status.code(),
        Some(4),
        "a failed score gate must return exit value 4: {}",
        String::from_utf8_lossy(&failed.stderr)
    );

    let empty = Command::new(command_path(&install))
        .args([
            "--enable",
            "conditional/bool-literal",
            "--min-msi",
            "75",
            "--ignore-msi-with-no-mutations",
        ])
        .arg(fixture.join("checked").join("src").join("empty.rs"))
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start for empty changed-line gates");
    assert!(
        empty.status.success(),
        "no mutable lines with ignore-msi-with-no-mutations must pass: {}",
        String::from_utf8_lossy(&empty.stderr)
    );
}

#[test]
fn installed_command_changed_line_selection_controls_self_mutation_scope() {
    let root = smoke_root();
    let install = install_command(&root);
    let fixture = write_changed_line_fixture(&root);
    let source = fixture.join("src").join("lib.rs");

    let unchanged = Command::new(command_path(&install))
        .args([
            "--git-diff-lines",
            "--git-diff-base",
            "main",
            "--enable",
            "conditional/bool-literal",
            "--min-msi",
            "75",
            "--ignore-msi-with-no-mutations",
            "--dry-run",
        ])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start for unchanged Git scope");
    assert!(
        unchanged.status.success(),
        "unchanged production lines must pass: {}",
        String::from_utf8_lossy(&unchanged.stderr)
    );
    let unchanged_stdout =
        String::from_utf8(unchanged.stdout).expect("unchanged stdout must be UTF-8");
    assert!(
        unchanged_stdout.contains("Total: 0 mutation(s) would be generated"),
        "unchanged lines must produce no mutants: {unchanged_stdout}"
    );

    fs::write(
        &source,
        "pub fn checked() -> bool { let value = false; value }\n",
    )
    .expect("changed production source must be written");
    let changed = Command::new(command_path(&install))
        .args([
            "--git-diff-lines",
            "--git-diff-base",
            "main",
            "--enable",
            "conditional/bool-literal",
            "--dry-run",
        ])
        .arg(&source)
        .current_dir(&fixture)
        .output()
        .expect("installed mutarust must start for changed Git scope");
    assert!(
        changed.status.success(),
        "changed production lines must select mutants: {}",
        String::from_utf8_lossy(&changed.stderr)
    );
    let changed_stdout = String::from_utf8(changed.stdout).expect("changed stdout must be UTF-8");
    assert!(
        changed_stdout.contains("Total: 1 mutation(s) would be generated"),
        "changed lines must select mutable production source: {changed_stdout}"
    );
}

fn mutation_workflow() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/mutation.yml"),
    )
    .expect("self-mutation workflow must exist")
}

fn write_gate_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("self-mutation-gate-fixture");
    fs::create_dir_all(fixture.join("checked").join("src"))
        .expect("gate fixture source must be created");
    fs::create_dir_all(fixture.join("checked").join("tests"))
        .expect("gate fixture tests must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[workspace]\nmembers = [\"checked\"]\nresolver = \"2\"\n",
    )
    .expect("gate fixture manifest must be written");
    fs::write(
        fixture.join("checked").join("Cargo.toml"),
        "[package]\nname = \"mutation-checked\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("gate fixture package manifest must be written");
    fs::write(
        fixture.join("checked").join("src").join("lib.rs"),
        "pub fn checked() -> bool { let value = true; value }\npub fn unchecked() -> bool { let value = true; value }\n",
    )
    .expect("gate fixture source must be written");
    fs::write(
        fixture.join("checked").join("src").join("empty.rs"),
        "pub fn empty() {}\n",
    )
    .expect("empty gate fixture source must be written");
    fs::write(
        fixture.join("checked").join("tests").join("mutation.rs"),
        "#[test]\nfn detects_checked_value() {\n    assert!(mutation_checked::checked());\n}\n",
    )
    .expect("gate fixture test must be written");
    fixture
}

fn write_changed_line_fixture(root: &Path) -> PathBuf {
    let fixture = root.join("self-mutation-changed-lines");
    fs::create_dir_all(fixture.join("src")).expect("changed-line fixture source must be created");
    fs::write(
        fixture.join("Cargo.toml"),
        "[package]\nname = \"self-mutation-changed-lines\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("changed-line fixture manifest must be written");
    fs::write(
        fixture.join("src").join("lib.rs"),
        "pub fn checked() -> bool { let value = true; value }\n",
    )
    .expect("changed-line fixture source must be written");
    run_git(&fixture, &["init", "--initial-branch", "main"]);
    run_git(
        &fixture,
        &["config", "user.email", "mutarust@example.invalid"],
    );
    run_git(&fixture, &["config", "user.name", "Mutarust Test"]);
    run_git(&fixture, &["add", "."]);
    run_git(&fixture, &["commit", "--message", "initial"]);
    fixture
}

fn run_git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .expect("git must start");
    assert!(
        output.status.success(),
        "git {:?} must succeed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn smoke_root() -> SmokeRoot {
    loop {
        let root = smoke_root_name();

        match fs::create_dir(&root) {
            Ok(()) => return SmokeRoot(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("self-mutation smoke root must be created: {error}"),
        }
    }
}

fn smoke_root_name() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time must be after the Unix epoch")
        .as_nanos();
    let sequence = NEXT_SMOKE_ROOT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "mutarust-self-mutation-ci-{}-{nonce}-{sequence}",
        std::process::id(),
    ))
}

fn install_command(root: &Path) -> PathBuf {
    let package_target = root.join("package-target");
    let target = root.join("target");
    let install = root.join("install");
    let package_status = Command::new(env!("CARGO"))
        .args([
            "package",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            "--allow-dirty",
            "--locked",
        ])
        .env("CARGO_TARGET_DIR", &package_target)
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
    let install_status = Command::new(env!("CARGO"))
        .args(["install", "--path"])
        .arg(&package_root)
        .arg("--root")
        .arg(&install)
        .args(["--locked", "--force"])
        .env("CARGO_TARGET_DIR", target)
        .status()
        .expect("cargo install must start");
    assert!(install_status.success(), "cargo install must succeed");
    install
}

fn command_path(install: &Path) -> PathBuf {
    install.join("bin").join(if cfg!(windows) {
        "mutarust.exe"
    } else {
        "mutarust"
    })
}
