use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::Mutation;
use crate::blacklist::MutationChecksum;

pub(crate) struct MutationEvidence {
    pub(crate) source: PathBuf,
    pub(crate) line: usize,
    pub(crate) stable_id: StableMutantId,
    pub(crate) blacklist_checksum: MutationChecksum,
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
    source_lines: &[&str],
    location: Option<&Range<usize>>,
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
    let mutant = mutation.apply(text);
    let after_lines: Vec<&str> = match &mutant {
        Some(m) => m.split_inclusive('\n').collect(),
        None => Vec::new(),
    };
    let changes = if mutant.is_some() {
        let prefix = shared_prefix(source_lines, &after_lines);
        let suffix = shared_suffix(&source_lines[prefix..], &after_lines[prefix..]);
        let before = &source_lines[prefix..source_lines.len() - suffix];
        let after = &after_lines[prefix..after_lines.len() - suffix];
        ChangedLines {
            before,
            after,
            first_line: mutation_line(text, mutation),
        }
    } else {
        ChangedLines {
            before: &[],
            after: &[],
            first_line: 1,
        }
    };
    let stable_id = stable_mutant_id(&source_name, mutator, &changes, location);
    let blacklist_checksum = blacklist_checksum(&changes, location);
    let diff = unified_diff(&source_name, &changes);
    Ok(MutationEvidence {
        source,
        line: changes.first_line,
        stable_id,
        blacklist_checksum,
        diff,
    })
}

struct ChangedLines<'a> {
    before: &'a [&'a str],
    after: &'a [&'a str],
    first_line: usize,
}

fn shared_prefix(before: &[&str], after: &[&str]) -> usize {
    before
        .iter()
        .zip(after)
        .take_while(|(before, after)| before == after)
        .count()
}

fn shared_suffix(before: &[&str], after: &[&str]) -> usize {
    before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(before, after)| before == after)
        .count()
}

fn mutation_line(source: &str, mutation: &Mutation) -> usize {
    let (range, _) = mutation.identity();
    source.get(..range.start).map_or(1, |prefix| {
        prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
    })
}

fn stable_mutant_id(
    source: &str,
    mutator: &str,
    changes: &ChangedLines<'_>,
    location: Option<&Range<usize>>,
) -> StableMutantId {
    let before = source_lines(changes.before);
    let after = source_lines(changes.after);
    let mut content = format!("{source}\0{mutator}\0{before}\0{after}");
    append_location(&mut content, location);
    StableMutantId(format!("{:x}", md5::compute(content)))
}

fn blacklist_checksum(
    changes: &ChangedLines<'_>,
    location: Option<&Range<usize>>,
) -> MutationChecksum {
    let mut content = String::new();
    append_checksum_lines(&mut content, '-', changes.before);
    append_checksum_lines(&mut content, '+', changes.after);
    append_location(&mut content, location);
    MutationChecksum::from_changed_lines(content)
}

fn append_location(content: &mut String, location: Option<&Range<usize>>) {
    if let Some(location) = location {
        content.push('\0');
        content.push_str(&location.start.to_string());
        content.push(':');
        content.push_str(&location.end.to_string());
    }
}

fn append_checksum_lines(content: &mut String, marker: char, lines: &[&str]) {
    for line in lines {
        content.push(marker);
        content.push_str(line.trim_end_matches('\n'));
        content.push('\n');
    }
}

fn source_lines(lines: &[&str]) -> String {
    let mut content = String::new();
    for line in lines {
        content.push_str(line.trim_end_matches('\n'));
        content.push('\n');
    }
    content
}

fn unified_diff(source: &str, changes: &ChangedLines<'_>) -> String {
    let mut diff = format!(
        "--- {source}\n+++ {source}\n@@ -{},{} +{},{} @@\n",
        changes.first_line,
        changes.before.len(),
        changes.first_line,
        changes.after.len()
    );
    append_diff_lines(&mut diff, '-', changes.before);
    append_diff_lines(&mut diff, '+', changes.after);
    diff
}

fn append_diff_lines(diff: &mut String, marker: char, lines: &[&str]) {
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

    use super::{MutationEvidence, StableMutantId, mutation_evidence};

    fn evidence_for_text(
        source_root: &Path,
        source: &Path,
        mutator: &str,
        mutation: &Mutation,
        text: &str,
    ) -> Result<MutationEvidence, String> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        mutation_evidence(source_root, source, mutator, mutation, text, &lines, None)
    }

    #[test]
    fn evidence_uses_the_mutago_stable_id_rule() {
        let source = "pub fn unchecked() -> bool { true }\n";
        let mutation = Mutation::new(29..33, "false");
        let evidence = evidence_for_text(
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
        assert_eq!(
            evidence.blacklist_checksum.as_str(),
            format!(
                "{:x}",
                md5::compute(
                    "-pub fn unchecked() -> bool { true }\n+pub fn unchecked() -> bool { false }\n"
                )
            )
        );
    }

    #[test]
    fn evidence_keeps_shared_prefix_and_suffix_out_of_the_diff() {
        let source = "fn first() {}\npub fn unchecked() -> bool { true }\nfn last() {}\n";
        let start = source.find("true").expect("fixture must contain true");
        let mutation = Mutation::new(start..start + 4, "false");
        let evidence = evidence_for_text(
            Path::new("workspace"),
            Path::new("workspace/checked/src/lib.rs"),
            "conditional/bool-literal",
            &mutation,
            source,
        )
        .expect("source must be inside the workspace layout");

        assert_eq!(evidence.line, 2);
        assert_eq!(
            evidence.diff,
            "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -2,1 +2,1 @@\n-pub fn unchecked() -> bool { true }\n+pub fn unchecked() -> bool { false }\n"
        );
        assert_eq!(
            evidence.blacklist_checksum.as_str(),
            format!(
                "{:x}",
                md5::compute(
                    "-pub fn unchecked() -> bool { true }\n+pub fn unchecked() -> bool { false }\n"
                )
            )
        );
    }

    #[test]
    fn evidence_keeps_the_mutation_start_line_when_equal_lines_shift_the_diff() {
        let source = "fn duplicated() {\n    counter();\n    counter();\n}\n";
        let first_start = source
            .find("    counter();")
            .expect("first statement must exist");
        let first_end = first_start + "    counter();\n".len();
        let second_start = source[first_end..]
            .find("    counter();")
            .expect("second statement must exist")
            + first_end;
        let second_end = second_start + "    counter();\n".len();
        let first_mutation = Mutation::new(first_start..first_end, "");
        let second_mutation = Mutation::new(second_start..second_end, "");
        let second_range = second_mutation.identity().0;
        let lines: Vec<&str> = source.split_inclusive('\n').collect();
        let first = mutation_evidence(
            Path::new("workspace"),
            Path::new("workspace/checked/src/lib.rs"),
            "statement/remove",
            &first_mutation,
            source,
            &lines,
            None,
        )
        .expect("first mutation evidence must be generated");
        let second = mutation_evidence(
            Path::new("workspace"),
            Path::new("workspace/checked/src/lib.rs"),
            "statement/remove",
            &second_mutation,
            source,
            &lines,
            Some(&second_range),
        )
        .expect("second mutation evidence must be generated");

        assert_eq!(first.line, 2);
        assert_eq!(second.line, 3);
        assert_ne!(first.stable_id.as_str(), second.stable_id.as_str());
        assert_ne!(
            first.blacklist_checksum.as_str(),
            second.blacklist_checksum.as_str()
        );
    }

    #[test]
    fn stable_mutant_id_requires_lower_case_md5_hexadecimal() {
        assert!(StableMutantId::parse(&"a".repeat(32)).is_some());
        assert!(StableMutantId::parse(&"f".repeat(32)).is_some());
        assert!(StableMutantId::parse("0123456789abcdef0123456789abcdef").is_some());
        assert!(StableMutantId::parse(&"a".repeat(31)).is_none());
        assert!(StableMutantId::parse(&"a".repeat(33)).is_none());
        assert!(StableMutantId::parse(&"g".repeat(32)).is_none());
        assert!(StableMutantId::parse(&"A".repeat(32)).is_none());
    }
}
