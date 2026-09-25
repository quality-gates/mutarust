use serde::Serialize;

use crate::MutationRun;

use super::{
    Rendered, Report, ReportContext, escaped_mutants, portable_path, serialize_json_pretty,
    write_one,
};

/// File name for the GitLab Code Quality report.
pub const GITLAB_REPORT_FILE_NAME: &str = "mutarust-gitlab.json";

/// Report generator for the GitLab Code Quality report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitlabReport;

impl Report for GitlabReport {
    fn name(&self) -> &'static str {
        "gitlab"
    }

    fn render(&self, run: &MutationRun, _context: &ReportContext) -> Result<Rendered, String> {
        let issues = gitlab_report(run);
        let body = serialize_json_pretty(&issues)
            .map_err(|error| format!("could not write {GITLAB_REPORT_FILE_NAME}: {error}"))?;
        Ok(Rendered::File {
            path: std::path::PathBuf::from(GITLAB_REPORT_FILE_NAME),
            body,
        })
    }
}

/// Builds the GitLab Code Quality document for escaped mutants.
pub fn gitlab_report(run: &MutationRun) -> Vec<GitLabIssue> {
    escaped_mutants(run)
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
    write_one(&GitlabReport, run, &ReportContext::default())
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
    use crate::{MutationResult, MutationState};
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
            source_root: PathBuf::new(),
            stable_id: id.to_owned(),
            line,
            mutator: "conditional/bool-literal".to_owned(),
            diff: String::new(),
            state,
            error: None,
        }
    }
}
