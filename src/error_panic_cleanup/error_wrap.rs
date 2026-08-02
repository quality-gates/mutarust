use syn::spanned::Spanned;

use crate::error_panic_cleanup::crate_root_aliases;
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
        let aliases = crate_root_aliases(&file.items);
        if aliases.0 {
            return Vec::new();
        }
        let mut mutations = Vec::new();
        collect_mutations(source, &file.items, aliases, &mut mutations);
        mutations
    }
}

fn collect_mutations(
    source: &str,
    items: &[syn::Item],
    aliases: (bool, bool),
    mutations: &mut Vec<Mutation>,
) {
    for item in items {
        match item {
            syn::Item::Impl(item) if standard_error_impl(item, aliases) => {
                mutations.extend(source_method_mutations(source, item, aliases));
            }
            syn::Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    collect_mutations(source, items, aliases, mutations);
                }
            }
            _ => {}
        }
    }
}

fn standard_error_impl(item: &syn::ItemImpl, aliases: (bool, bool)) -> bool {
    let Some((_, path, _)) = &item.trait_ else {
        return false;
    };
    if path.leading_colon.is_none() {
        return false;
    }
    let names = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    (names == ["core", "error", "Error"] && !aliases.0)
        || (names == ["std", "error", "Error"] && !aliases.1)
}

fn source_method_mutations(
    source: &str,
    item: &syn::ItemImpl,
    aliases: (bool, bool),
) -> Vec<Mutation> {
    item.items
        .iter()
        .filter_map(|member| match member {
            syn::ImplItem::Fn(method) if method.sig.ident == "source" => Some(method),
            _ => None,
        })
        .filter_map(|method| method.block.stmts.last())
        .filter_map(|statement| match statement {
            syn::Stmt::Expr(expression, None) => some_call(expression, aliases),
            _ => None,
        })
        .filter_map(|expression| {
            span_range(source, expression.span())
                .map(|range| Mutation::new(range, "::core::option::Option::None"))
        })
        .collect()
}

fn some_call(expression: &syn::Expr, aliases: (bool, bool)) -> Option<&syn::Expr> {
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
    let names = function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    ((names == ["core", "option", "Option", "Some"] && !aliases.0)
        || (names == ["std", "option", "Option", "Some"] && !aliases.1))
        .then_some(expression)
}
