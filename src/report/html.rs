use std::collections::BTreeMap;
use std::fs;

use crate::{MutationResult, MutationRun, MutationState};

use super::{ReportMutatorStats, compact_summary, mutator_stats};

/// File name for the HTML mutation report.
pub const HTML_REPORT_FILE_NAME: &str = "mutarust-report.html";

/// Builds a self-contained HTML report from a completed run.
pub fn html_report(run: &MutationRun) -> String {
    let stats = compact_summary(run);
    let mutator_stats = mutator_stats(run.mutator_summaries());
    let grouped = group_escaped_mutants(run);
    let msi_percent = (stats.msi * 10_000.0).round() / 100.0;
    let covered_percent = (stats.covered_code_msi * 10_000.0).round() / 100.0;
    let mut body = String::new();
    body.push_str(HTML_HEAD);
    body.push_str(
        r#"<div class="container">
        <div class="header">
            <h1>Mutation Testing Report</h1>
            <p>Detailed analysis of code mutation results</p>
        </div>
        <div class="stats-grid">"#,
    );
    push_stat_card(
        &mut body,
        "total",
        &stats.total_mutants_count.to_string(),
        "Total Mutants",
    );
    push_stat_card(
        &mut body,
        "killed",
        &stats.killed_count.to_string(),
        "Killed",
    );
    push_stat_card(
        &mut body,
        "escaped",
        &stats.escaped_count.to_string(),
        "Escaped",
    );
    push_stat_card(
        &mut body,
        "errored",
        &stats.error_count.to_string(),
        "Errored",
    );
    push_stat_card(
        &mut body,
        "not-covered",
        &stats.not_covered_count.to_string(),
        "Not Covered",
    );
    push_stat_card(
        &mut body,
        "skipped",
        &stats.skipped_count.to_string(),
        "Skipped",
    );
    push_stat_card(&mut body, "msi", &format!("{msi_percent}%"), "MSI");
    push_stat_card(
        &mut body,
        "covered",
        &format!("{covered_percent}%"),
        "Covered MSI",
    );
    body.push_str("</div>");
    push_mutator_table(&mut body, &mutator_stats);
    body.push_str(
        r#"<div class="warning-box">
            <div class="warning-box-header"><span>Important Notice</span></div>
            <div class="warning-box-content">
                This report displays only <strong>escaped mutants</strong> that require attention.
                Killed mutants are not shown in detail.
            </div>
        </div>
        <div class="controls">
            <button class="control-btn" onclick="expandAll()">Expand All Files</button>
            <button class="control-btn" onclick="collapseAll()">Collapse All Files</button>
        </div>"#,
    );
    if grouped.is_empty() {
        body.push_str(r#"<div class="empty">No escaped mutants.</div>"#);
    } else {
        for (file_path, mutants) in grouped {
            push_file_section(&mut body, &file_path, &mutants);
        }
    }
    body.push_str("</div>");
    body.push_str(HTML_SCRIPT);
    body.push_str("</body>\n</html>\n");
    body
}

/// Writes the HTML report when enabled.
pub fn write_html_report(run: &MutationRun) -> Result<(), String> {
    fs::write(HTML_REPORT_FILE_NAME, html_report(run))
        .map_err(|error| format!("could not write {HTML_REPORT_FILE_NAME}: {error}"))
}

fn group_escaped_mutants(run: &MutationRun) -> BTreeMap<String, Vec<&MutationResult>> {
    let mut grouped = BTreeMap::new();
    for result in run.results() {
        if result.state != MutationState::Escaped {
            continue;
        }
        let path = result.source.to_string_lossy().replace('\\', "/");
        grouped.entry(path).or_insert_with(Vec::new).push(result);
    }
    grouped
}

fn push_stat_card(body: &mut String, class_name: &str, value: &str, label: &str) {
    body.push_str(&format!(
        r#"<div class="stat-card {class_name}"><div class="stat-value">{value}</div><div class="stat-label">{label}</div></div>"#
    ));
}

fn push_mutator_table(body: &mut String, stats: &[ReportMutatorStats]) {
    body.push_str(
        r#"<div class="mutator-table-wrap"><h2>Per-mutator results</h2><table class="mutator-table"><thead><tr><th>Mutator</th><th>Killed</th><th>Escaped</th><th>Skipped</th><th>Total</th></tr></thead><tbody>"#,
    );
    if stats.is_empty() {
        body.push_str(r#"<tr><td colspan="5">No tested mutants.</td></tr>"#);
    } else {
        for entry in stats {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&entry.name),
                entry.killed,
                entry.escaped,
                entry.skipped,
                entry.total
            ));
        }
    }
    body.push_str("</tbody></table></div>");
}

fn push_file_section(body: &mut String, file_path: &str, mutants: &[&MutationResult]) {
    body.push_str(&format!(
        r#"<div class="file"><div class="file-header" onclick="toggleFile(this)"><div class="file-header-info"><span>{}</span><span class="mutator-count">{} mutants</span></div></div><div class="file-content">"#,
        escape_html(file_path),
        mutants.len()
    ));
    for mutant in mutants {
        body.push_str(&format!(
            r#"<div class="mutator"><div class="mutator-header" onclick="toggleMutator(this)"><span>Mutator: {} (line {})</span></div><div class="mutator-content"><div class="diff"><h3>Diff:</h3><div class="diff-content">"#,
            escape_html(&mutant.mutator),
            mutant.line
        ));
        for line in mutant.diff.lines() {
            let class = if line.starts_with('-') && !line.starts_with("---") {
                "removed"
            } else if line.starts_with('+') && !line.starts_with("+++") {
                "added"
            } else {
                "unchanged"
            };
            body.push_str(&format!(
                r#"<div class="diff-line {class}">{}</div>"#,
                escape_html(line)
            ));
        }
        body.push_str("</div></div></div></div>");
    }
    body.push_str("</div></div>");
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

const HTML_HEAD: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Mutation Testing Report</title>
    <style>
        :root {
            --theme-palette-green800: #02d15c;
            --theme-palette-blue800: #0099f7;
            --theme-palette-red600: #ff4053;
            --theme-palette-violet600: #965eeb;
            --theme-palette-gray50: #f9fafb;
            --theme-palette-gray100: #f3f4f6;
            --theme-palette-gray200: #e5e7eb;
            --theme-palette-gray600: #4b5563;
            --theme-palette-gray800: #1f2937;
            --theme-palette-yellow500: #f59e0b;
            --border-radius: 8px;
            --box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
            --transition: all 0.3s ease;
        }
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', system-ui, -apple-system, sans-serif;
            background: linear-gradient(135deg, var(--theme-palette-gray100), var(--theme-palette-gray50));
            background-attachment: fixed;
            color: var(--theme-palette-gray800);
            line-height: 1.6;
            padding: 20px;
        }
        .container { max-width: 1200px; margin: 0 auto; }
        .header {
            text-align: center;
            margin-bottom: 30px;
            padding: 30px;
            background: white;
            border-radius: var(--border-radius);
            box-shadow: var(--box-shadow);
            border-left: 4px solid var(--theme-palette-violet600);
        }
        .header h1 { font-size: 2.2rem; margin-bottom: 10px; color: var(--theme-palette-violet600); }
        .stats-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
            gap: 15px;
            margin-bottom: 30px;
        }
        .stat-card {
            background: white;
            padding: 20px;
            border-radius: var(--border-radius);
            box-shadow: var(--box-shadow);
            text-align: center;
            transition: var(--transition);
            border-top: 3px solid var(--theme-palette-blue800);
        }
        .stat-card.killed { border-top-color: var(--theme-palette-green800); }
        .stat-card.escaped { border-top-color: var(--theme-palette-red600); }
        .stat-card.msi, .stat-card.covered { border-top-color: var(--theme-palette-violet600); }
        .stat-value { font-size: 2rem; font-weight: bold; margin: 10px 0; }
        .stat-label { font-size: 1rem; color: var(--theme-palette-gray600); font-weight: 500; }
        .mutator-table-wrap {
            background: white;
            border-radius: var(--border-radius);
            box-shadow: var(--box-shadow);
            padding: 20px;
            margin-bottom: 25px;
        }
        .mutator-table-wrap h2 { margin-bottom: 12px; font-size: 1.2rem; }
        .mutator-table { width: 100%; border-collapse: collapse; }
        .mutator-table th, .mutator-table td {
            border-bottom: 1px solid var(--theme-palette-gray200);
            padding: 8px 10px;
            text-align: left;
        }
        .warning-box {
            background: linear-gradient(90deg, #fff9e6, #fff5d6);
            border: 1px solid #fde68a;
            border-radius: var(--border-radius);
            padding: 20px;
            margin-bottom: 25px;
            box-shadow: var(--box-shadow);
            border-left: 4px solid var(--theme-palette-yellow500);
        }
        .warning-box-header { margin-bottom: 10px; color: #92400e; font-weight: 600; }
        .warning-box-content { color: #92400e; }
        .empty {
            background: white;
            padding: 20px;
            border-radius: var(--border-radius);
            box-shadow: var(--box-shadow);
            text-align: center;
        }
        .file {
            background: white;
            margin-bottom: 15px;
            border-radius: var(--border-radius);
            box-shadow: var(--box-shadow);
            overflow: hidden;
        }
        .file-header {
            background: linear-gradient(90deg, var(--theme-palette-gray100), var(--theme-palette-gray50));
            padding: 15px 20px;
            font-size: 1.1rem;
            font-weight: 600;
            cursor: pointer;
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid var(--theme-palette-gray200);
        }
        .file-header-info { display: flex; align-items: center; gap: 10px; }
        .file-header::after { content: "▼"; font-size: 0.8rem; color: var(--theme-palette-violet600); }
        .file.expanded .file-header::after { transform: rotate(180deg); }
        .file-content { max-height: 0; overflow: hidden; }
        .file.expanded .file-content { max-height: 10000px; padding: 15px 20px; }
        .mutator { margin-bottom: 15px; border: 1px solid var(--theme-palette-gray200); border-radius: 6px; overflow: hidden; }
        .mutator-header {
            padding: 12px 15px;
            background: white;
            font-weight: 500;
            cursor: pointer;
        }
        .mutator-header::after { content: "▶"; float: right; font-size: 0.7rem; color: var(--theme-palette-blue800); }
        .mutator.expanded .mutator-header::after { transform: rotate(90deg); display: inline-block; }
        .mutator-content { max-height: 0; overflow: hidden; background: var(--theme-palette-gray50); }
        .mutator.expanded .mutator-content { max-height: 2000px; padding: 15px; }
        .diff h3 { margin: 0 0 10px 0; font-size: 0.95rem; }
        .diff-content {
            background: white;
            border-radius: 4px;
            border: 1px solid var(--theme-palette-gray200);
            overflow: hidden;
        }
        .diff-line {
            font-family: 'Monaco', 'Consolas', monospace;
            font-size: 12px;
            padding: 3px 10px;
            white-space: pre;
        }
        .diff-line.removed { background-color: rgba(255, 64, 83, 0.1); color: var(--theme-palette-red600); }
        .diff-line.added { background-color: rgba(2, 209, 92, 0.1); color: var(--theme-palette-green800); }
        .controls { text-align: center; margin-bottom: 20px; }
        .control-btn {
            background: var(--theme-palette-violet600);
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 6px;
            cursor: pointer;
            font-size: 14px;
            margin: 0 5px;
        }
        .mutator-count {
            background: var(--theme-palette-gray200);
            color: var(--theme-palette-gray600);
            padding: 3px 10px;
            border-radius: 15px;
            font-size: 0.85rem;
            font-weight: 600;
        }
    </style>
</head>
<body>
"#;

const HTML_SCRIPT: &str = r#"
<script>
function toggleFile(element) { element.parentElement.classList.toggle('expanded'); }
function toggleMutator(element) { element.parentElement.classList.toggle('expanded'); }
function expandAll() {
  document.querySelectorAll('.file').forEach(file => {
    file.classList.add('expanded');
    file.querySelectorAll('.mutator').forEach(mutator => mutator.classList.add('expanded'));
  });
}
function collapseAll() {
  document.querySelectorAll('.file').forEach(file => {
    file.classList.remove('expanded');
    file.querySelectorAll('.mutator').forEach(mutator => mutator.classList.remove('expanded'));
  });
}
</script>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MutationResult;
    use std::path::PathBuf;

    #[test]
    fn html_report_shows_counts_scores_mutators_and_escaped_evidence() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "src/lib.rs", 1, "id-killed"),
                mutant(MutationState::Escaped, "src/lib.rs", 2, "id-escaped"),
            ],
            true,
        );
        let html = html_report(&run);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Total Mutants"));
        assert!(html.contains(">2<"));
        assert!(html.contains("Killed"));
        assert!(html.contains("Escaped"));
        assert!(html.contains("MSI"));
        assert!(html.contains("Covered MSI"));
        assert!(html.contains("Per-mutator results"));
        assert!(html.contains("conditional/bool-literal"));
        assert!(html.contains("src/lib.rs"));
        assert!(html.contains("Diff:"));
        assert!(html.contains("let value = true;"));
        assert!(!html.contains("http://") && !html.contains("https://"));
    }

    #[test]
    fn empty_html_report_is_self_contained() {
        let html = html_report(&MutationRun::for_test(Vec::new(), false));
        assert!(html.contains("No escaped mutants."));
        assert!(html.contains("Total Mutants"));
        assert!(html.contains("0%"));
        assert!(!html.contains("<script src="));
        assert!(!html.contains("<link "));
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
