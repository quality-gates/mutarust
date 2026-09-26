use std::path::PathBuf;

use mutarust::{
    AgenticJsonReport, FullJsonReport, GithubAnnotations, GitlabReport, HtmlReport, MutationResult,
    MutationRun, MutationState, Rendered, Report, ReportContext, SummaryJsonReport, run_for_test,
    write_all,
};

fn create_test_run() -> MutationRun {
    run_for_test(
        vec![
            mutant(MutationState::Killed, "src/lib.rs", 10, "id-killed"),
            mutant(MutationState::Escaped, "src/lib.rs", 20, "id-escaped"),
            mutant(
                MutationState::NotCovered,
                "src/lib.rs",
                30,
                "id-not-covered",
            ),
        ],
        true,
    )
}

fn mutant(state: MutationState, source: &str, line: usize, id: &str) -> MutationResult {
    MutationResult {
        source: PathBuf::from(source),
        source_root: PathBuf::new(),
        stable_id: id.to_owned(),
        blacklist_checksum: id.chars().rev().collect(),
        line,
        mutator: "conditional/bool-literal".to_owned(),
        diff: format!("--- {source}\n+++ {source}\n@@ -{line},1 +{line},1 @@\n"),
        state,
        error: None,
    }
}

#[test]
fn all_six_report_formats_implement_report_and_render() {
    let run = create_test_run();
    let context = ReportContext::default();

    let reports: Vec<&dyn Report> = vec![
        &FullJsonReport,
        &HtmlReport,
        &SummaryJsonReport,
        &AgenticJsonReport,
        &GithubAnnotations,
        &GitlabReport,
    ];

    assert_eq!(reports.len(), 6);

    for report in reports {
        let rendered = report.render(&run, &context).expect("report must render");
        match rendered {
            Rendered::File { path, body } => {
                assert!(
                    !body.is_empty(),
                    "report {} body must not be empty",
                    report.name()
                );
                assert!(!path.as_os_str().is_empty());
            }
            Rendered::Stdout(body) => {
                assert_eq!(report.name(), "github");
                assert!(!body.is_empty());
                assert!(body.contains("::warning"));
            }
        }
    }
}

fn file_body(rendered: Rendered) -> String {
    match rendered {
        Rendered::File { body, .. } => body,
        Rendered::Stdout(_) => panic!("expected file rendered output"),
    }
}

#[test]
fn report_formats_reflect_consistent_counts() {
    let run = create_test_run();
    let context = ReportContext::default();

    // 1. Full report
    let full_body = file_body(FullJsonReport.render(&run, &context).unwrap());
    let full_json: serde_json::Value = serde_json::from_str(&full_body).unwrap();
    assert_eq!(full_json["stats"]["killedCount"], 1);
    assert_eq!(full_json["stats"]["escapedCount"], 1);
    assert_eq!(full_json["stats"]["notCoveredCount"], 1);
    assert_eq!(full_json["killed"].as_array().unwrap().len(), 1);
    assert_eq!(full_json["escaped"].as_array().unwrap().len(), 1);
    assert_eq!(full_json["notCovered"].as_array().unwrap().len(), 1);

    // 2. Summary JSON
    let summary_body = file_body(SummaryJsonReport.render(&run, &context).unwrap());
    let summary_json: serde_json::Value = serde_json::from_str(&summary_body).unwrap();
    assert_eq!(summary_json["killedCount"], 1);
    assert_eq!(summary_json["escapedCount"], 1);
    assert_eq!(summary_json["notCoveredCount"], 1);

    // 3. HTML report
    let html_body = file_body(HtmlReport.render(&run, &context).unwrap());
    assert!(html_body.contains("Killed"));
    assert!(html_body.contains("Escaped"));
    assert!(html_body.contains("Not covered"));
    assert!(
        html_body
            .contains(r#"<div class="stat-value">1</div><div class="stat-label">Killed</div>"#)
    );
    assert!(
        html_body
            .contains(r#"<div class="stat-value">1</div><div class="stat-label">Escaped</div>"#)
    );
    assert!(
        html_body.contains(
            r#"<div class="stat-value">1</div><div class="stat-label">Not covered</div>"#
        )
    );

    // 4. Agentic JSON
    let agentic_body = file_body(AgenticJsonReport.render(&run, &context).unwrap());
    let agentic_json: serde_json::Value = serde_json::from_str(&agentic_body).unwrap();
    assert_eq!(agentic_json["escaped_count"], 1);
    assert_eq!(agentic_json["mutants"].as_array().unwrap().len(), 1);

    // 5. GitLab JSON
    let gitlab_body = file_body(GitlabReport.render(&run, &context).unwrap());
    let gitlab_json: serde_json::Value = serde_json::from_str(&gitlab_body).unwrap();
    assert_eq!(gitlab_json.as_array().unwrap().len(), 1);
    assert!(
        gitlab_json[0]["description"]
            .as_str()
            .unwrap()
            .contains("Escaped mutant")
    );

    // 6. GitHub annotations
    let github = GithubAnnotations.render(&run, &context).unwrap();
    let Rendered::Stdout(github_body) = github else {
        panic!("expected stdout")
    };
    assert_eq!(github_body.lines().count(), 1);
    assert!(github_body.contains("Mutant escaped"));
}

struct FailingRenderReport;

impl Report for FailingRenderReport {
    fn name(&self) -> &'static str {
        "failing-render"
    }

    fn render(&self, _run: &MutationRun, _context: &ReportContext) -> Result<Rendered, String> {
        Err("intentional render failure".to_owned())
    }
}

struct FailingWriteReport;

impl Report for FailingWriteReport {
    fn name(&self) -> &'static str {
        "failing-write"
    }

    fn render(&self, _run: &MutationRun, _context: &ReportContext) -> Result<Rendered, String> {
        Ok(Rendered::File {
            path: PathBuf::from("/nonexistent/directory/unwritable-report.json"),
            body: "{}".to_owned(),
        })
    }
}

#[test]
fn write_all_isolates_failures_and_attempts_all_reports() {
    let run = create_test_run();
    let context = ReportContext::default();

    let reports: Vec<&dyn Report> = vec![
        &FailingRenderReport,
        &FailingWriteReport,
        &GithubAnnotations,
    ];

    let results = write_all(&reports, &run, &context);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_err());
    assert_eq!(
        results[0].as_ref().unwrap_err(),
        "intentional render failure"
    );
    assert!(results[1].is_err());
    assert!(results[1].as_ref().unwrap_err().contains("could not write"));
    assert!(results[2].is_ok());
}
