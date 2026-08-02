use std::fmt;
use std::ops::Range;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    BinOp, Expr, ExprAssign, ExprBinary, ExprCall, ExprIf, ExprLit, ExprMethodCall, ExprRepeat,
    ExprUnary, ExprWhile, Lit, Local, TypeArray, UnOp,
};

use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct BinaryOperatorMutator {
    name: &'static str,
    replacement: fn(&BinOp) -> Option<&'static str>,
}

pub(crate) struct ArithmeticNegate;

pub(crate) struct BoolLiteralMutator;

pub(crate) struct ConditionalNotMutator;

impl Mutator for ConditionalNotMutator {
    fn name(&self) -> &str {
        "conditional/not"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = ConditionalNotVisitor {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct ConditionalNotVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for ConditionalNotVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        self.add_expression(&expression.cond);
        visit::visit_expr_if(self, expression);
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.add_expression(&expression.cond);
        visit::visit_expr_while(self, expression);
    }

    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        if matches!(expression.op, BinOp::And(_) | BinOp::Or(_)) {
            self.add_expression(&expression.left);
            self.add_expression(&expression.right);
        }
        visit::visit_expr_binary(self, expression);
    }
}

impl ConditionalNotVisitor<'_> {
    fn add_expression(&mut self, expression: &Expr) {
        let Expr::Unary(ExprUnary {
            op: UnOp::Not(operator),
            ..
        }) = expression
        else {
            return;
        };
        if let Some(range) = span_range(self.source, operator.span()) {
            self.mutations.push(Mutation::new(range, ""));
        }
    }
}

impl Mutator for BoolLiteralMutator {
    fn name(&self) -> &str {
        "conditional/bool-literal"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = BoolLiteralVisitor {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct BoolLiteralVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for BoolLiteralVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(initializer) = &local.init {
            self.add_expression(&initializer.expr);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_assign(&mut self, assignment: &'ast ExprAssign) {
        self.add_expression(&assignment.right);
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        for argument in &call.args {
            self.add_expression(argument);
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        for argument in &call.args {
            self.add_expression(argument);
        }
        visit::visit_expr_method_call(self, call);
    }
}

impl BoolLiteralVisitor<'_> {
    fn add_expression(&mut self, expression: &Expr) {
        let Expr::Lit(ExprLit {
            lit: Lit::Bool(literal),
            ..
        }) = expression
        else {
            return;
        };
        let replacement = if literal.value { "false" } else { "true" };
        if let Some(range) = span_range(self.source, literal.span()) {
            self.mutations.push(Mutation::new(range, replacement));
        }
    }
}

pub(crate) struct StringLiteralMutator;

impl Mutator for StringLiteralMutator {
    fn name(&self) -> &str {
        "expression/string-literal"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = StringComparisonVisitor {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct StringComparisonVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for StringComparisonVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        if matches!(expression.op, BinOp::Eq(_) | BinOp::Ne(_)) {
            self.add_operand(&expression.left);
            self.add_operand(&expression.right);
        }
        visit::visit_expr_binary(self, expression);
    }
}

impl StringComparisonVisitor<'_> {
    fn add_operand(&mut self, expression: &Expr) {
        let Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        }) = expression
        else {
            return;
        };
        if literal.value().is_empty() {
            return;
        }
        if let Some(range) = span_range(self.source, literal.span()) {
            self.mutations.push(Mutation::new(range, "\"\""));
        }
    }
}

pub(crate) struct NumberMutator {
    name: &'static str,
    change: NumberChange,
}

#[derive(Clone, Copy)]
enum NumberChange {
    Adjust(i8),
    ZeroFloat,
}

impl NumberMutator {
    pub(crate) fn incrementer() -> Self {
        Self {
            name: "numbers/incrementer",
            change: NumberChange::Adjust(1),
        }
    }

    pub(crate) fn decrementer() -> Self {
        Self {
            name: "numbers/decrementer",
            change: NumberChange::Adjust(-1),
        }
    }

    pub(crate) fn float_negate() -> Self {
        Self {
            name: "numbers/float-negate",
            change: NumberChange::ZeroFloat,
        }
    }
}

impl Mutator for NumberMutator {
    fn name(&self) -> &str {
        self.name
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = NumberVisitor {
            source,
            change: self.change,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct NumberVisitor<'a> {
    source: &'a str,
    change: NumberChange,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for NumberVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_lit(&mut self, expression: &'ast ExprLit) {
        let replacement = match (&expression.lit, self.change) {
            (Lit::Int(literal), NumberChange::Adjust(amount)) => {
                adjust_integer(self.source, literal, amount)
            }
            (Lit::Float(literal), NumberChange::Adjust(amount)) => {
                adjust_float(self.source, literal, amount)
            }
            (Lit::Float(literal), NumberChange::ZeroFloat) => zero_float(self.source, literal),
            _ => None,
        };
        if let Some((range, replacement)) = replacement {
            self.mutations.push(Mutation::new(range, replacement));
        }
    }

    fn visit_expr_repeat(&mut self, expression: &'ast ExprRepeat) {
        self.visit_expr(&expression.expr);
    }

    fn visit_type_array(&mut self, array: &'ast TypeArray) {
        self.visit_type(&array.elem);
    }
}

fn adjust_integer(
    source: &str,
    literal: &syn::LitInt,
    amount: i8,
) -> Option<(Range<usize>, String)> {
    let range = span_range(source, literal.span())?;
    let token = source.get(range.clone())?;
    let suffix = literal.suffix();
    let digits = token.strip_suffix(suffix)?;
    if !digits
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    let value = adjusted_integer(literal, suffix, amount)?;
    Some((range, format!("{value}{suffix}")))
}

fn adjusted_integer(literal: &syn::LitInt, suffix: &str, amount: i8) -> Option<String> {
    if let Some(maximum) = unsigned_integer_maximum(suffix) {
        let value = literal.base10_parse::<u128>().ok()?;
        let adjusted = if amount.is_positive() {
            value.checked_add(1)?
        } else {
            value.checked_sub(1)?
        };
        if adjusted > maximum {
            return None;
        }
        return Some(adjusted.to_string());
    }
    let value = literal.base10_parse::<i128>().ok()?;
    let adjusted = value.checked_add(i128::from(amount))?;
    let (minimum, maximum) = signed_integer_bounds(suffix)?;
    if adjusted < minimum || adjusted > maximum {
        return None;
    }
    Some(adjusted.to_string())
}

fn unsigned_integer_maximum(suffix: &str) -> Option<u128> {
    match suffix {
        "u8" => Some(u128::from(u8::MAX)),
        "u16" => Some(u128::from(u16::MAX)),
        "u32" => Some(u128::from(u32::MAX)),
        "u64" => Some(u128::from(u64::MAX)),
        "u128" => Some(u128::MAX),
        "usize" => Some(usize::MAX as u128),
        _ => None,
    }
}

fn signed_integer_bounds(suffix: &str) -> Option<(i128, i128)> {
    match suffix {
        "i8" => Some((i128::from(i8::MIN), i128::from(i8::MAX))),
        "i16" => Some((i128::from(i16::MIN), i128::from(i16::MAX))),
        "i32" => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        "i64" => Some((i128::from(i64::MIN), i128::from(i64::MAX))),
        "isize" => Some((isize::MIN as i128, isize::MAX as i128)),
        "" | "i128" => Some((i128::MIN, i128::MAX)),
        _ => None,
    }
}

fn adjust_float(
    source: &str,
    literal: &syn::LitFloat,
    amount: i8,
) -> Option<(Range<usize>, String)> {
    let range = span_range(source, literal.span())?;
    let token = source.get(range.clone())?;
    let suffix = literal.suffix();
    let digits = token.strip_suffix(suffix)?;
    if !digits.bytes().all(|byte| {
        byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'e' | b'E' | b'+' | b'-')
    }) {
        return None;
    }
    match suffix {
        "f32" => {
            let original = literal.base10_parse::<f32>().ok()?;
            let value = original + f32::from(amount);
            finite_float(range, original, value, suffix)
        }
        "" | "f64" => {
            let original = literal.base10_parse::<f64>().ok()?;
            let value = original + f64::from(amount);
            finite_float(range, original, value, suffix)
        }
        _ => None,
    }
}

fn zero_float(source: &str, literal: &syn::LitFloat) -> Option<(Range<usize>, String)> {
    let range = span_range(source, literal.span())?;
    let valid = match literal.suffix() {
        "f32" => literal
            .base10_parse::<f32>()
            .ok()
            .is_some_and(|value| value.is_finite() && value != 0.0),
        "" | "f64" => literal
            .base10_parse::<f64>()
            .ok()
            .is_some_and(|value| value.is_finite() && value != 0.0),
        _ => false,
    };
    if !valid {
        return None;
    }
    Some((range, format!("0.0{}", literal.suffix())))
}

fn finite_float<T>(
    range: Range<usize>,
    original: T,
    value: T,
    suffix: &str,
) -> Option<(Range<usize>, String)>
where
    T: Copy + PartialEq + fmt::Display + FloatValue,
{
    if !value.finite() || value == original {
        return None;
    }
    let mut replacement = value.to_string();
    if !replacement.contains(['.', 'e', 'E']) {
        replacement.push_str(".0");
    }
    replacement.push_str(suffix);
    Some((range, replacement))
}

trait FloatValue {
    fn finite(self) -> bool;
}

impl FloatValue for f32 {
    fn finite(self) -> bool {
        self.is_finite()
    }
}

impl FloatValue for f64 {
    fn finite(self) -> bool {
        self.is_finite()
    }
}

impl Mutator for ArithmeticNegate {
    fn name(&self) -> &str {
        "arithmetic/negate"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = UnaryMinusVisitor {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct UnaryMinusVisitor<'a> {
    source: &'a str,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for UnaryMinusVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_unary(&mut self, expression: &'ast ExprUnary) {
        if let UnOp::Neg(operator) = &expression.op {
            if let Some(range) = span_range(self.source, operator.span()) {
                self.mutations.push(Mutation::new(range, ""));
            }
        }
        visit::visit_expr_unary(self, expression);
    }
}

impl BinaryOperatorMutator {
    pub(crate) fn arithmetic_base() -> Self {
        Self {
            name: "arithmetic/base",
            replacement: |operator| match operator {
                BinOp::Add(_) => Some("-"),
                BinOp::Sub(_) => Some("+"),
                BinOp::Mul(_) => Some("/"),
                BinOp::Div(_) => Some("*"),
                BinOp::Rem(_) => Some("*"),
                _ => None,
            },
        }
    }

    pub(crate) fn arithmetic_bitwise() -> Self {
        Self {
            name: "arithmetic/bitwise",
            replacement: |operator| match operator {
                BinOp::BitAnd(_) => Some("|"),
                BinOp::BitOr(_) => Some("&"),
                BinOp::BitXor(_) => Some("&"),
                BinOp::Shl(_) => Some(">>"),
                BinOp::Shr(_) => Some("<<"),
                _ => None,
            },
        }
    }

    pub(crate) fn arithmetic_assign_invert() -> Self {
        Self {
            name: "arithmetic/assign_invert",
            replacement: assign_invert_replacement,
        }
    }

    pub(crate) fn arithmetic_assignment() -> Self {
        Self {
            name: "arithmetic/assignment",
            replacement: |operator| match operator {
                BinOp::AddAssign(_)
                | BinOp::SubAssign(_)
                | BinOp::MulAssign(_)
                | BinOp::DivAssign(_)
                | BinOp::RemAssign(_)
                | BinOp::BitAndAssign(_)
                | BinOp::BitOrAssign(_)
                | BinOp::BitXorAssign(_)
                | BinOp::ShlAssign(_)
                | BinOp::ShrAssign(_) => Some("="),
                _ => None,
            },
        }
    }

    pub(crate) fn conditional_negated() -> Self {
        Self {
            name: "conditional/negated",
            replacement: |operator| match operator {
                BinOp::Gt(_) => Some("<="),
                BinOp::Lt(_) => Some(">="),
                BinOp::Ge(_) => Some("<"),
                BinOp::Le(_) => Some(">"),
                BinOp::Eq(_) => Some("!="),
                BinOp::Ne(_) => Some("=="),
                _ => None,
            },
        }
    }

    pub(crate) fn expression_comparison() -> Self {
        Self {
            name: "expression/comparison",
            replacement: |operator| match operator {
                BinOp::Lt(_) => Some("<="),
                BinOp::Le(_) => Some("<"),
                BinOp::Gt(_) => Some(">="),
                BinOp::Ge(_) => Some(">"),
                _ => None,
            },
        }
    }

    pub(crate) fn expression_logical() -> Self {
        Self {
            name: "expression/logical",
            replacement: |operator| match operator {
                BinOp::And(_) => Some("||"),
                BinOp::Or(_) => Some("&&"),
                _ => None,
            },
        }
    }
}

fn assign_invert_replacement(operator: &BinOp) -> Option<&'static str> {
    arithmetic_assign_replacement(operator).or_else(|| bitwise_assign_replacement(operator))
}

fn arithmetic_assign_replacement(operator: &BinOp) -> Option<&'static str> {
    match operator {
        BinOp::AddAssign(_) => Some("-="),
        BinOp::SubAssign(_) => Some("+="),
        BinOp::MulAssign(_) => Some("/="),
        BinOp::DivAssign(_) => Some("*="),
        BinOp::RemAssign(_) => Some("*="),
        _ => None,
    }
}

fn bitwise_assign_replacement(operator: &BinOp) -> Option<&'static str> {
    match operator {
        BinOp::BitAndAssign(_) => Some("|="),
        BinOp::BitOrAssign(_) => Some("&="),
        BinOp::BitXorAssign(_) => Some("&="),
        BinOp::ShlAssign(_) => Some(">>="),
        BinOp::ShrAssign(_) => Some("<<="),
        _ => None,
    }
}

impl Mutator for BinaryOperatorMutator {
    fn name(&self) -> &str {
        self.name
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = BinaryOperatorVisitor {
            source,
            replacement: self.replacement,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct BinaryOperatorVisitor<'a> {
    source: &'a str,
    replacement: fn(&BinOp) -> Option<&'static str>,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for BinaryOperatorVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        if let Some(replacement) = (self.replacement)(&expression.op) {
            if let Some(range) = span_range(self.source, expression.op.span()) {
                self.mutations.push(Mutation::new(range, replacement));
            }
        }
        visit::visit_expr_binary(self, expression);
    }
}
