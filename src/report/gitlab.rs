use std::fs;

use serde::Serialize;

use crate::{MutationRun, MutationState};

use super::portable_path;

/// File name for the GitLab Code Quality report.
pub const GITLAB_REPORT_FILE_NAME: &str = "mutarust-gitlab.json";

/// Builds the GitLab Code Quality document for escaped mutants.
pub fn gitlab_report(run: &MutationRun) -> Vec<GitLabIssue> {
    run.results()
        .iter()
        .filter(|result| result.state == MutationState::Escaped)
        .map(|result| {
            let path = portable_path(&result.source);
            GitLabIssue {
                kind: "issue",
                check_name: result.mutator.clone(),
                description: format!(
                    "Escaped mutant ({}) at {path}:{} — no test kills this mutation",
                    result.mutator, result.line
                ),
                severity: "minor",
                fingerprint: result.stable_id.clone(),
                location: GitLabLocation {
                    path,
                    lines: GitLabLines { begin: result.line },
                },
            }
        })
        .collect()
}

/// Writes the GitLab Code Quality report when enabled.
pub fn write_gitlab_report(run: &MutationRun) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&gitlab_report(run))
        .map_err(|error| format!("could not write {GITLAB_REPORT_FILE_NAME}: {error}"))?;
    fs::write(GITLAB_REPORT_FILE_NAME, text)
        .map_err(|error| format!("could not write {GITLAB_REPORT_FILE_NAME}: {error}"))
}

/// One GitLab Code Quality finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitLabIssue {
    /// Finding type. Always `issue`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Stable mutator name used as the check name.
    pub check_name: String,
    /// Human-readable description of the escaped mutant.
    pub description: String,
    /// Severity. Always `minor`.
    pub severity: &'static str,
    /// Stable mutant ID used as the fingerprint.
    pub fingerprint: String,
    /// Source location of the escaped mutant.
    pub location: GitLabLocation,
}

/// Source location for a GitLab finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitLabLocation {
    /// Repository-relative source path.
    pub path: String,
    /// One-based line range.
    pub lines: GitLabLines,
}

/// Line range for a GitLab finding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GitLabLines {
    /// One-based start line.
    pub begin: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MutationResult;
    use std::path::PathBuf;

    #[test]
    fn report_uses_stable_ids_and_relative_paths() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "checked/src/lib.rs", 1, "killed-id"),
                mutant(
                    MutationState::Escaped,
                    "checked/src/lib.rs",
                    2,
                    "4582b234c128077507b7558eb62c337e",
                ),
            ],
            false,
        );
        let report = gitlab_report(&run);
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].kind, "issue");
        assert_eq!(report[0].check_name, "conditional/bool-literal");
        assert_eq!(report[0].severity, "minor");
        assert_eq!(report[0].fingerprint, "4582b234c128077507b7558eb62c337e");
        assert_eq!(report[0].location.path, "checked/src/lib.rs");
        assert_eq!(report[0].location.lines.begin, 2);
        assert!(
            report[0]
                .description
                .contains("Escaped mutant (conditional/bool-literal) at checked/src/lib.rs:2")
        );
    }

    #[test]
    fn empty_run_writes_an_empty_array() {
        assert!(gitlab_report(&MutationRun::for_test(Vec::new(), false)).is_empty());
    }

    fn mutant(state: MutationState, source: &str, line: usize, id: &str) -> MutationResult {
        MutationResult {
            source: PathBuf::from(source),
            stable_id: id.to_owned(),
            line,
            mutator: "conditional/bool-literal".to_owned(),
            diff: String::new(),
            state,
            error: None,
        }
    }
}
