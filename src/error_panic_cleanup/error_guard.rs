use std::collections::BTreeMap;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprIf, FnArg, Pat, Stmt, Type};

use crate::error_panic_cleanup::crate_root_aliases;
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
        ErrorGuardVisitor::collect(source, &file, crate_root_aliases(&file.items))
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
    fn collect(source: &'source str, file: &syn::File, aliases: (bool, bool)) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            core_available: !aliases.0,
            scopes: Vec::new(),
            std_available: !aliases.1,
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
        let names = kind
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        (names == ["core", "result", "Result"] && self.core_available)
            || (names == ["std", "result", "Result"] && self.std_available)
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

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        let Expr::MethodCall(call) = expression.cond.as_ref() else {
            visit::visit_expr_if(self, expression);
            return;
        };
        let replacement = match call.method.to_string().as_str() {
            "is_err" if call.args.is_empty() => "false",
            "is_ok" if call.args.is_empty() => "true",
            _ => {
                visit::visit_expr_if(self, expression);
                return;
            }
        };
        if self.result_is_proved(&call.receiver)
            && let Some(range) = span_range(self.source, expression.cond.span())
        {
            self.mutations.push(Mutation::new(range, replacement));
        }
        visit::visit_expr_if(self, expression);
    }
}

fn record_pattern(bindings: &mut BTreeMap<String, bool>, pattern: &Pat, is_result: bool) {
    match pattern {
        Pat::Ident(pattern) => {
            bindings.insert(pattern.ident.to_string(), is_result);
        }
        Pat::Paren(pattern) => record_pattern(bindings, &pattern.pat, is_result),
        Pat::Reference(pattern) => record_pattern(bindings, &pattern.pat, is_result),
        Pat::Type(pattern) => record_pattern(bindings, &pattern.pat, is_result),
        _ => {}
    }
}
