use std::path::Path;
use std::path::PathBuf;

use mutarust::{Baseline, Gates, MutationResult, MutationState, judge, run_for_test};

#[test]
fn zero_minimum_scores_pass_without_coverage() {
    let run = run_for_test(Vec::new(), false);
    let baseline = empty_baseline();

    let outcome = judge(
        &run,
        &baseline,
        &Gates {
            minimum_mutation_score: Some(0),
            minimum_covered_mutation_score: Some(0),
            ..Gates::default()
        },
    );

    assert!(!outcome.is_failure());
    assert_eq!(outcome.message(), None);
}

#[test]
fn score_gate_fails_below_minimum_and_passes_at_exact_threshold() {
    let below = run_for_test(vec![mutant(MutationState::Escaped, "a")], false);
    let baseline = empty_baseline();
    let failed = judge(
        &below,
        &baseline,
        &Gates {
            minimum_mutation_score: Some(1),
            ..Gates::default()
        },
    );
    assert!(failed.is_failure());
    assert_eq!(
        failed.message(),
        Some("mutation score 0.00% is below the required 1%")
    );

    let at_threshold = run_for_test(
        vec![
            mutant(MutationState::Killed, "b"),
            mutant(MutationState::Escaped, "c"),
        ],
        true,
    );
    let passed = judge(
        &at_threshold,
        &baseline,
        &Gates {
            minimum_mutation_score: Some(50),
            minimum_covered_mutation_score: Some(50),
            ..Gates::default()
        },
    );
    assert!(!passed.is_failure());
    assert_eq!(passed.message(), None);
}

fn empty_baseline() -> Baseline {
    Baseline::load(Path::new("target/run-outcome-no-baseline.json"))
        .expect("a missing baseline must mean no accepted escaped mutants")
}

fn mutant(state: MutationState, id: &str) -> MutationResult {
    MutationResult {
        source: PathBuf::from("src/lib.rs"),
        source_root: PathBuf::new(),
        stable_id: id.to_owned(),
        blacklist_checksum: id.to_owned(),
        line: 1,
        mutator: "conditional/bool-literal".to_owned(),
        diff: String::new(),
        state,
        error: None,
    }
}

#[test]
fn covered_score_minimum_requires_coverage() {
    let run = run_for_test(Vec::new(), false);
    let outcome = judge(
        &run,
        &empty_baseline(),
        &Gates {
            minimum_covered_mutation_score: Some(1),
            ..Gates::default()
        },
    );

    assert!(outcome.is_failure());
    assert_eq!(
        outcome.message(),
        Some("covered-code mutation score requires --coverage")
    );
}

#[test]
fn empty_run_fails_score_gate_unless_ignore_is_set() {
    let run = run_for_test(Vec::new(), false);
    let baseline = empty_baseline();
    let gates = Gates {
        minimum_mutation_score: Some(50),
        ..Gates::default()
    };

    let failed = judge(&run, &baseline, &gates);
    assert!(failed.is_failure());
    assert_eq!(
        failed.message(),
        Some("mutation score 0.00% is below the required 50%")
    );

    let passed = judge(
        &run,
        &baseline,
        &Gates {
            pass_score_gates_when_no_mutations: true,
            ..gates
        },
    );
    assert!(!passed.is_failure());
    assert_eq!(passed.message(), None);
}

#[test]
fn baseline_gate_accepts_known_escapes_and_fails_new_escapes() {
    let accepted_id = "a".repeat(32);
    let new_id = "b".repeat(32);
    let accepted_run = run_for_test(vec![mutant(MutationState::Escaped, &accepted_id)], false);
    let path = TempBaseline(
        std::env::temp_dir().join(format!("mutarust-run-outcome-{}.json", std::process::id())),
    );
    Baseline::write(&path.0, &accepted_run).expect("baseline must be written");
    let baseline = Baseline::load(&path.0).expect("written baseline must load");
    let gates = Gates {
        fail_on_escaped: true,
        ..Gates::default()
    };

    let accepted = judge(&accepted_run, &baseline, &gates);
    assert!(!accepted.is_failure());
    assert_eq!(accepted.message(), None);

    let run_with_new_escape = run_for_test(
        vec![
            mutant(MutationState::Escaped, &accepted_id),
            mutant(MutationState::Escaped, &new_id),
        ],
        false,
    );
    let failed = judge(&run_with_new_escape, &baseline, &gates);
    assert!(failed.is_failure());
}

struct TempBaseline(PathBuf);

impl Drop for TempBaseline {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
