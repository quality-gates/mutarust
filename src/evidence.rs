use std::path::{Path, PathBuf};

use crate::Mutation;

pub(crate) struct MutationEvidence {
    pub(crate) source: PathBuf,
    pub(crate) line: usize,
    pub(crate) stable_id: StableMutantId,
    pub(crate) diff: String,
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct StableMutantId(String);

impl StableMutantId {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.len() == 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Some(Self(value.to_owned()))
        } else {
            None
        }
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn mutation_evidence(
    source_root: &Path,
    source: &Path,
    mutator: &str,
    mutation: &Mutation,
    text: &str,
) -> Result<MutationEvidence, String> {
    let source = source
        .strip_prefix(source_root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            format!(
                "path is outside the stable source root: {}",
                source.display()
            )
        })?;
    let source_name = portable_path(&source);
    let changes = changed_lines(text, mutation);
    let stable_id = stable_mutant_id(&source_name, mutator, &changes);
    let diff = unified_diff(&source_name, &changes);
    Ok(MutationEvidence {
        source,
        line: changes.first_line,
        stable_id,
        diff,
    })
}

struct ChangedLines {
    before: Vec<String>,
    after: Vec<String>,
    first_line: usize,
}

fn changed_lines(source: &str, mutation: &Mutation) -> ChangedLines {
    let Some(mutant) = mutation.apply(source) else {
        return ChangedLines {
            before: Vec::new(),
            after: Vec::new(),
            first_line: 1,
        };
    };
    let before = lines(source);
    let after = lines(&mutant);
    let prefix = shared_prefix(&before, &after);
    let suffix = shared_suffix(&before[prefix..], &after[prefix..]);
    ChangedLines {
        before: before[prefix..before.len() - suffix].to_vec(),
        after: after[prefix..after.len() - suffix].to_vec(),
        first_line: prefix + 1,
    }
}

fn lines(source: &str) -> Vec<String> {
    source.split_inclusive('\n').map(str::to_owned).collect()
}

fn shared_prefix(before: &[String], after: &[String]) -> usize {
    before
        .iter()
        .zip(after)
        .take_while(|(before, after)| before == after)
        .count()
}

fn shared_suffix(before: &[String], after: &[String]) -> usize {
    before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(before, after)| before == after)
        .count()
}

fn stable_mutant_id(source: &str, mutator: &str, changes: &ChangedLines) -> StableMutantId {
    let before = source_lines(&changes.before);
    let after = source_lines(&changes.after);
    StableMutantId(format!(
        "{:x}",
        md5::compute(format!("{source}\0{mutator}\0{before}\0{after}"))
    ))
}

fn source_lines(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("{}\n", line.trim_end_matches('\n')))
        .collect()
}

fn unified_diff(source: &str, changes: &ChangedLines) -> String {
    let mut diff = format!(
        "--- {source}\n+++ {source}\n@@ -{},{} +{},{} @@\n",
        changes.first_line,
        changes.before.len(),
        changes.first_line,
        changes.after.len()
    );
    append_diff_lines(&mut diff, '-', &changes.before);
    append_diff_lines(&mut diff, '+', &changes.after);
    diff
}

fn append_diff_lines(diff: &mut String, marker: char, lines: &[String]) {
    for line in lines {
        diff.push(marker);
        diff.push_str(line);
        if !line.ends_with('\n') {
            diff.push('\n');
        }
    }
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::Mutation;

    use super::{StableMutantId, mutation_evidence};

    #[test]
    fn evidence_uses_the_mutago_stable_id_rule() {
        let source = "pub fn unchecked() -> bool { true }\n";
        let mutation = Mutation::new(29..33, "false");
        let evidence = mutation_evidence(
            Path::new("workspace"),
            Path::new("workspace/checked/src/lib.rs"),
            "conditional/bool-literal",
            &mutation,
            source,
        )
        .expect("source must be inside the workspace layout");

        assert_eq!(
            evidence.stable_id.as_str(),
            "7e44f5b6649ca4de087acb260e75a287"
        );
        assert_eq!(
            evidence.diff,
            "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -1,1 +1,1 @@\n-pub fn unchecked() -> bool { true }\n+pub fn unchecked() -> bool { false }\n"
        );
    }

    #[test]
    fn stable_mutant_id_requires_lower_case_md5_hexadecimal() {
        assert!(StableMutantId::parse(&"a".repeat(32)).is_some());
        assert!(StableMutantId::parse(&"f".repeat(32)).is_some());
        assert!(StableMutantId::parse(&"g".repeat(32)).is_none());
        assert!(StableMutantId::parse(&"A".repeat(32)).is_none());
    }
}
