use crate::report::format_percentage;
use crate::{Baseline, MutationRun};

/// Score and baseline requirements for a completed mutation run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Gates {
    /// The minimum mutation score percentage.
    pub minimum_mutation_score: Option<u8>,
    /// The minimum covered-code mutation score percentage.
    pub minimum_covered_mutation_score: Option<u8>,
    /// Fails when the run has escaped mutants outside the baseline.
    pub fail_on_escaped: bool,
    /// Passes score gates when the run has no mutations.
    pub pass_score_gates_when_no_mutations: bool,
}

/// The result of judging a completed mutation run against its gates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    message: Option<String>,
}

impl RunOutcome {
    /// Returns true when a gate fails.
    pub fn is_failure(&self) -> bool {
        self.message.is_some()
    }

    /// Returns the first gate failure message, if a gate fails.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    fn success() -> Self {
        Self { message: None }
    }

    fn failure(message: String) -> Self {
        Self {
            message: Some(message),
        }
    }
}

/// Judges a mutation run against score requirements and an escaped-mutant baseline.
pub fn judge(run: &MutationRun, baseline: &Baseline, gates: &Gates) -> RunOutcome {
    if gates.pass_score_gates_when_no_mutations && run.total() == 0 {
        return RunOutcome::success();
    }
    total_score_failure(run, gates)
        .or_else(|| covered_score_failure(run, gates))
        .or_else(|| escaped_mutant_failure(run, baseline, gates))
        .unwrap_or_else(RunOutcome::success)
}

fn total_score_failure(run: &MutationRun, gates: &Gates) -> Option<RunOutcome> {
    let minimum = gates.minimum_mutation_score?;
    let score = run.mutation_score();
    (score * 100.0 < f64::from(minimum)).then(|| {
        RunOutcome::failure(format!(
            "mutation score {} is below the required {minimum}%",
            format_percentage(score)
        ))
    })
}

fn covered_score_failure(run: &MutationRun, gates: &Gates) -> Option<RunOutcome> {
    let minimum = gates
        .minimum_covered_mutation_score
        .filter(|minimum| *minimum > 0)?;
    if !run.has_coverage() {
        return Some(RunOutcome::failure(
            "covered-code mutation score requires --coverage".into(),
        ));
    }
    let score = run.covered_mutation_score();
    (score * 100.0 < f64::from(minimum)).then(|| {
        RunOutcome::failure(format!(
            "covered-code mutation score {} is below the required {minimum}%",
            format_percentage(score)
        ))
    })
}

fn escaped_mutant_failure(
    run: &MutationRun,
    baseline: &Baseline,
    gates: &Gates,
) -> Option<RunOutcome> {
    if !gates.fail_on_escaped {
        return None;
    }
    let count = baseline.new_escaped_count(run);
    (count > 0).then(|| {
        RunOutcome::failure(format!(
            "{count} new mutant(s) escaped — kill them or run --update-baseline to accept"
        ))
    })
}
