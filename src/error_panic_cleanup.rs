mod cleanup;
mod error_guard;
mod error_wrap;
mod recovery;

use std::collections::BTreeSet;

use syn::visit::{self, Visit};

pub(crate) use cleanup::CleanupMutator;
pub(crate) use error_guard::ErrorGuardMutator;
pub(crate) use error_wrap::ErrorWrapMutator;
pub(crate) use recovery::RecoveryMutator;

#[derive(Clone, Copy)]
struct StandardCrates {
    core_available: bool,
    std_available: bool,
}

fn standard_crates(items: &[syn::Item]) -> StandardCrates {
    let mut core_available = true;
    let mut std_available = true;
    for item in items {
        let syn::Item::ExternCrate(item) = item else {
            continue;
        };
        let local = item
            .rename
            .as_ref()
            .map_or(&item.ident, |(_, rename)| rename);
        core_available &= local != "core" || item.ident == "core";
        std_available &= local != "std" || item.ident == "std";
    }
    StandardCrates {
        core_available,
        std_available,
    }
}

fn path_names_are(path: &syn::Path, expected: &[&str]) -> bool {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(expected.iter().copied())
}

fn condition_binding_names(expression: &syn::Expr) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_condition_binding_names(expression, &mut names);
    names
}

fn collect_condition_binding_names(expression: &syn::Expr, names: &mut BTreeSet<String>) {
    match expression {
        syn::Expr::Binary(expression) => {
            collect_condition_binding_names(&expression.left, names);
            collect_condition_binding_names(&expression.right, names);
        }
        syn::Expr::Group(expression) => {
            collect_condition_binding_names(&expression.expr, names);
        }
        syn::Expr::Let(expression) => {
            let mut collector = PatternNameCollector { names };
            collector.visit_pat(&expression.pat);
        }
        syn::Expr::Paren(expression) => {
            collect_condition_binding_names(&expression.expr, names);
        }
        _ => {}
    }
}

struct PatternNameCollector<'names> {
    names: &'names mut BTreeSet<String>,
}

impl<'ast> Visit<'ast> for PatternNameCollector<'_> {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.names.insert(pattern.ident.to_string());
        visit::visit_pat_ident(self, pattern);
    }
}
