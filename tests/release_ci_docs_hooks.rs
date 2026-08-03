use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn quality_workflow_checks_msrv_and_stable_rust() {
    let workflow = read_repo_file(".github/workflows/quality.yml");
    assert!(
        workflow.contains(r#"toolchain: ["1.85", stable]"#)
            || workflow.contains("toolchain: [\"1.85\", stable]"),
        "quality CI must check the minimum supported Rust version and stable Rust"
    );
}

#[test]
fn quality_workflow_runs_format_clippy_release_build_locked_tests_and_docs() {
    let workflow = read_repo_file(".github/workflows/quality.yml");
    assert!(
        workflow.contains("cargo fmt --all -- --check"),
        "quality CI must run the format check"
    );
    assert!(
        workflow.contains("cargo clippy --workspace --all-targets --all-features -- -D warnings"),
        "quality CI must run Clippy on all targets and features with warnings denied"
    );
    assert!(
        workflow.contains("cargo build --release --locked"),
        "quality CI must run the release build"
    );
    assert!(
        workflow.contains("cargo test --workspace --all-targets --all-features --locked"),
        "quality CI must run locked tests"
    );
    assert!(
        workflow.contains("cargo doc --workspace --all-features --no-deps --locked"),
        "quality CI must build documentation"
    );
}

#[test]
fn messrust_workflow_uses_strict_released_production_checks() {
    let workflow = read_repo_file(".github/workflows/messrust.yml");
    assert!(
        workflow.contains("cargo install messrust --version 0.1.0 --locked"),
        "messrust CI must use the fixed released version"
    );
    assert!(
        workflow.contains("messrust src text codesize,design --ignore-tests --strict"),
        "messrust CI must use codesize and design rules, production-code scope, test exclusion, and strict mode"
    );
    assert!(
        !workflow.contains("--baseline") && !workflow.contains("--ignore-findings"),
        "messrust CI must not use a baseline or ignored findings"
    );
}

#[test]
fn security_workflow_runs_advisory_checks_for_prs_main_and_weekly() {
    let workflow = read_repo_file(".github/workflows/security.yml");
    assert!(
        workflow.contains("pull_request"),
        "security CI must run for pull requests"
    );
    assert!(
        workflow.contains("branches: [main]"),
        "security CI must run for main pushes"
    );
    assert!(
        workflow.contains("schedule:") && workflow.contains("cron:"),
        "security CI must run on a weekly schedule"
    );
    assert!(
        workflow.contains("cargo audit") || workflow.contains("cargo-audit"),
        "security CI must run an applicable Rust advisory check"
    );
}

#[test]
fn package_gate_installs_the_locked_artifact_and_checks_help_and_version() {
    let workflow = read_repo_file(".github/workflows/quality.yml");
    assert!(
        workflow.contains("cargo package --locked"),
        "the package gate must package with the lock file"
    );
    assert!(
        workflow.contains("cargo install --path")
            && workflow.contains("--root")
            && workflow.contains("--locked"),
        "the package gate must install the artifact in a clean Cargo root"
    );
    assert!(
        workflow.contains("mutarust\" --help") || workflow.contains("mutarust --help"),
        "the package gate must check help"
    );
    assert!(
        workflow.contains("mutarust\" --version") || workflow.contains("mutarust --version"),
        "the package gate must check version"
    );
}

#[test]
fn third_party_actions_use_full_commit_ids_with_minimal_permissions_and_dependabot() {
    for name in [
        "quality.yml",
        "messrust.yml",
        "security.yml",
        "mutation.yml",
        "docs.yml",
    ] {
        let workflow = read_repo_file(&format!(".github/workflows/{name}"));
        assert!(
            workflow.contains("permissions:") && workflow.contains("contents:"),
            "{name} must declare token permissions"
        );
        for line in workflow.lines() {
            let trimmed = line.trim();
            if let Some(action) = trimmed.strip_prefix("uses: ") {
                let action = action.split_whitespace().next().expect("action ref");
                let sha = action
                    .rsplit_once('@')
                    .map(|(_, pin)| pin)
                    .expect("third-party actions must pin a ref");
                assert!(
                    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
                    "{name} must pin third-party actions to full commit identifiers: {action}"
                );
            }
        }
    }

    let dependabot = read_repo_file(".github/dependabot.yml");
    assert!(
        dependabot.contains("package-ecosystem: cargo")
            && dependabot.contains("package-ecosystem: github-actions")
            && dependabot.contains("interval: weekly")
            && dependabot.contains("groups:"),
        "weekly grouped updates must cover Cargo and GitHub Actions"
    );
}

#[test]
fn docs_workflow_builds_user_guides_and_publishes_from_main() {
    let workflow = read_repo_file(".github/workflows/docs.yml");
    assert!(
        workflow.contains("mkdocs build --strict"),
        "docs CI must build the user documentation site in strict mode"
    );
    assert!(
        workflow.contains("mkdocs gh-deploy --force --strict"),
        "docs CI must publish only after a successful strict build"
    );
    assert!(
        workflow.contains("branches: [main]")
            && workflow.contains("needs: build")
            && workflow.contains(r#"github.ref == 'refs/heads/main'"#),
        "documentation publication must run from main after the build job"
    );
}

#[test]
fn user_guides_cover_the_release_documentation_set() {
    let required = [
        ("docs/install.md", "Install"),
        ("docs/quickstart.md", "Quick Start"),
        ("docs/cli.md", "Command"),
        ("docs/config.md", "config"),
        ("docs/mutators.md", "mutator"),
        ("docs/json-outputs.md", "report"),
        ("docs/custom-mutators.md", "custom"),
        ("docs/ci.md", "CI"),
        ("docs/release.md", "Release"),
        ("docs/glossary.md", "Mutation"),
    ];
    for (path, marker) in required {
        let body = read_repo_file(path);
        assert!(
            body.to_ascii_lowercase()
                .contains(&marker.to_ascii_lowercase()),
            "{path} must cover {marker}"
        );
    }
    assert!(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("mkdocs.yml")
            .is_file(),
        "mkdocs.yml must publish the user guide set"
    );
}

#[test]
fn optional_hooks_mirror_fast_ci_and_are_not_activated_automatically() {
    let pre_commit = read_repo_file("githooks/pre-commit");
    let pre_push = read_repo_file("githooks/pre-push");

    assert!(
        pre_commit.contains("cargo fmt")
            && pre_commit.contains("cargo clippy")
            && pre_commit.contains("messrust")
            && pre_commit.contains("--strict"),
        "pre-commit must mirror the fast CI format, Clippy, and messrust controls"
    );
    assert!(
        pre_push.contains("--git-diff-lines")
            && pre_push.contains("--git-diff-base origin/main")
            && pre_push.contains("--min-msi 75")
            && pre_push.contains("--min-covered-msi 80"),
        "pre-push must mirror changed-line self-mutation gates"
    );
    assert!(
        pre_commit.contains("git config core.hooksPath githooks")
            && pre_push.contains("git config core.hooksPath githooks"),
        "hooks must document manual activation"
    );

    let pre_commit_path = repo_path("githooks/pre-commit");
    let pre_push_path = repo_path("githooks/pre-push");
    assert!(
        is_executable(&pre_commit_path),
        "pre-commit must be executable"
    );
    assert!(is_executable(&pre_push_path), "pre-push must be executable");

    let git_config = read_repo_file(".git/config");
    assert!(
        !git_config.contains("hooksPath = githooks") && !git_config.contains("hooksPath=githooks"),
        "repository hooks must not activate automatically"
    );
    assert!(
        !repo_path(".git/hooks/pre-commit").is_file()
            || !fs::read_to_string(repo_path(".git/hooks/pre-commit"))
                .unwrap_or_default()
                .contains("messrust"),
        "Git must not install the optional pre-commit hook by default"
    );
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_path(relative)).unwrap_or_else(|_| {
        panic!("{relative} must exist for release CI, documentation, and optional hooks")
    })
}

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
