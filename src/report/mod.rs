mod agentic;
mod github;
mod gitlab;
mod hints;
mod html;

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::{MutationResult, MutationRun, MutationState, MutatorSummary, VERSION};

pub use agentic::{
    AGENTIC_REMINDER, AGENTIC_REPORT_FILE_NAME, AgenticMutant, AgenticReport, AgenticReportInput,
    agentic_report, write_agentic_report,
};
pub use github::github_annotations;
pub use gitlab::{
    GITLAB_REPORT_FILE_NAME, GitLabIssue, GitLabLines, GitLabLocation, gitlab_report,
    write_gitlab_report,
};
pub use html::{HTML_REPORT_FILE_NAME, html_report, write_html_report};

/// File name for the full JSON mutation report.
pub const FULL_REPORT_FILE_NAME: &str = "report.json";

/// File name for the compact JSON summary.
pub const COMPACT_SUMMARY_FILE_NAME: &str = "mutarust-summary.json";

/// Writes the full JSON report when enabled.
pub fn write_full_report(run: &MutationRun, context: &ReportContext) -> Result<(), String> {
    write_json(FULL_REPORT_FILE_NAME, &full_report(run, context))
}

/// Writes the compact JSON summary when enabled.
pub fn write_compact_summary(run: &MutationRun) -> Result<(), String> {
    write_json(COMPACT_SUMMARY_FILE_NAME, &compact_summary(run))
}

/// Context that shapes documented report forms.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportContext {
    /// True when the run selected one mutant by stable ID.
    pub one_mutant: bool,
}

/// Full mutation report for downstream programs.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullReport {
    /// Run metadata for empty, coverage, and one-mutant forms.
    pub metadata: ReportMetadata,
    /// Aggregate counts and scores.
    pub stats: ReportStats,
    /// Sorted per-mutator counts for tested mutants.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mutator_stats: Vec<ReportMutatorStats>,
    /// Escaped mutants.
    pub escaped: Vec<ReportMutant>,
    /// Killed mutants.
    pub killed: Vec<ReportMutant>,
    /// Skipped mutants.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<ReportMutant>,
    /// Errored mutants.
    pub errored: Vec<ReportMutant>,
    /// Not-covered mutants.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub not_covered: Vec<ReportMutant>,
    /// Generated mutants from dry-run or no-exec modes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<ReportMutant>,
}

/// Compact summary for dashboards and badges.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStats {
    /// Total mutants in the run.
    pub total_mutants_count: u64,
    /// Killed mutants.
    pub killed_count: u64,
    /// Not-covered mutants.
    pub not_covered_count: u64,
    /// Escaped mutants.
    pub escaped_count: u64,
    /// Errored mutants.
    pub error_count: u64,
    /// Skipped mutants.
    pub skipped_count: u64,
    /// Mutation score from zero to one.
    pub msi: f64,
    /// Covered-code mutation score from zero to one.
    pub covered_code_msi: f64,
}

/// Per-mutator counts for tested mutants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMutatorStats {
    /// Stable mutator name.
    pub name: String,
    /// Killed and errored mutants for this mutator.
    pub killed: u64,
    /// Escaped mutants for this mutator.
    pub escaped: u64,
    /// Skipped mutants for this mutator.
    pub skipped: u64,
    /// Tested mutants for this mutator.
    pub total: u64,
}

/// One mutant candidate and its result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMutant {
    /// Stable mutant ID.
    pub id: String,
    /// Mutator identity and source position.
    pub mutator: ReportMutator,
    /// Unified source diff.
    pub diff: String,
    /// Optional process or error detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_output: Option<String>,
}

/// Mutator identity for one candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMutator {
    /// Stable mutator name.
    pub mutator_name: String,
    /// Repository-relative source path.
    pub original_file_path: String,
    /// One-based source line of the mutation.
    pub original_start_line: u64,
}

/// Stable run metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportMetadata {
    /// Mutarust package version.
    pub version: String,
    /// True when normal coverage shaped the run.
    pub has_coverage: bool,
    /// True when the run selected one mutant by stable ID.
    pub one_mutant: bool,
}

/// Builds the full report document from a completed run.
pub fn full_report(run: &MutationRun, context: &ReportContext) -> FullReport {
    let mut escaped = Vec::new();
    let mut killed = Vec::new();
    let mut skipped = Vec::new();
    let mut errored = Vec::new();
    let mut not_covered = Vec::new();
    let mut generated = Vec::new();
    for result in run.results() {
        let mutant = report_mutant(result);
        match result.state {
            MutationState::Escaped => escaped.push(mutant),
            MutationState::Killed => killed.push(mutant),
            MutationState::Skipped => skipped.push(mutant),
            MutationState::Errored => errored.push(mutant),
            MutationState::NotCovered => not_covered.push(mutant),
            MutationState::Generated => generated.push(mutant),
        }
    }
    FullReport {
        metadata: ReportMetadata {
            version: VERSION.to_owned(),
            has_coverage: run.has_coverage(),
            one_mutant: context.one_mutant,
        },
        stats: compact_summary(run),
        mutator_stats: mutator_stats(run.mutator_summaries()),
        escaped,
        killed,
        skipped,
        errored,
        not_covered,
        generated,
    }
}

/// Builds the compact summary document from a completed run.
pub fn compact_summary(run: &MutationRun) -> ReportStats {
    ReportStats {
        total_mutants_count: run.total() as u64,
        killed_count: run.killed() as u64,
        not_covered_count: run.not_covered() as u64,
        escaped_count: run.escaped() as u64,
        error_count: run.errored() as u64,
        skipped_count: run.skipped() as u64,
        msi: run.mutation_score(),
        covered_code_msi: run.covered_mutation_score(),
    }
}

pub(super) fn mutator_stats(summaries: Vec<MutatorSummary>) -> Vec<ReportMutatorStats> {
    summaries
        .into_iter()
        .map(|summary| ReportMutatorStats {
            name: summary.mutator,
            killed: summary.killed as u64,
            escaped: summary.escaped as u64,
            skipped: summary.skipped as u64,
            total: summary.total as u64,
        })
        .collect()
}

fn report_mutant(result: &MutationResult) -> ReportMutant {
    ReportMutant {
        id: result.stable_id.clone(),
        mutator: ReportMutator {
            mutator_name: result.mutator.clone(),
            original_file_path: portable_path(&result.source),
            original_start_line: result.line as u64,
        },
        diff: result.diff.clone(),
        process_output: result.error.clone(),
    }
}

fn write_json(file_name: &str, value: &impl Serialize) -> Result<(), String> {
    let text = serde_json::to_string(value)
        .map_err(|error| format!("could not write {file_name}: {error}"))?;
    fs::write(file_name, text).map_err(|error| format!("could not write {file_name}: {error}"))
}

pub(super) fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MutationResult;
    use std::path::PathBuf;

    #[test]
    fn full_report_uses_ratios_ids_and_relative_paths() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "checked/src/lib.rs", 1, "id-killed"),
                mutant(
                    MutationState::Escaped,
                    "checked/src/lib.rs",
                    2,
                    "id-escaped",
                ),
            ],
            false,
        );
        let report = full_report(&run, &ReportContext::default());
        let json = serde_json::to_value(&report).expect("report must serialize");
        assert_eq!(json["stats"]["totalMutantsCount"], 2);
        assert_eq!(json["stats"]["killedCount"], 1);
        assert_eq!(json["stats"]["escapedCount"], 1);
        assert_eq!(json["stats"]["msi"], 0.5);
        assert_eq!(json["stats"]["coveredCodeMsi"], 0.0);
        assert_eq!(json["metadata"]["hasCoverage"], false);
        assert_eq!(json["metadata"]["oneMutant"], false);
        assert_eq!(json["metadata"]["version"], VERSION);
        assert_eq!(json["killed"][0]["id"], "id-killed");
        assert_eq!(
            json["killed"][0]["mutator"]["originalFilePath"],
            "checked/src/lib.rs"
        );
        assert_eq!(json["killed"][0]["mutator"]["originalStartLine"], 1);
        assert_eq!(
            json["escaped"][0]["mutator"]["mutatorName"],
            "conditional/bool-literal"
        );
        assert!(json["escaped"][0]["diff"].as_str().unwrap().contains("---"));
        assert!(json.get("notCovered").is_none());
        assert!(json.get("generated").is_none());
    }

    #[test]
    fn empty_coverage_and_one_mutant_forms_are_stable() {
        let empty = full_report(
            &MutationRun::for_test(Vec::new(), false),
            &ReportContext::default(),
        );
        assert_eq!(empty.stats.total_mutants_count, 0);
        assert_eq!(empty.stats.msi, 0.0);
        assert!(!empty.metadata.has_coverage);
        assert!(!empty.metadata.one_mutant);
        assert!(empty.escaped.is_empty());
        assert!(empty.killed.is_empty());
        assert!(empty.errored.is_empty());

        let covered = full_report(
            &MutationRun::for_test(
                vec![mutant(
                    MutationState::NotCovered,
                    "src/lib.rs",
                    3,
                    "id-not-covered",
                )],
                true,
            ),
            &ReportContext::default(),
        );
        assert!(covered.metadata.has_coverage);
        assert_eq!(covered.stats.not_covered_count, 1);
        assert_eq!(covered.stats.covered_code_msi, 0.0);
        assert_eq!(covered.not_covered[0].id, "id-not-covered");

        let one = full_report(
            &MutationRun::for_test(
                vec![mutant(MutationState::Escaped, "src/lib.rs", 4, "id-one")],
                false,
            ),
            &ReportContext { one_mutant: true },
        );
        assert!(one.metadata.one_mutant);
        assert_eq!(one.stats.total_mutants_count, 1);
        assert_eq!(one.escaped.len(), 1);
    }

    #[test]
    fn compact_summary_uses_ratios_from_zero_to_one() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "a.rs", 1, "a"),
                mutant(MutationState::Escaped, "a.rs", 2, "b"),
                mutant(MutationState::Errored, "a.rs", 3, "c"),
                mutant(MutationState::Skipped, "a.rs", 4, "d"),
            ],
            true,
        );
        let summary = compact_summary(&run);
        let json = serde_json::to_value(&summary).expect("summary must serialize");
        assert_eq!(json["totalMutantsCount"], 4);
        assert_eq!(json["killedCount"], 1);
        assert_eq!(json["escapedCount"], 1);
        assert_eq!(json["errorCount"], 1);
        assert_eq!(json["skippedCount"], 1);
        assert_eq!(json["msi"], 0.75);
        assert_eq!(json["coveredCodeMsi"], 0.75);
        assert!(json.get("timeOutCount").is_none());
    }

    fn mutant(state: MutationState, source: &str, line: usize, id: &str) -> MutationResult {
        MutationResult {
            source: PathBuf::from(source),
            stable_id: id.to_owned(),
            line,
            mutator: "conditional/bool-literal".to_owned(),
            diff: format!("--- {source}\n+++ {source}\n@@ -{line},1 +{line},1 @@\n"),
            state,
            error: None,
        }
    }
}
