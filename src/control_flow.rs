use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Arm, BinOp, Block, Expr, ExprBlock, ExprBreak, ExprClosure, ExprContinue, ExprForLoop, ExprIf,
    ExprLit, ExprLoop, ExprWhile, Lit, Stmt,
};

use crate::mutator::span_range;
use crate::{Mutation, Mutator};

#[derive(Clone, Copy)]
enum ControlFlowKind {
    BranchCase,
    BranchElse,
    BranchIf,
    LoopBreak,
    LoopCondition,
    LoopRangeBreak,
    StatementRemove,
}

pub(crate) struct ControlFlowMutator {
    name: &'static str,
    kind: ControlFlowKind,
}

impl ControlFlowMutator {
    pub(crate) fn branch_case() -> Self {
        Self::new("branch/case", ControlFlowKind::BranchCase)
    }

    pub(crate) fn branch_else() -> Self {
        Self::new("branch/else", ControlFlowKind::BranchElse)
    }

    pub(crate) fn branch_if() -> Self {
        Self::new("branch/if", ControlFlowKind::BranchIf)
    }

    pub(crate) fn loop_break() -> Self {
        Self::new("loop/break", ControlFlowKind::LoopBreak)
    }

    pub(crate) fn loop_condition() -> Self {
        Self::new("loop/condition", ControlFlowKind::LoopCondition)
    }

    pub(crate) fn loop_range_break() -> Self {
        Self::new("loop/range_break", ControlFlowKind::LoopRangeBreak)
    }

    pub(crate) fn statement_remove() -> Self {
        Self::new("statement/remove", ControlFlowKind::StatementRemove)
    }

    fn new(name: &'static str, kind: ControlFlowKind) -> Self {
        Self { name, kind }
    }
}

impl Mutator for ControlFlowMutator {
    fn name(&self) -> &str {
        self.name
    }

    fn mutations_from_parsed(&self, source: &str, file: &syn::File) -> Vec<Mutation> {
        match self.kind {
            ControlFlowKind::BranchCase => BranchCaseVisitor::collect(source, file),
            ControlFlowKind::BranchElse => BranchElseVisitor::collect(source, file),
            ControlFlowKind::BranchIf => BranchIfVisitor::collect(source, file),
            ControlFlowKind::LoopBreak => collect_loop_control(source, file),
            ControlFlowKind::LoopCondition => LoopConditionVisitor::collect(source, file),
            ControlFlowKind::LoopRangeBreak => LoopRangeBreakVisitor::collect(source, file),
            ControlFlowKind::StatementRemove => StatementRemoveVisitor::collect(source, file),
        }
    }
}

struct BranchCaseVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> BranchCaseVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }

    fn add_arm(&mut self, arm: &Arm) {
        match arm.body.as_ref() {
            Expr::Block(block) => {
                add_nonempty_block(self.source, &mut self.mutations, &block.block);
            }
            Expr::Tuple(tuple) if tuple.elems.is_empty() => {}
            body => {
                if let Some(range) = span_range(self.source, body.span()) {
                    self.mutations.push(Mutation::new(range, "{}"));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for BranchCaseVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_arm(&mut self, arm: &'ast Arm) {
        self.add_arm(arm);
        visit::visit_arm(self, arm);
    }
}

struct BranchElseVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> BranchElseVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

impl<'ast> Visit<'ast> for BranchElseVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        if let Some((_, branch)) = &expression.else_branch {
            if let Expr::Block(block) = branch.as_ref() {
                add_nonempty_block(self.source, &mut self.mutations, &block.block);
            }
        }
        visit::visit_expr_if(self, expression);
    }
}

struct BranchIfVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> BranchIfVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

impl<'ast> Visit<'ast> for BranchIfVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        add_nonempty_block(self.source, &mut self.mutations, &expression.then_branch);
        visit::visit_expr_if(self, expression);
    }
}

fn add_nonempty_block(source: &str, mutations: &mut Vec<Mutation>, block: &Block) {
    if let Some(range) = block_statement_range(source, block) {
        mutations.push(Mutation::new(range, ""));
    }
}

fn block_statement_range(source: &str, block: &Block) -> Option<std::ops::Range<usize>> {
    let start = span_range(source, block.stmts.first()?.span())?.start;
    let end = span_range(source, block.stmts.last()?.span())?.end;
    (start < end).then_some(start..end)
}

#[derive(Clone)]
struct LabelTarget {
    name: String,
    is_loop: bool,
}

struct LoopControlVisitor<'a> {
    source: &'a str,
    loop_depth: usize,
    labels: Vec<LabelTarget>,
    mutations: Vec<Mutation>,
}

fn collect_loop_control(source: &str, file: &syn::File) -> Vec<Mutation> {
    let mut visitor = LoopControlVisitor {
        source,
        loop_depth: 0,
        labels: Vec::new(),
        mutations: Vec::new(),
    };
    visitor.visit_file(file);
    visitor.mutations
}

impl LoopControlVisitor<'_> {
    fn target_is_loop(&self, label: Option<&syn::Lifetime>) -> bool {
        match label {
            Some(label) => self
                .labels
                .iter()
                .rev()
                .find(|target| label.ident == target.name)
                .is_some_and(|target| target.is_loop),
            None => self.loop_depth > 0,
        }
    }

    fn push_label(&mut self, label: Option<&syn::Label>, is_loop: bool) {
        if let Some(label) = label {
            self.labels.push(LabelTarget {
                name: label.name.ident.to_string(),
                is_loop,
            });
        }
    }

    fn pop_label(&mut self, label: Option<&syn::Label>) {
        if label.is_some() {
            self.labels.pop();
        }
    }
}

impl<'ast> Visit<'ast> for LoopControlVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_break(&mut self, expression: &'ast ExprBreak) {
        if expression.expr.is_none() && self.target_is_loop(expression.label.as_ref()) {
            add_replacement(
                self.source,
                &mut self.mutations,
                expression.break_token.span,
                "continue",
            );
        }
        visit::visit_expr_break(self, expression);
    }

    fn visit_expr_continue(&mut self, expression: &'ast ExprContinue) {
        if self.target_is_loop(expression.label.as_ref()) {
            add_replacement(
                self.source,
                &mut self.mutations,
                expression.continue_token.span,
                "break",
            );
        }
        visit::visit_expr_continue(self, expression);
    }

    fn visit_expr_loop(&mut self, expression: &'ast ExprLoop) {
        self.loop_depth += 1;
        self.push_label(expression.label.as_ref(), true);
        visit::visit_expr_loop(self, expression);
        self.pop_label(expression.label.as_ref());
        self.loop_depth -= 1;
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.loop_depth += 1;
        self.push_label(expression.label.as_ref(), true);
        visit::visit_expr_for_loop(self, expression);
        self.pop_label(expression.label.as_ref());
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.loop_depth += 1;
        self.push_label(expression.label.as_ref(), true);
        visit::visit_expr_while(self, expression);
        self.pop_label(expression.label.as_ref());
        self.loop_depth -= 1;
    }

    fn visit_expr_block(&mut self, expression: &'ast ExprBlock) {
        self.push_label(expression.label.as_ref(), false);
        visit::visit_expr_block(self, expression);
        self.pop_label(expression.label.as_ref());
    }

    fn visit_expr_closure(&mut self, expression: &'ast ExprClosure) {
        let loop_depth = self.loop_depth;
        let labels = std::mem::take(&mut self.labels);
        self.loop_depth = 0;
        visit::visit_expr_closure(self, expression);
        self.loop_depth = loop_depth;
        self.labels = labels;
    }
}

fn add_replacement(
    source: &str,
    mutations: &mut Vec<Mutation>,
    span: proc_macro2::Span,
    replacement: &str,
) {
    if let Some(range) = span_range(source, span) {
        mutations.push(Mutation::new(range, replacement));
    }
}

struct LoopConditionVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> LoopConditionVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

impl<'ast> Visit<'ast> for LoopConditionVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        if !is_false_literal(&expression.cond) {
            if let Some(range) = span_range(self.source, expression.cond.span()) {
                self.mutations.push(Mutation::new(range, "false"));
            }
        }
        visit::visit_expr_while(self, expression);
    }
}

fn is_false_literal(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::Lit(ExprLit {
            lit: Lit::Bool(literal),
            ..
        }) if !literal.value
    )
}

struct LoopRangeBreakVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> LoopRangeBreakVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

impl<'ast> Visit<'ast> for LoopRangeBreakVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        if !starts_with_current_loop_break(&expression.body, expression.label.as_ref()) {
            if let Some(offset) = for_body_insertion_offset(self.source, expression) {
                self.mutations
                    .push(Mutation::new(offset..offset, " break;"));
            }
        }
        visit::visit_expr_for_loop(self, expression);
    }
}

fn for_body_insertion_offset(source: &str, expression: &ExprForLoop) -> Option<usize> {
    let open = expression.body.brace_token.span.open().byte_range();
    let offset = expression
        .attrs
        .iter()
        .filter(|attribute| matches!(attribute.style, syn::AttrStyle::Inner(_)))
        .filter_map(|attribute| span_range(source, attribute.span()).map(|range| range.end))
        .max()
        .unwrap_or(open.end);
    (offset <= source.len() && source.is_char_boundary(offset)).then_some(offset)
}

fn starts_with_current_loop_break(block: &Block, loop_label: Option<&syn::Label>) -> bool {
    let Some(Stmt::Expr(Expr::Break(expression), Some(_))) = block.stmts.first() else {
        return false;
    };
    if expression.expr.is_some() {
        return false;
    }
    match (expression.label.as_ref(), loop_label) {
        (None, _) => true,
        (Some(target), Some(loop_label)) => target.ident == loop_label.name.ident,
        (Some(_), None) => false,
    }
}

struct StatementRemoveVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'a> StatementRemoveVisitor<'a> {
    fn collect(source: &'a str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }

    fn add_statement(&mut self, statement: &Stmt) {
        if removable_statement(statement) {
            if let Some(range) = span_range(self.source, statement.span()) {
                self.mutations.push(Mutation::new(range, ""));
            }
        }
    }
}

impl<'ast> Visit<'ast> for StatementRemoveVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_block(&mut self, block: &'ast Block) {
        for statement in &block.stmts {
            self.add_statement(statement);
        }
        visit::visit_block(self, block);
    }
}

fn removable_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::Expr(expression, Some(_)) => removable_expression(expression),
        Stmt::Macro(statement) => statement.semi_token.is_some(),
        Stmt::Local(_) | Stmt::Item(_) | Stmt::Expr(_, None) => false,
    }
}

fn removable_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Assign(assignment) => !crate::value::is_safe_self_assignment(assignment),
        Expr::Call(_) | Expr::Macro(_) | Expr::MethodCall(_) => true,
        Expr::Binary(binary) => assignment_operator(&binary.op),
        _ => false,
    }
}

fn assignment_operator(operator: &BinOp) -> bool {
    matches!(
        operator,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}
