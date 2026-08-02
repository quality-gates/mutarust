use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Arm, Expr, ExprClosure, ExprForLoop, ExprIf, ExprWhile, FnArg, Pat, Stmt, Type};

use crate::error_panic_cleanup::{
    StandardCrates, condition_binding_names, path_names_are, standard_crates,
};
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct ErrorGuardMutator;

impl Mutator for ErrorGuardMutator {
    fn name(&self) -> &str {
        "expression/error-guard"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        ErrorGuardVisitor::collect(source, &file, standard_crates(&file.items))
    }
}

struct ErrorGuardVisitor<'source> {
    source: &'source str,
    core_available: bool,
    scopes: Vec<BTreeMap<String, bool>>,
    std_available: bool,
    mutations: Vec<Mutation>,
}

impl<'source> ErrorGuardVisitor<'source> {
    fn collect(source: &'source str, file: &syn::File, crates: StandardCrates) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            core_available: crates.core_available,
            scopes: Vec::new(),
            std_available: crates.std_available,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }

    fn visit_function(&mut self, signature: &syn::Signature, block: &syn::Block) {
        let mut bindings = BTreeMap::new();
        for input in &signature.inputs {
            if let FnArg::Typed(input) = input {
                let is_result = self.is_result_type(&input.ty);
                record_pattern(&mut bindings, &input.pat, is_result);
            }
        }
        self.scopes.push(bindings);
        self.visit_block(block);
        self.scopes.pop();
    }

    fn result_is_proved(&self, receiver: &Expr) -> bool {
        let Expr::Path(path) = receiver else {
            return false;
        };
        if path.qself.is_some() || path.path.leading_colon.is_some() {
            return false;
        }
        let Some(identifier) = path.path.get_ident() else {
            return false;
        };
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(identifier.to_string().as_str()))
            .copied()
            .unwrap_or(false)
    }

    fn is_result_type(&self, kind: &Type) -> bool {
        let Type::Path(kind) = kind else {
            return false;
        };
        if kind.qself.is_some() || kind.path.leading_colon.is_none() {
            return false;
        }
        (path_names_are(&kind.path, &["core", "result", "Result"]) && self.core_available)
            || (path_names_are(&kind.path, &["std", "result", "Result"]) && self.std_available)
    }
}

impl<'ast> Visit<'ast> for ErrorGuardVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.visit_function(&function.sig, &function.block);
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        self.visit_function(&function.sig, &function.block);
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if let Some(block) = &function.default {
            self.visit_function(&function.sig, block);
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scopes.push(BTreeMap::new());
        for statement in &block.stmts {
            visit::visit_stmt(self, statement);
            if let Stmt::Local(local) = statement {
                let is_result = match &local.pat {
                    Pat::Type(pattern) => self.is_result_type(&pattern.ty),
                    _ => false,
                };
                let scope = self.scopes.last_mut().expect("block scope must exist");
                record_pattern(scope, &local.pat, is_result);
            }
        }
        self.scopes.pop();
    }

    fn visit_expr_closure(&mut self, expression: &'ast ExprClosure) {
        let mut bindings = BTreeMap::new();
        for input in &expression.inputs {
            let is_result = match input {
                Pat::Type(pattern) => self.is_result_type(&pattern.ty),
                _ => false,
            };
            record_pattern(&mut bindings, input, is_result);
        }
        self.scopes.push(bindings);
        self.visit_expr(&expression.body);
        self.scopes.pop();
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.visit_expr(&expression.expr);
        let mut bindings = BTreeMap::new();
        record_pattern(&mut bindings, &expression.pat, false);
        self.scopes.push(bindings);
        self.visit_block(&expression.body);
        self.scopes.pop();
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        let mut bindings = BTreeMap::new();
        record_pattern(&mut bindings, &arm.pat, false);
        self.scopes.push(bindings);
        if let Some((_, guard)) = &arm.guard {
            self.visit_expr(guard);
        }
        self.visit_expr(&arm.body);
        self.scopes.pop();
    }

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        if let Some((call, replacement)) = error_guard_call(&expression.cond)
            && self.result_is_proved(&call.receiver)
            && let Some(range) = span_range(self.source, expression.cond.span())
        {
            self.mutations.push(Mutation::new(range, replacement));
        }
        self.visit_expr(&expression.cond);
        let bindings = condition_binding_names(&expression.cond)
            .into_iter()
            .map(|name| (name, false))
            .collect();
        self.scopes.push(bindings);
        self.visit_block(&expression.then_branch);
        self.scopes.pop();
        if let Some((_, alternate)) = &expression.else_branch {
            self.visit_expr(alternate);
        }
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.visit_expr(&expression.cond);
        let bindings = condition_binding_names(&expression.cond)
            .into_iter()
            .map(|name| (name, false))
            .collect();
        self.scopes.push(bindings);
        self.visit_block(&expression.body);
        self.scopes.pop();
    }
}

fn error_guard_call(expression: &Expr) -> Option<(&syn::ExprMethodCall, &'static str)> {
    let Expr::MethodCall(call) = expression else {
        return None;
    };
    match call.method.to_string().as_str() {
        "is_err" if call.args.is_empty() => Some((call, "false")),
        "is_ok" if call.args.is_empty() => Some((call, "true")),
        _ => None,
    }
}

fn record_pattern(bindings: &mut BTreeMap<String, bool>, pattern: &Pat, is_result: bool) {
    let mut recorder = PatternRecorder {
        bindings,
        is_result,
    };
    recorder.visit_pat(pattern);
}

struct PatternRecorder<'bindings> {
    bindings: &'bindings mut BTreeMap<String, bool>,
    is_result: bool,
}

impl<'ast> Visit<'ast> for PatternRecorder<'_> {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.bindings
            .insert(pattern.ident.to_string(), self.is_result);
        visit::visit_pat_ident(self, pattern);
    }
}
