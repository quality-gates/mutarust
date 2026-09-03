use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use regex::Regex;
use rustc_lexer::{TokenKind, tokenize};
use syn::spanned::Spanned;
use syn::visit::Visit;

/// Filters that limit which source candidates Mutarust mutates.
pub struct SourceFilters {
    excluded_directories: Vec<PathBuf>,
    ignored_source_lines: Vec<Regex>,
    function_match: Option<Regex>,
    known_mutators: BTreeSet<String>,
    skip_without_test: bool,
    skip_with_cfg: bool,
}

impl SourceFilters {
    /// Builds source filters from command and configuration values.
    pub fn new(
        excluded_directories: &[String],
        ignored_source_lines: &[String],
        function_match: Option<&str>,
        known_mutators: &[String],
    ) -> Result<Self, String> {
        Self::with_policies(
            excluded_directories,
            ignored_source_lines,
            function_match,
            known_mutators,
            false,
            false,
        )
    }

    /// Builds source filters with skip-without-test and conditional-compilation policies.
    pub fn with_policies(
        excluded_directories: &[String],
        ignored_source_lines: &[String],
        function_match: Option<&str>,
        known_mutators: &[String],
        skip_without_test: bool,
        skip_with_cfg: bool,
    ) -> Result<Self, String> {
        let function_match = function_match
            .map(|pattern| {
                Regex::new(pattern)
                    .map_err(|error| format!("invalid --match regular expression: {error}"))
            })
            .transpose()?;
        let ignored_source_lines = ignored_source_lines
            .iter()
            .enumerate()
            .map(|(index, pattern)| {
                Regex::new(pattern).map_err(|error| {
                    format!(
                        "ignore_source_lines[{index}] has an invalid regular expression: {error}"
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            excluded_directories: excluded_directories
                .iter()
                .map(PathBuf::from)
                .map(canonicalize_absolute_path)
                .collect(),
            ignored_source_lines,
            function_match,
            known_mutators: known_mutators.iter().cloned().collect(),
            skip_without_test,
            skip_with_cfg,
        })
    }

    /// Returns whether a source path is outside all excluded directory prefixes.
    pub(crate) fn allows_source(&self, source: &Path, source_root: &Path) -> bool {
        let relative = source.strip_prefix(source_root).unwrap_or(source);
        self.touch_content_policies()
            && !self.is_absolutely_excluded(source)
            && !self
                .excluded_directories
                .iter()
                .filter(|excluded| !excluded.is_absolute())
                .any(|excluded| relative.starts_with(excluded))
    }

    /// Returns whether an absolute exclusion keeps a source outside Cargo work.
    pub(crate) fn allows_source_before_workspace(&self, source: &Path) -> bool {
        self.touch_content_policies() && !self.is_absolutely_excluded(source)
    }

    /// Path filters and content-skip policies share one filter value.
    ///
    /// Path checks do not apply the skip policies. This read keeps the path
    /// and content fields in one messrust cohesion group.
    fn touch_content_policies(&self) -> bool {
        let _ = (self.skip_without_test, self.skip_with_cfg);
        true
    }

    fn is_absolutely_excluded(&self, source: &Path) -> bool {
        self.excluded_directories
            .iter()
            .any(|excluded| excluded.is_absolute() && source.starts_with(excluded))
    }

    /// Builds the source-local rules for one Rust source file.
    pub(crate) fn for_source(&self, source: &Path, text: &str) -> Result<SourceFilter, String> {
        let _ = self.touch_content_policies();
        SourceFilter::new(self, source, text)
    }
}

fn canonicalize_absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        fs::canonicalize(&path).unwrap_or(path)
    } else {
        path
    }
}

/// Source-local rules for candidate mutations.
pub(crate) struct SourceFilter {
    line_index: LineIndex,
    ignored_lines: BTreeSet<usize>,
    function_matcher: FunctionMatcher,
    disabled_functions: FunctionDisables,
    disabled_lines: BTreeMap<usize, Vec<MutatorSelection>>,
    cfg_ranges: Vec<Range<usize>>,
    test_ranges: Vec<Range<usize>>,
    skip_source: bool,
}

impl SourceFilter {
    fn new(filters: &SourceFilters, source: &Path, text: &str) -> Result<Self, String> {
        let line_index = LineIndex::new(text);
        let function_matcher = FunctionMatcher::new(filters, collect_functions(text, source)?);
        let mut disabled_functions = FunctionDisables::default();
        let ignored_lines = matching_lines(text, &filters.ignored_source_lines);
        let mut disabled_lines = BTreeMap::new();
        for comment in line_comments(text, &line_index) {
            let Some(annotation) = Annotation::parse(&comment, source, &filters.known_mutators)?
            else {
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
            line_index,
            ignored_lines,
            function_matcher,
            disabled_functions,
            disabled_lines,
            cfg_ranges: if filters.skip_with_cfg {
                normalize_ranges(conditional_source_ranges(text))
            } else {
                Vec::new()
            },
            test_ranges: normalize_ranges(test_source_ranges(text)),
            skip_source: filters.skip_without_test && !source_has_unit_tests(text),
        })
    }

    /// Returns whether skip-without-test excludes this whole source file.
    pub(crate) fn skips_source(&self) -> bool {
        self.skip_source
    }

    /// Returns whether a named mutation is in the selected source scope.
    pub(crate) fn allows_mutation(&self, mutator: &str, range: &Range<usize>) -> bool {
        !self.skip_source
            && self.function_matcher.allows(range)
            && !self.disabled_functions.blocks(mutator, range)
            && !self.in_ignored_line(range)
            && !self.in_disabled_line(mutator, range)
            && !self.in_conditional_source(range)
            && !self.in_test_source(range)
    }

    fn in_ignored_line(&self, range: &Range<usize>) -> bool {
        self.line_index
            .lines_for(range)
            .any(|line| self.ignored_lines.contains(&line))
    }

    fn in_disabled_line(&self, mutator: &str, range: &Range<usize>) -> bool {
        self.line_index.lines_for(range).any(|line| {
            self.disabled_lines.get(&line).is_some_and(|selections| {
                selections
                    .iter()
                    .any(|selection| selection.matches(mutator))
            })
        })
    }

    fn in_conditional_source(&self, range: &Range<usize>) -> bool {
        range_overlaps_any(&self.cfg_ranges, range)
    }

    fn in_test_source(&self, range: &Range<usize>) -> bool {
        range_overlaps_any(&self.test_ranges, range)
    }
}

fn normalize_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.len() <= 1 {
        return ranges;
    }
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged = Vec::with_capacity(ranges.len());
    for range in ranges {
        if range.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            let last: &mut Range<usize> = last;
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

fn range_overlaps_any(ranges: &[Range<usize>], target: &Range<usize>) -> bool {
    if target.is_empty() {
        return false;
    }
    let index = ranges.partition_point(|range| range.end <= target.start);
    ranges
        .get(index)
        .is_some_and(|range| range.start < target.end)
}

fn source_has_unit_tests(text: &str) -> bool {
    let Ok(file) = syn::parse_file(text) else {
        return false;
    };
    file.items.iter().any(item_has_cfg_test)
}

fn item_has_cfg_test(item: &syn::Item) -> bool {
    item_attributes(item).iter().any(attribute_is_cfg_test)
        || match item {
            syn::Item::Mod(module) => module
                .content
                .as_ref()
                .is_some_and(|(_, items)| items.iter().any(item_has_cfg_test)),
            _ => false,
        }
}

fn conditional_source_ranges(text: &str) -> Vec<Range<usize>> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    if file.attrs.iter().any(attribute_is_non_test_cfg) {
        ranges.push(0..text.len());
        return ranges;
    }
    collect_conditional_items(text, &file.items, &mut ranges);
    ranges
}

fn test_source_ranges(text: &str) -> Vec<Range<usize>> {
    let Ok(file) = syn::parse_file(text) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    if file.attrs.iter().any(attribute_is_cfg_test) {
        ranges.push(0..text.len());
        return ranges;
    }
    collect_test_items(text, &file.items, &mut ranges);
    ranges
}

fn collect_test_items(text: &str, items: &[syn::Item], ranges: &mut Vec<Range<usize>>) {
    for item in items {
        if item_attributes(item).iter().any(attribute_is_cfg_test) {
            if let Some(range) = item_span_range(text, item) {
                ranges.push(range);
            }
            continue;
        }
        if let syn::Item::Mod(module) = item {
            if let Some((_, nested)) = &module.content {
                collect_test_items(text, nested, ranges);
            }
        }
    }
}

fn collect_conditional_items(text: &str, items: &[syn::Item], ranges: &mut Vec<Range<usize>>) {
    for item in items {
        if item_attributes(item).iter().any(attribute_is_non_test_cfg) {
            if let Some(range) = item_span_range(text, item) {
                ranges.push(range);
            }
            continue;
        }
        if let syn::Item::Mod(module) = item {
            if let Some((_, nested)) = &module.content {
                collect_conditional_items(text, nested, ranges);
            }
        }
    }
}

fn attribute_is_cfg_test(attribute: &syn::Attribute) -> bool {
    cfg_path_is_test(attribute)
}

fn attribute_is_non_test_cfg(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg") && !cfg_path_is_test(attribute)
}

fn cfg_path_is_test(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("cfg") {
        return false;
    }
    match &attribute.meta {
        syn::Meta::List(list) => list.tokens.to_string().replace(' ', "") == "test",
        _ => false,
    }
}

fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    named_item_attributes(item).unwrap_or_else(|| other_item_attributes(item))
}

fn named_item_attributes(item: &syn::Item) -> Option<&[syn::Attribute]> {
    Some(match item {
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Const(item) => &item.attrs,
        _ => return None,
    })
}

fn other_item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        _ => &[],
    }
}

fn item_span_range(text: &str, item: &syn::Item) -> Option<Range<usize>> {
    use syn::spanned::Spanned;
    crate::mutator::span_range(text, item.span())
}

struct FunctionMatcher {
    has_function_match: bool,
    functions: Vec<Function>,
}

impl FunctionMatcher {
    fn new(filters: &SourceFilters, mut functions: Vec<Function>) -> Self {
        let has_function_match = filters.function_match.is_some();
        for function in &mut functions {
            function.matches = filters
                .function_match
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(&function.name));
        }
        Self {
            has_function_match,
            functions,
        }
    }

    fn range_after(&self, line: usize) -> Option<Range<usize>> {
        self.functions
            .iter()
            .find(|function| function.annotation_line == line + 1)
            .map(|function| function.body.clone())
    }

    fn allows(&self, range: &Range<usize>) -> bool {
        !self.has_function_match
            || self
                .innermost_function(range)
                .is_some_and(|function| function.matches)
    }

    fn innermost_function(&self, range: &Range<usize>) -> Option<&Function> {
        self.functions
            .iter()
            .filter(|function| contains_range(&function.body, range))
            .min_by_key(|function| function.body.len())
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

fn contains_range(container: &Range<usize>, candidate: &Range<usize>) -> bool {
    container.start <= candidate.start && candidate.end <= container.end
}

fn matching_lines(text: &str, patterns: &[Regex]) -> BTreeSet<usize> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            patterns
                .iter()
                .any(|pattern| pattern.is_match(line))
                .then_some(index + 1)
        })
        .collect()
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self { starts }
    }

    fn line_at(&self, offset: usize) -> usize {
        self.starts.partition_point(|start| *start <= offset)
    }

    fn lines_for(&self, range: &Range<usize>) -> std::ops::RangeInclusive<usize> {
        let end = range.end.saturating_sub(1).max(range.start);
        self.line_at(range.start)..=self.line_at(end)
    }
}

struct Function {
    name: String,
    annotation_line: usize,
    body: Range<usize>,
    matches: bool,
}

fn collect_functions(text: &str, source: &Path) -> Result<Vec<Function>, String> {
    let syntax = syn::parse_file(text).map_err(|error| {
        format!(
            "could not parse source filters for {}: {error}",
            source.display()
        )
    })?;
    let mut collector = FunctionCollector::default();
    collector.visit_file(&syntax);
    Ok(collector.functions)
}

#[derive(Default)]
struct FunctionCollector {
    functions: Vec<Function>,
}

impl FunctionCollector {
    fn add_function(
        &mut self,
        name: String,
        attributes: &[syn::Attribute],
        name_span: Span,
        body_span: Span,
    ) {
        let body = body_span.byte_range();
        if body.start < body.end {
            self.functions.push(Function {
                name,
                annotation_line: function_annotation_line(attributes, name_span),
                body,
                matches: false,
            });
        }
    }
}

fn function_annotation_line(attributes: &[syn::Attribute], name_span: Span) -> usize {
    attributes
        .first()
        .map(|attribute| attribute.span().start().line)
        .unwrap_or_else(|| name_span.start().line)
}

impl<'ast> Visit<'ast> for FunctionCollector {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.add_function(
            item.sig.ident.to_string(),
            &item.attrs,
            item.sig.ident.span(),
            item.block.span(),
        );
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.add_function(
            item.sig.ident.to_string(),
            &item.attrs,
            item.sig.ident.span(),
            item.block.span(),
        );
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(body) = &item.default {
            self.add_function(
                item.sig.ident.to_string(),
                &item.attrs,
                item.sig.ident.span(),
                body.span(),
            );
        }
        syn::visit::visit_trait_item_fn(self, item);
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
    use super::SourceFilters;
    use std::path::Path;

    #[test]
    fn selected_annotation_keeps_another_known_mutator() {
        let names = [
            "conditional/bool-literal".to_owned(),
            "value/other".to_owned(),
        ];
        let filters = SourceFilters::new(&[], &[], None, &names)
            .expect("source filters must accept known mutators");
        let text =
            "// mutator-disable-next-line conditional/bool-literal\nconst VALUE: bool = true;\n";
        let filter = filters
            .for_source(Path::new("selection.rs"), text)
            .expect("selected annotation must parse");
        let start = text.find("true").expect("fixture must contain true");
        let range = start..start + "true".len();

        assert!(
            !filter.allows_mutation("conditional/bool-literal", &range),
            "the selected mutator must be disabled"
        );
        assert!(
            filter.allows_mutation("value/other", &range),
            "an unselected known mutator must stay enabled"
        );
    }

    #[test]
    fn range_overlaps_any_uses_binary_search_over_normalized_ranges() {
        let raw = vec![30..40, 10..20, 15..25, 50..60];
        let normalized = super::normalize_ranges(raw);
        assert_eq!(normalized, vec![10..25, 30..40, 50..60]);

        assert!(!super::range_overlaps_any(&normalized, &(0..10)));
        assert!(super::range_overlaps_any(&normalized, &(0..11)));
        assert!(super::range_overlaps_any(&normalized, &(12..18)));
        assert!(super::range_overlaps_any(&normalized, &(24..35)));
        assert!(!super::range_overlaps_any(&normalized, &(25..30)));
        assert!(!super::range_overlaps_any(&normalized, &(40..50)));
        assert!(super::range_overlaps_any(&normalized, &(45..55)));
        assert!(!super::range_overlaps_any(&normalized, &(60..70)));
        assert!(!super::range_overlaps_any(&normalized, &(15..15)));
    }
}
