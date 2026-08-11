use std::collections::{BTreeMap, BTreeSet};

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Arm, Expr, ExprClosure, ExprForLoop, ExprIf, ExprWhile, FnArg, Pat, Stmt, Type};

use crate::error_panic_cleanup::{
    StandardCrates, condition_binding_names, path_names_are, standard_crates,
};
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct CleanupMutator;

impl Mutator for CleanupMutator {
    fn name(&self) -> &str {
        "statement/defer-remove"
    }

    fn mutations_from_parsed(&self, source: &str, file: &syn::File) -> Vec<Mutation> {
        let crates = standard_crates(&file.items);
        let mut drop_types = DropTypeVisitor::new(crates);
        drop_types.visit_file(file);
        let mut shadow_visitor = DropShadowVisitor::default();
        shadow_visitor.visit_file(file);
        let mut visitor = CleanupVisitor {
            source,
            crates,
            drop_available: !shadow_visitor.shadowed,
            drop_types: drop_types.names,
            module_path: Vec::new(),
            scopes: Vec::new(),
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

struct CleanupVisitor<'source> {
    source: &'source str,
    crates: StandardCrates,
    drop_available: bool,
    drop_types: BTreeSet<LocalDropType>,
    module_path: Vec<String>,
    scopes: Vec<BTreeMap<String, bool>>,
    mutations: Vec<Mutation>,
}

impl CleanupVisitor<'_> {
    fn visit_function(&mut self, signature: &syn::Signature, block: &syn::Block) {
        let mut bindings = BTreeMap::new();
        for input in &signature.inputs {
            if let FnArg::Typed(input) = input {
                let has_cleanup = self.type_has_cleanup(&input.ty);
                record_pattern(&mut bindings, &input.pat, has_cleanup);
            }
        }
        self.scopes.push(bindings);
        self.visit_block(block);
        self.scopes.pop();
    }

    fn type_has_cleanup(&self, kind: &Type) -> bool {
        let Type::Path(kind) = kind else {
            return false;
        };
        if kind.qself.is_some() {
            return false;
        }
        if kind.path.leading_colon.is_some() {
            return standard_guard_type(&kind.path, self.crates);
        }
        kind.path.segments.len() == 1
            && kind.path.segments.last().is_some_and(|segment| {
                self.drop_types.contains(&LocalDropType::new(
                    &self.module_path,
                    segment.ident.to_string(),
                ))
            })
    }

    fn initializer_has_cleanup(&self, expression: &Expr) -> bool {
        let path = match expression {
            Expr::Call(call) => match call.func.as_ref() {
                Expr::Path(function) if function.qself.is_none() => Some(&function.path),
                _ => None,
            },
            Expr::Struct(expression) if expression.qself.is_none() => Some(&expression.path),
            _ => None,
        };
        path.is_some_and(|path| {
            path.leading_colon.is_none()
                && path.segments.len() == 1
                && path.segments.last().is_some_and(|segment| {
                    self.drop_types.contains(&LocalDropType::new(
                        &self.module_path,
                        segment.ident.to_string(),
                    ))
                })
        })
    }

    fn binding_has_cleanup(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .copied()
            .unwrap_or(false)
    }

    fn is_cleanup_statement(&self, statement: &Stmt) -> bool {
        let Some(call) = semicolon_call(statement) else {
            return false;
        };
        let Some(function) = drop_function_path(call) else {
            return false;
        };
        if !supported_drop_path(function, self.crates, self.drop_available) {
            return false;
        }
        let Some(Expr::Path(value)) = call.args.first() else {
            return false;
        };
        if value.qself.is_some() || value.path.leading_colon.is_some() {
            return false;
        }
        value
            .path
            .get_ident()
            .is_some_and(|identifier| self.binding_has_cleanup(&identifier.to_string()))
    }
}

impl<'ast> Visit<'ast> for CleanupVisitor<'_> {
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

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let Some((_, items)) = &module.content else {
            return;
        };
        self.module_path.push(module.ident.to_string());
        for item in items {
            self.visit_item(item);
        }
        self.module_path.pop();
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scopes.push(BTreeMap::new());
        for (index, statement) in block.stmts.iter().enumerate() {
            if has_later_runtime_statement(&block.stmts, index)
                && self.is_cleanup_statement(statement)
            {
                if let Some(range) = span_range(self.source, statement.span()) {
                    self.mutations
                        .push(Mutation::new(range, "").requiring_compile_validation());
                }
            }
            visit::visit_stmt(self, statement);
            if let Stmt::Local(local) = statement {
                let has_cleanup = match &local.pat {
                    Pat::Type(pattern) => self.type_has_cleanup(&pattern.ty),
                    _ => local
                        .init
                        .as_ref()
                        .is_some_and(|initial| self.initializer_has_cleanup(&initial.expr)),
                };
                let scope = self.scopes.last_mut().expect("block scope must exist");
                record_pattern(scope, &local.pat, has_cleanup);
            }
        }
        self.scopes.pop();
    }

    fn visit_expr_closure(&mut self, expression: &'ast ExprClosure) {
        let mut bindings = BTreeMap::new();
        for input in &expression.inputs {
            let has_cleanup = match input {
                Pat::Type(pattern) => self.type_has_cleanup(&pattern.ty),
                _ => false,
            };
            record_pattern(&mut bindings, input, has_cleanup);
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
        if let Pat::Guard(guard) = &arm.pat {
            self.visit_expr(&guard.guard);
        }
        self.visit_expr(&arm.body);
        self.scopes.pop();
    }

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
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

fn has_later_runtime_statement(statements: &[Stmt], index: usize) -> bool {
    statements[index + 1..]
        .iter()
        .any(|statement| matches!(statement, Stmt::Local(_) | Stmt::Expr(_, _)))
}

fn semicolon_call(statement: &Stmt) -> Option<&syn::ExprCall> {
    match statement {
        Stmt::Expr(Expr::Call(call), Some(_)) if call.args.len() == 1 => Some(call),
        _ => None,
    }
}

fn drop_function_path(call: &syn::ExprCall) -> Option<&syn::Path> {
    match call.func.as_ref() {
        Expr::Path(function) if function.qself.is_none() => Some(&function.path),
        _ => None,
    }
}

fn supported_drop_path(path: &syn::Path, crates: StandardCrates, drop_available: bool) -> bool {
    (drop_available && path.leading_colon.is_none() && path_names_are(path, &["drop"]))
        || (crates.core_available
            && path.leading_colon.is_some()
            && path_names_are(path, &["core", "mem", "drop"]))
        || (crates.std_available
            && path.leading_colon.is_some()
            && path_names_are(path, &["std", "mem", "drop"]))
}

fn standard_guard_type(path: &syn::Path, crates: StandardCrates) -> bool {
    (crates.std_available
        && (path_names_are(path, &["std", "sync", "MutexGuard"])
            || path_names_are(path, &["std", "sync", "RwLockReadGuard"])
            || path_names_are(path, &["std", "sync", "RwLockWriteGuard"])))
        || (crates.core_available
            && (path_names_are(path, &["core", "cell", "Ref"])
                || path_names_are(path, &["core", "cell", "RefMut"])))
}

fn record_pattern(bindings: &mut BTreeMap<String, bool>, pattern: &Pat, has_cleanup: bool) {
    let mut recorder = PatternRecorder {
        bindings,
        has_cleanup,
    };
    recorder.visit_pat(pattern);
}

struct PatternRecorder<'bindings> {
    bindings: &'bindings mut BTreeMap<String, bool>,
    has_cleanup: bool,
}

impl<'ast> Visit<'ast> for PatternRecorder<'_> {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.bindings
            .insert(pattern.ident.to_string(), self.has_cleanup);
        visit::visit_pat_ident(self, pattern);
    }

    fn visit_pat_guard(&mut self, pattern: &'ast syn::PatGuard) {
        // Match-arm guards are Pat::Guard in syn 3. The default walk enters the guard
        // expression and would record closure or if-let bindings as arm bindings.
        self.visit_pat(&pattern.pat);
    }
}

struct DropTypeVisitor {
    crates: StandardCrates,
    module_path: Vec<String>,
    names: BTreeSet<LocalDropType>,
}

impl DropTypeVisitor {
    fn new(crates: StandardCrates) -> Self {
        Self {
            crates,
            module_path: Vec::new(),
            names: BTreeSet::new(),
        }
    }
}

impl<'ast> Visit<'ast> for DropTypeVisitor {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let is_local_drop_impl = item
            .trait_
            .as_ref()
            .is_some_and(|(path, _)| exact_drop_trait(path, self.crates))
            && drop_impl_has_work(item);
        if is_local_drop_impl {
            if let Some(name) = local_impl_type_name(&item.self_ty) {
                self.names
                    .insert(LocalDropType::new(&self.module_path, name));
            }
        }
        visit::visit_item_impl(self, item);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let Some((_, items)) = &module.content else {
            return;
        };
        self.module_path.push(module.ident.to_string());
        for item in items {
            self.visit_item(item);
        }
        self.module_path.pop();
    }
}

fn drop_impl_has_work(item: &syn::ItemImpl) -> bool {
    item.items.iter().any(|member| match member {
        syn::ImplItem::Fn(method) if method.sig.ident == "drop" => method
            .block
            .stmts
            .iter()
            .any(|statement| matches!(statement, Stmt::Local(_) | Stmt::Expr(_, _))),
        _ => false,
    })
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct LocalDropType {
    module_path: Vec<String>,
    name: String,
}

impl LocalDropType {
    fn new(module_path: &[String], name: String) -> Self {
        Self {
            module_path: module_path.to_vec(),
            name,
        }
    }
}

fn exact_drop_trait(path: &syn::Path, crates: StandardCrates) -> bool {
    path.leading_colon.is_some()
        && ((crates.core_available && path_names_are(path, &["core", "ops", "Drop"]))
            || (crates.std_available && path_names_are(path, &["std", "ops", "Drop"])))
}

fn local_impl_type_name(kind: &Type) -> Option<String> {
    let Type::Path(kind) = kind else {
        return None;
    };
    (kind.qself.is_none() && kind.path.leading_colon.is_none() && kind.path.segments.len() == 1)
        .then(|| {
            kind.path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
        })
        .flatten()
}

#[derive(Default)]
struct DropShadowVisitor {
    shadowed: bool,
}

impl<'ast> Visit<'ast> for DropShadowVisitor {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.shadowed |= function.sig.ident == "drop";
        visit::visit_item_fn(self, function);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.shadowed |= item.ident == "drop";
        visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.shadowed |= item.ident == "drop";
        visit::visit_item_static(self, item);
    }

    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.shadowed |= pattern.ident == "drop";
        visit::visit_pat_ident(self, pattern);
    }

    fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
        self.shadowed |= item.rename == "drop";
        visit::visit_use_rename(self, item);
    }

    fn visit_use_name(&mut self, item: &'ast syn::UseName) {
        self.shadowed |= item.ident == "drop";
        visit::visit_use_name(self, item);
    }

    fn visit_use_glob(&mut self, item: &'ast syn::UseGlob) {
        self.shadowed = true;
        visit::visit_use_glob(self, item);
    }
}
