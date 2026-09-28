use std::io::{self, Write};

use crate::{DisplayFilter, MutationResult, MutationRun, MutationState};

use super::format_percentage;

/// Writes the number of mutations that a dry run would generate.
pub fn write_dry_run_summary(output: &mut impl Write, run: &MutationRun) -> io::Result<()> {
    writeln!(
        output,
        "Total: {} mutation(s) would be generated. No files written, no tests run.",
        run.total()
    )
}

/// Writes generated mutation details without running tests.
pub fn write_generated_mutants(output: &mut impl Write, run: &MutationRun) -> io::Result<()> {
    for result in run.results() {
        write_result_details(output, result)?;
    }
    writeln!(output, "Generated: {}", run.total())?;
    writeln!(
        output,
        "No tests run. Generated mutations are in the mutation areas above."
    )
}

/// Writes mutation results and scores to the supplied output.
pub fn write_mutation_results(
    output: &mut impl Write,
    run: &MutationRun,
    filter: &DisplayFilter,
    no_diffs: bool,
    one_mutant: bool,
) -> io::Result<()> {
    for result in run.results() {
        if !filter.shows(result.state) {
            continue;
        }
        write_result_details(output, result)?;
        if result.state == MutationState::Escaped && !no_diffs {
            write!(output, "{}", result.diff)?;
        }
    }
    if one_mutant {
        return Ok(());
    }
    writeln!(output, "Killed: {}", run.killed())?;
    writeln!(output, "Escaped: {}", run.escaped())?;
    writeln!(output, "Errored: {}", run.errored())?;
    writeln!(output, "Not covered: {}", run.not_covered())?;
    writeln!(output, "Skipped: {}", run.skipped())?;
    writeln!(output, "Total: {}", run.total())?;
    writeln!(
        output,
        "Mutation score: {}",
        format_percentage(run.mutation_score())
    )?;
    if run.has_coverage() {
        writeln!(
            output,
            "Covered-code mutation score: {}",
            format_percentage(run.covered_mutation_score())
        )?;
    }
    writeln!(output, "Per-mutator results:")?;
    writeln!(output, "Mutator | Killed | Escaped | Skipped | Total")?;
    for summary in run.mutator_summaries() {
        writeln!(
            output,
            "{} | {} | {} | {} | {}",
            summary.mutator, summary.killed, summary.escaped, summary.skipped, summary.total
        )?;
    }
    Ok(())
}

fn write_result_details(output: &mut impl Write, result: &MutationResult) -> io::Result<()> {
    writeln!(
        output,
        "{} {} {}",
        result.state,
        result.source.display(),
        result.mutator
    )?;
    writeln!(output, "  ID: {}", result.stable_id)?;
    writeln!(
        output,
        "  Blacklist checksum: {}",
        result.blacklist_checksum
    )?;
    if let Some(detail) = &result.error {
        writeln!(output, "  {detail}")?;
    }
    Ok(())
}
