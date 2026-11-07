use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;

use regex::Regex;
use rustc_lexer::{TokenKind, tokenize};

use crate::filter::{FunctionMatcher, LineIndex, contains_range, matching_lines};

pub(crate) struct Suppression {
    disabled_functions: FunctionDisables,
    disabled_lines: BTreeMap<usize, Vec<MutatorSelection>>,
}

impl Suppression {
    pub(crate) fn new(
        text: &str,
        line_index: &LineIndex,
        source: &Path,
        known_mutators: &BTreeSet<String>,
        function_matcher: &FunctionMatcher,
    ) -> Result<Self, String> {
        let mut disabled_functions = FunctionDisables::default();
        let mut disabled_lines = BTreeMap::new();
        for comment in line_comments(text, line_index) {
            let Some(annotation) = Annotation::parse(&comment, source, known_mutators)? else {
                continue;
            };
            match annotation {
                Annotation::Function(selection) => {
                    let range = function_matcher.range_after(comment.line).ok_or_else(|| {
                        annotation_error(
                            source,
                            comment.line,
                            "function annotation must be directly before a function",
                        )
                    })?;
                    disabled_functions.add(range, selection);
                }
                Annotation::NextLine(selection) => {
                    disabled_lines
                        .entry(comment.line + 1)
                        .or_insert_with(Vec::new)
                        .push(selection);
                }
                Annotation::RegularExpression(pattern, selection) => {
                    for line in matching_lines(text, &[pattern]) {
                        disabled_lines
                            .entry(line)
                            .or_insert_with(Vec::new)
                            .push(selection.clone());
                    }
                }
            }
        }
        Ok(Self {
            disabled_functions,
            disabled_lines,
        })
    }

    pub(crate) fn blocks(&self, mutator: &str, range: &Range<usize>, lines: &LineIndex) -> bool {
        self.disabled_functions.blocks(mutator, range)
            || lines.lines_for(range).any(|line| {
                self.disabled_lines.get(&line).is_some_and(|selections| {
                    selections
                        .iter()
                        .any(|selection| selection.matches(mutator))
                })
            })
    }
}

#[derive(Default)]
struct FunctionDisables {
    items: Vec<FunctionDisable>,
}

impl FunctionDisables {
    fn add(&mut self, range: Range<usize>, selection: MutatorSelection) {
        self.items.push(FunctionDisable { range, selection });
    }

    fn blocks(&self, mutator: &str, range: &Range<usize>) -> bool {
        self.items.iter().any(|disabled| {
            contains_range(&disabled.range, range) && disabled.selection.matches(mutator)
        })
    }
}

struct Comment<'a> {
    text: &'a str,
    line: usize,
}

fn line_comments<'a>(text: &'a str, line_index: &LineIndex) -> Vec<Comment<'a>> {
    let mut comments = Vec::new();
    let mut offset = 0;
    for token in tokenize(text) {
        let end = offset + token.len;
        if matches!(token.kind, TokenKind::LineComment) {
            let comment = &text[offset..end];
            let content = comment
                .strip_prefix("//")
                .unwrap_or(comment)
                .trim_start_matches('/')
                .trim();
            comments.push(Comment {
                text: content,
                line: line_index.line_at(offset),
            });
        }
        offset = end;
    }
    comments
}

enum Annotation {
    Function(MutatorSelection),
    NextLine(MutatorSelection),
    RegularExpression(Regex, MutatorSelection),
}

struct FunctionDisable {
    range: Range<usize>,
    selection: MutatorSelection,
}

impl Annotation {
    fn parse(
        comment: &Comment<'_>,
        source: &Path,
        known_mutators: &BTreeSet<String>,
    ) -> Result<Option<Self>, String> {
        if !comment.text.starts_with("mutator-disable-") {
            return Ok(None);
        }
        let (name, content) = annotation_parts(comment, source)?;
        match name {
            "mutator-disable-func" => Ok(Some(Self::Function(parse_selection(
                content,
                source,
                comment.line,
                known_mutators,
            )?))),
            "mutator-disable-next-line" => Ok(Some(Self::NextLine(parse_selection(
                content,
                source,
                comment.line,
                known_mutators,
            )?))),
            "mutator-disable-regexp" => {
                let (pattern, selection) = content
                    .split_once(char::is_whitespace)
                    .map_or((content, ""), |(pattern, selection)| (pattern, selection));
                let pattern = Regex::new(pattern).map_err(|error| {
                    annotation_error(
                        source,
                        comment.line,
                        &format!("invalid annotation regular expression: {error}"),
                    )
                })?;
                Ok(Some(Self::RegularExpression(
                    pattern,
                    parse_selection(selection, source, comment.line, known_mutators)?,
                )))
            }
            _ => Err(annotation_error(
                source,
                comment.line,
                "unknown mutation annotation",
            )),
        }
    }
}

fn annotation_parts<'a>(
    comment: &'a Comment<'_>,
    source: &Path,
) -> Result<(&'a str, &'a str), String> {
    if let Some(parts) = comment.text.split_once(char::is_whitespace) {
        return Ok(parts);
    }
    match comment.text {
        "mutator-disable-func" | "mutator-disable-next-line" => Ok((comment.text, "")),
        "mutator-disable-regexp" => Err(annotation_error(
            source,
            comment.line,
            "regular-expression annotation requires a pattern",
        )),
        _ => Err(annotation_error(
            source,
            comment.line,
            "unknown mutation annotation",
        )),
    }
}

#[derive(Clone)]
enum MutatorSelection {
    All,
    Names(BTreeSet<String>),
}

impl MutatorSelection {
    fn matches(&self, mutator: &str) -> bool {
        matches!(self, Self::All) || matches!(self, Self::Names(names) if names.contains(mutator))
    }
}

fn parse_selection(
    selection: &str,
    source: &Path,
    line: usize,
    known_mutators: &BTreeSet<String>,
) -> Result<MutatorSelection, String> {
    let selection = selection.trim();
    if selection.is_empty() || selection == "*" {
        return Ok(MutatorSelection::All);
    }
    let names = selection
        .split(',')
        .map(str::trim)
        .map(|name| {
            known_mutators
                .contains(name)
                .then_some(name.to_owned())
                .ok_or_else(|| {
                    annotation_error(
                        source,
                        line,
                        &format!("unknown annotation mutator {name:?}"),
                    )
                })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(MutatorSelection::Names(names))
}

fn annotation_error(source: &Path, line: usize, message: &str) -> String {
    format!("{}:{line}: {message}", source.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::test_function_matcher;

    fn check(text: &str, needle: &str, name: &str) -> bool {
        let index = LineIndex::new(text);
        let matcher = test_function_matcher(text);
        let known = ["one".to_owned(), "two".to_owned()].into_iter().collect();
        let suppression =
            Suppression::new(text, &index, Path::new("source.rs"), &known, &matcher).unwrap();
        let start = text.find(needle).unwrap();
        suppression.blocks(name, &(start..start + needle.len()), &index)
    }

    #[test]
    fn directives_select_mutators_and_locations() {
        let text = "// mutator-disable-func one\n/// docs\nfn outer() { fn inner() { let nested = 1; } }\n// mutator-disable-next-line\nconst NEXT: i32 = 2;\n// mutator-disable-regexp MARK two\nconst MARK: i32 = 3;\nconst OTHER: i32 = 4;\n";
        assert!(check(text, "nested", "one"));
        assert!(!check(text, "nested", "two"));
        assert!(check(text, "NEXT", "one"));
        assert!(check(text, "NEXT", "two"));
        assert!(check(text, "const MARK", "two"));
        assert!(!check(text, "const MARK", "one"));
        assert!(!check(text, "OTHER", "two"));
    }

    #[test]
    fn directive_errors_include_source_and_line() {
        for (text, expected) in [
            (
                "// mutator-disable-unknown\n",
                "source.rs:1: unknown mutation annotation",
            ),
            (
                "// mutator-disable-regexp\n",
                "source.rs:1: regular-expression annotation requires a pattern",
            ),
            (
                "// mutator-disable-next-line missing\n",
                "source.rs:1: unknown annotation mutator \"missing\"",
            ),
            (
                "// mutator-disable-func\n\nfn f() {}\n",
                "source.rs:1: function annotation must be directly before a function",
            ),
        ] {
            let index = LineIndex::new(text);
            let matcher = test_function_matcher(text);
            let error = Suppression::new(
                text,
                &index,
                Path::new("source.rs"),
                &BTreeSet::new(),
                &matcher,
            )
            .err()
            .unwrap();
            assert_eq!(error, expected);
        }
        let text = "// mutator-disable-regexp [\n";
        let index = LineIndex::new(text);
        let matcher = test_function_matcher(text);
        let error = Suppression::new(
            text,
            &index,
            Path::new("source.rs"),
            &BTreeSet::new(),
            &matcher,
        )
        .err()
        .unwrap();
        assert!(error.starts_with("source.rs:1: invalid annotation regular expression:"));
    }
}
