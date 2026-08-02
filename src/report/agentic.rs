use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{MutationResult, MutationRun, MutationState};

use super::hints::{kill_hint, mutator_description};

/// File name for the agent-ready escaped-mutant report.
pub const AGENTIC_REPORT_FILE_NAME: &str = "mutarust-agentic.json";

/// Reminder that tells an agent how to interpret escaped mutants.
pub const AGENTIC_REMINDER: &str = "A mutant is an example of how this code could be wrong — it's not a script for the test. Don't assert on the mutant directly. Instead ask: if this code were buggy, what would a caller of the public API observe go wrong? Write a test for that.";

const CONTEXT_RADIUS: usize = 3;

/// Inputs that shape the agent-ready report.
pub struct AgenticReportInput<'a> {
    /// RFC 3339 generation time.
    pub generated_at: &'a str,
    /// Directory used to resolve repository-relative source paths.
    pub source_root: &'a Path,
}

/// Agent-ready report for escaped mutants.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgenticReport {
    /// RFC 3339 generation time.
    pub generated_at: String,
    /// Mutation score from zero to one.
    pub msi: f64,
    /// Escaped mutant count.
    pub escaped_count: usize,
    /// Interpretation reminder for agents.
    pub reminder: String,
    /// Escaped mutants with evidence.
    pub mutants: Vec<AgenticMutant>,
}

/// One escaped mutant with evidence for an agent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgenticMutant {
    /// Stable mutant ID.
    pub id: String,
    /// Repository-relative source path.
    pub file: String,
    /// One-based source line of the mutation.
    pub line: usize,
    /// Stable mutator name.
    pub mutator: String,
    /// Plain-language description of the change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Hint for a test that can kill the mutant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kill_hint: Option<String>,
    /// Unified source diff.
    pub diff: String,
    /// One-based line of the first context line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_start_line: Option<usize>,
    /// Nearby source lines.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub context_lines: Vec<String>,
    /// Nearby test file paths.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub test_files: Vec<String>,
}

/// Builds the agent-ready report from a completed run.
pub fn agentic_report(run: &MutationRun, input: &AgenticReportInput<'_>) -> AgenticReport {
    let mutants = run
        .results()
        .iter()
        .filter(|result| result.state == MutationState::Escaped)
        .map(|result| agentic_mutant(result, input.source_root))
        .collect::<Vec<_>>();
    AgenticReport {
        generated_at: input.generated_at.to_owned(),
        msi: run.mutation_score(),
        escaped_count: mutants.len(),
        reminder: AGENTIC_REMINDER.to_owned(),
        mutants,
    }
}

/// Writes the agent-ready report when enabled.
pub fn write_agentic_report(run: &MutationRun, source_root: &Path) -> Result<(), String> {
    let generated_at = utc_rfc3339_now()?;
    let report = agentic_report(
        run,
        &AgenticReportInput {
            generated_at: &generated_at,
            source_root,
        },
    );
    let text = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("could not write {AGENTIC_REPORT_FILE_NAME}: {error}"))?;
    fs::write(AGENTIC_REPORT_FILE_NAME, text)
        .map_err(|error| format!("could not write {AGENTIC_REPORT_FILE_NAME}: {error}"))
}

fn agentic_mutant(result: &MutationResult, source_root: &Path) -> AgenticMutant {
    let file = portable_path(&result.source);
    let source_path = source_root.join(&result.source);
    let source_text = fs::read_to_string(&source_path).unwrap_or_default();
    let (context_lines, context_start_line) =
        extract_context_lines(&source_text, result.line, CONTEXT_RADIUS);
    AgenticMutant {
        id: result.stable_id.clone(),
        file: file.clone(),
        line: result.line,
        mutator: result.mutator.clone(),
        description: instance_description(&result.mutator, &result.diff),
        kill_hint: kill_hint(&result.mutator).map(str::to_owned),
        diff: result.diff.clone(),
        context_start_line,
        context_lines,
        test_files: find_test_files(&result.source, source_root),
    }
}

fn instance_description(mutator: &str, diff: &str) -> Option<String> {
    let (from_lines, to_lines) = diff_changed_lines(diff);
    if let Some(description) = single_line_change_description(&from_lines, &to_lines) {
        return Some(description);
    }
    mutator_description(mutator).map(str::to_owned)
}

fn diff_changed_lines(diff: &str) -> (Vec<&str>, Vec<&str>) {
    let mut from_lines = Vec::new();
    let mut to_lines = Vec::new();
    for line in diff.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ") {
            continue;
        }
        if let Some(content) = line.strip_prefix('-') {
            from_lines.push(content);
        } else if let Some(content) = line.strip_prefix('+') {
            to_lines.push(content);
        }
    }
    (from_lines, to_lines)
}

fn single_line_change_description(from_lines: &[&str], to_lines: &[&str]) -> Option<String> {
    if from_lines.len() != 1 || to_lines.len() != 1 {
        return None;
    }
    let from = from_lines[0].trim();
    let to = to_lines[0].trim();
    if from.is_empty() || to.is_empty() || from == to {
        return None;
    }
    Some(format!("Changes: `{from}` → `{to}`"))
}

fn extract_context_lines(source: &str, line: usize, radius: usize) -> (Vec<String>, Option<usize>) {
    if source.is_empty() || line == 0 {
        return (Vec::new(), None);
    }
    let lines: Vec<&str> = source.split('\n').collect();
    let index = line.saturating_sub(1);
    if index >= lines.len() {
        return (Vec::new(), None);
    }
    let start = index.saturating_sub(radius);
    let end = (index + radius).min(lines.len().saturating_sub(1));
    let context = lines[start..=end]
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect();
    (context, Some(start + 1))
}

fn find_test_files(source: &Path, source_root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let directory = source_root.join(source.parent().unwrap_or_else(|| Path::new("")));
    push_matching_files(&directory, "_test.rs", source_root, &mut files);
    if let Some(package_tests) = package_tests_directory(source) {
        push_matching_files(
            &source_root.join(package_tests),
            ".rs",
            source_root,
            &mut files,
        );
    }
    files.sort();
    files.dedup();
    files
}

fn package_tests_directory(source: &Path) -> Option<PathBuf> {
    let mut components = source.components().collect::<Vec<_>>();
    let src_index = components.iter().position(
        |component| matches!(component, std::path::Component::Normal(name) if *name == "src"),
    )?;
    components.truncate(src_index);
    let mut package = PathBuf::new();
    for component in components {
        package.push(component);
    }
    package.push("tests");
    Some(package)
}

fn push_matching_files(
    directory: &Path,
    suffix: &str,
    source_root: &Path,
    files: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.ends_with(suffix) || !path.is_file() {
            continue;
        }
        let relative = path.strip_prefix(source_root).unwrap_or(path.as_path());
        files.push(portable_path(relative));
    }
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn utc_rfc3339_now() -> Result<String, String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("could not read system time: {error}"))?;
    let secs = elapsed.as_secs();
    let days = secs / 86_400;
    let day_secs = secs % 86_400;
    let hour = day_secs / 3_600;
    let minute = (day_secs % 3_600) / 60;
    let second = day_secs % 60;
    let (year, month, day) = civil_from_days(days as i64);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

fn civil_from_days(days: i64) -> (i64, u8, u8) {
    // Howard Hinnant civil-from-days algorithm for the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u8, d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MutationResult;
    use std::path::PathBuf;

    #[test]
    fn agentic_report_includes_generation_score_escaped_count_and_reminder() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "src/lib.rs", 1, "id-killed"),
                mutant(MutationState::Escaped, "src/lib.rs", 2, "id-escaped"),
            ],
            false,
        );
        let report = agentic_report(
            &run,
            &AgenticReportInput {
                generated_at: "2026-08-02T22:00:00Z",
                source_root: Path::new("."),
            },
        );
        assert_eq!(report.generated_at, "2026-08-02T22:00:00Z");
        assert_eq!(report.msi, 0.5);
        assert_eq!(report.escaped_count, 1);
        assert_eq!(report.reminder, AGENTIC_REMINDER);
        assert_eq!(report.mutants.len(), 1);
        assert_eq!(report.mutants[0].id, "id-escaped");
        assert_eq!(report.mutants[0].file, "src/lib.rs");
        assert_eq!(report.mutants[0].line, 2);
        assert_eq!(report.mutants[0].mutator, "conditional/bool-literal");
        assert!(
            report.mutants[0]
                .description
                .as_deref()
                .unwrap_or_default()
                .contains("Changes:")
                || report.mutants[0].description.is_some()
        );
        assert!(report.mutants[0].kill_hint.is_some());
        assert!(report.mutants[0].diff.contains("---"));
    }

    #[test]
    fn empty_agentic_report_keeps_stable_shape() {
        let report = agentic_report(
            &MutationRun::for_test(Vec::new(), false),
            &AgenticReportInput {
                generated_at: "2026-08-02T22:00:00Z",
                source_root: Path::new("."),
            },
        );
        assert_eq!(report.escaped_count, 0);
        assert_eq!(report.msi, 0.0);
        assert!(report.mutants.is_empty());
        assert_eq!(report.reminder, AGENTIC_REMINDER);
    }

    #[test]
    fn agentic_report_reads_source_context_and_nearby_tests() {
        let root = std::env::temp_dir().join(format!("mutarust-agentic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("checked/src")).expect("source dir");
        fs::create_dir_all(root.join("checked/tests")).expect("tests dir");
        fs::write(
            root.join("checked/src/lib.rs"),
            "line1\nline2\nline3\nline4\nline5\nline6\nline7\n",
        )
        .expect("source");
        fs::write(
            root.join("checked/tests/mutation.rs"),
            "#[test] fn t() {}\n",
        )
        .expect("test");
        fs::write(
            root.join("checked/src/helper_test.rs"),
            "#[test] fn u() {}\n",
        )
        .expect("unit test");

        let run = MutationRun::for_test(
            vec![mutant(
                MutationState::Escaped,
                "checked/src/lib.rs",
                4,
                "id-escaped",
            )],
            false,
        );
        let report = agentic_report(
            &run,
            &AgenticReportInput {
                generated_at: "2026-08-02T22:00:00Z",
                source_root: &root,
            },
        );
        let mutant = &report.mutants[0];
        assert_eq!(mutant.context_start_line, Some(1));
        assert_eq!(
            mutant.context_lines,
            vec![
                "line1".to_owned(),
                "line2".to_owned(),
                "line3".to_owned(),
                "line4".to_owned(),
                "line5".to_owned(),
                "line6".to_owned(),
                "line7".to_owned()
            ]
        );
        assert_eq!(
            mutant.test_files,
            vec![
                "checked/src/helper_test.rs".to_owned(),
                "checked/tests/mutation.rs".to_owned()
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    fn mutant(state: MutationState, source: &str, line: usize, id: &str) -> MutationResult {
        MutationResult {
            source: PathBuf::from(source),
            stable_id: id.to_owned(),
            line,
            mutator: "conditional/bool-literal".to_owned(),
            diff: format!(
                "--- {source}\n+++ {source}\n@@ -{line},1 +{line},1 @@\n-let value = true;\n+let value = false;\n"
            ),
            state,
            error: None,
        }
    }
}
