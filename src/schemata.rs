use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::ops::Range;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::mutator::span_range;

pub(crate) const ACTIVE_MUTANT_VARIABLE: &str = "MUTARUST_ACTIVE_MUTANT";

pub(crate) struct SourceEdit {
    pub index: u32,
    pub range: Range<usize>,
    pub replacement: String,
}

pub(crate) fn rewrite_with_mutant_match(source: &str, edits: &[SourceEdit]) -> Option<String> {
    let file = syn::parse_file(source).ok()?;
    let nodes = NodeRanges::collect(source, &file);
    let renderer = Renderer {
        source,
        chosen: chosen_nodes(&nodes, edits)?,
    };
    let mut rewritten = renderer.render_interval(&(0..source.len()))?;
    rewritten.push_str(&active_mutant_function());
    syn::parse_file(&rewritten).ok()?;
    Some(rewritten)
}

struct NodeRanges<'a> {
    source: &'a str,
    ranges: Vec<Range<usize>>,
}

impl<'a> NodeRanges<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Range<usize>> {
        let mut visitor = Self {
            source,
            ranges: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.ranges
    }

    fn push(&mut self, span: Span) {
        if let Some(range) = span_range(self.source, span) {
            self.ranges.push(range);
        }
    }
}

impl<'ast> Visit<'ast> for NodeRanges<'_> {
    skip_non_expression_syntax!();

    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        self.push(expression.span());
        visit::visit_expr(self, expression);
    }

    fn visit_stmt(&mut self, statement: &'ast syn::Stmt) {
        self.push(statement.span());
        visit::visit_stmt(self, statement);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.push(block.span());
        visit::visit_block(self, block);
    }
}

struct ChosenNode<'a> {
    range: Range<usize>,
    edits: Vec<&'a SourceEdit>,
}

fn chosen_nodes<'a>(
    nodes: &[Range<usize>],
    edits: &'a [SourceEdit],
) -> Option<Vec<ChosenNode<'a>>> {
    let mut chosen = BTreeMap::<(usize, Reverse<usize>), Vec<&SourceEdit>>::new();
    for edit in edits {
        let node = nodes
            .iter()
            .filter(|node| contains(node, &edit.range))
            .min_by_key(|node| node.len())?;
        chosen
            .entry((node.start, Reverse(node.end)))
            .or_default()
            .push(edit);
    }
    Some(
        chosen
            .into_iter()
            .map(|((start, Reverse(end)), edits)| ChosenNode {
                range: start..end,
                edits,
            })
            .collect(),
    )
}

fn contains(node: &Range<usize>, edit: &Range<usize>) -> bool {
    edit.start <= edit.end && node.start <= edit.start && node.end >= edit.end
}

fn inside(node: &Range<usize>, interval: &Range<usize>) -> bool {
    interval.start <= node.start && node.end <= interval.end && node != interval
}

struct Renderer<'a> {
    source: &'a str,
    chosen: Vec<ChosenNode<'a>>,
}

impl Renderer<'_> {
    fn render_interval(&self, interval: &Range<usize>) -> Option<String> {
        let mut rendered = String::new();
        let mut cursor = interval.start;
        for node in self
            .chosen
            .iter()
            .filter(|node| inside(&node.range, interval))
        {
            if node.range.end <= cursor {
                continue;
            }
            if node.range.start < cursor {
                return None;
            }
            rendered.push_str(self.source.get(cursor..node.range.start)?);
            rendered.push_str(&self.render_node(node)?);
            cursor = node.range.end;
        }
        rendered.push_str(self.source.get(cursor..interval.end)?);
        Some(rendered)
    }

    fn render_node(&self, node: &ChosenNode<'_>) -> Option<String> {
        let arms = node
            .edits
            .iter()
            .map(|edit| self.render_arm(&node.range, edit))
            .collect::<Option<String>>()?;
        let default = self.render_interval(&node.range)?;
        Some(format!(
            "(match mutarust_active_mutant() {{ {arms}_ => {{ {default} }} }})"
        ))
    }

    fn render_arm(&self, node: &Range<usize>, edit: &SourceEdit) -> Option<String> {
        let before = self.source.get(node.start..edit.range.start)?;
        let after = self.source.get(edit.range.end..node.end)?;
        Some(format!(
            "{} => {{ {before}{}{after} }}, ",
            edit.index, edit.replacement
        ))
    }
}

fn active_mutant_function() -> String {
    format!(
        "\nfn mutarust_active_mutant() -> u32 {{\n    std::env::var({ACTIVE_MUTANT_VARIABLE:?})\n        .ok()\n        .and_then(|value| value.parse().ok())\n        .unwrap_or(u32::MAX)\n}}\n"
    )
}
