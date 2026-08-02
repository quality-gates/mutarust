use syn::spanned::Spanned;

use crate::error_panic_cleanup::{StandardCrates, path_names_are, standard_crates};
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct ErrorWrapMutator;

impl Mutator for ErrorWrapMutator {
    fn name(&self) -> &str {
        "expression/errorf-wrap"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let crates = standard_crates(&file.items);
        if !crates.core_available {
            return Vec::new();
        }
        let mut mutations = Vec::new();
        collect_mutations(source, &file.items, crates, &mut mutations);
        mutations
    }
}

fn collect_mutations(
    source: &str,
    items: &[syn::Item],
    crates: StandardCrates,
    mutations: &mut Vec<Mutation>,
) {
    for item in items {
        match item {
            syn::Item::Impl(item) if standard_error_impl(item, crates) => {
                mutations.extend(source_method_mutations(source, item, crates));
            }
            syn::Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    collect_mutations(source, items, crates, mutations);
                }
            }
            _ => {}
        }
    }
}

fn standard_error_impl(item: &syn::ItemImpl, crates: StandardCrates) -> bool {
    let Some((_, path, _)) = &item.trait_ else {
        return false;
    };
    if path.leading_colon.is_none() {
        return false;
    }
    (path_names_are(path, &["core", "error", "Error"]) && crates.core_available)
        || (path_names_are(path, &["std", "error", "Error"]) && crates.std_available)
}

fn source_method_mutations(
    source: &str,
    item: &syn::ItemImpl,
    crates: StandardCrates,
) -> Vec<Mutation> {
    item.items
        .iter()
        .filter_map(|member| match member {
            syn::ImplItem::Fn(method) if method.sig.ident == "source" => Some(method),
            _ => None,
        })
        .filter_map(|method| method.block.stmts.last())
        .filter_map(|statement| match statement {
            syn::Stmt::Expr(expression, None) => some_call(expression, crates),
            _ => None,
        })
        .filter_map(|expression| {
            span_range(source, expression.span())
                .map(|range| Mutation::new(range, "::core::option::Option::None"))
        })
        .collect()
}

fn some_call(expression: &syn::Expr, crates: StandardCrates) -> Option<&syn::Expr> {
    let syn::Expr::Call(call) = expression else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    let syn::Expr::Path(function) = call.func.as_ref() else {
        return None;
    };
    if function.qself.is_some() || function.path.leading_colon.is_none() {
        return None;
    }
    ((path_names_are(&function.path, &["core", "option", "Option", "Some"])
        && crates.core_available)
        || (path_names_are(&function.path, &["std", "option", "Option", "Some"])
            && crates.std_available))
        .then_some(expression)
}
