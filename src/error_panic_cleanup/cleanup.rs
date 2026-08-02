use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::error_panic_cleanup::crate_root_aliases;
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct CleanupMutator;

impl Mutator for CleanupMutator {
    fn name(&self) -> &str {
        "statement/defer-remove"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let aliases = crate_root_aliases(&file.items);
        let mut shadow_visitor = DropShadowVisitor::default();
        shadow_visitor.visit_file(&file);
        let mut visitor = CleanupVisitor {
            source,
            core_available: !aliases.0,
            drop_available: !shadow_visitor.shadowed,
            std_available: !aliases.1,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct CleanupVisitor<'source> {
    source: &'source str,
    core_available: bool,
    drop_available: bool,
    std_available: bool,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for CleanupVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_block(&mut self, block: &'ast syn::Block) {
        for statement in &block.stmts {
            if cleanup_statement(
                statement,
                self.core_available,
                self.drop_available,
                self.std_available,
            ) && let Some(range) = span_range(self.source, statement.span())
            {
                self.mutations
                    .push(Mutation::new(range, "").requiring_compile_validation());
            }
            visit::visit_stmt(self, statement);
        }
    }
}

fn cleanup_statement(
    statement: &syn::Stmt,
    core_available: bool,
    drop_available: bool,
    std_available: bool,
) -> bool {
    semicolon_call(statement)
        .and_then(drop_function_path)
        .is_some_and(|path| {
            supported_drop_path(path, core_available, drop_available, std_available)
        })
}

fn semicolon_call(statement: &syn::Stmt) -> Option<&syn::ExprCall> {
    match statement {
        syn::Stmt::Expr(syn::Expr::Call(call), Some(_)) if call.args.len() == 1 => Some(call),
        _ => None,
    }
}

fn drop_function_path(call: &syn::ExprCall) -> Option<&syn::Path> {
    match call.func.as_ref() {
        syn::Expr::Path(function) if function.qself.is_none() => Some(&function.path),
        _ => None,
    }
}

fn supported_drop_path(
    path: &syn::Path,
    core_available: bool,
    drop_available: bool,
    std_available: bool,
) -> bool {
    unqualified_drop(path, drop_available)
        || root_drop(path, &["core", "mem", "drop"], core_available)
        || root_drop(path, &["std", "mem", "drop"], std_available)
}

fn unqualified_drop(path: &syn::Path, available: bool) -> bool {
    available && path.leading_colon.is_none() && path_names_are(path, &["drop"])
}

fn root_drop(path: &syn::Path, names: &[&str], available: bool) -> bool {
    available && path.leading_colon.is_some() && path_names_are(path, names)
}

fn path_names_are(path: &syn::Path, expected: &[&str]) -> bool {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(expected.iter().copied())
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
