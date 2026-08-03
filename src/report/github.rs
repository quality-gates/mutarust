use crate::{MutationRun, MutationState};

use super::portable_path;

/// Builds GitHub Actions warning annotations for escaped mutants.
pub fn github_annotations(run: &MutationRun) -> String {
    let mut output = String::new();
    for result in run.results() {
        if result.state != MutationState::Escaped {
            continue;
        }
        let path = portable_path(&result.source);
        let title = format!("Mutant escaped ({})", result.mutator);
        let message = format!(
            "Escaped mutation at {path}:{} — add a test to kill it",
            result.line
        );
        output.push_str(&format!(
            "::warning file={},line={},title={}::{}\n",
            escape_property(&path),
            result.line,
            escape_property(&title),
            escape_data(&message)
        ));
    }
    output
}

fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_property(value: &str) -> String {
    escape_data(value).replace(':', "%3A").replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MutationResult, MutationState};
    use std::path::PathBuf;

    #[test]
    fn annotations_use_relative_paths_and_escape_special_characters() {
        let run = MutationRun::for_test(
            vec![
                mutant(MutationState::Killed, "checked/src/lib.rs", 1),
                mutant(MutationState::Escaped, "checked/src/a%b,c:d.rs", 2),
            ],
            false,
        );
        let annotations = github_annotations(&run);
        assert_eq!(
            annotations,
            "::warning file=checked/src/a%25b%2Cc%3Ad.rs,line=2,title=Mutant escaped (conditional/bool-literal)::Escaped mutation at checked/src/a%25b,c:d.rs:2 — add a test to kill it\n"
        );
    }

    #[test]
    fn empty_run_emits_no_annotations() {
        assert!(github_annotations(&MutationRun::for_test(Vec::new(), false)).is_empty());
    }

    fn mutant(state: MutationState, source: &str, line: usize) -> MutationResult {
        MutationResult {
            source: PathBuf::from(source),
            stable_id: "a".repeat(32),
            line,
            mutator: "conditional/bool-literal".to_owned(),
            diff: String::new(),
            state,
            error: None,
        }
    }
}
