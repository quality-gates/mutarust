use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Token};

use super::bindings::Bindings;
use crate::{Mutation, Mutator};

pub(crate) struct SelectionMutator {
    name: &'static str,
    kind: SelectionKind,
}

impl SelectionMutator {
    pub(crate) const fn case_remove() -> Self {
        Self {
            name: "select/case-remove",
            kind: SelectionKind::Case,
        }
    }

    pub(crate) const fn default_remove() -> Self {
        Self {
            name: "select/default-remove",
            kind: SelectionKind::Default,
        }
    }
}

impl Mutator for SelectionMutator {
    fn name(&self) -> &str {
        self.name
    }

    fn mutations_from_parsed(&self, source: &str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = SelectionVisitor {
            source,
            kind: self.kind,
            bindings: Bindings::for_crate(&file.items),
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

#[derive(Clone, Copy)]
enum SelectionKind {
    Case,
    Default,
}

struct SelectionVisitor<'source> {
    source: &'source str,
    kind: SelectionKind,
    bindings: Bindings,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for SelectionVisitor<'_> {
    fn visit_macro(&mut self, expression: &'ast syn::Macro) {
        if !is_tokio_select(&expression.path, self.bindings) {
            return;
        }
        let Ok(input) = syn::parse2::<SelectInput>(expression.tokens.clone()) else {
            return;
        };
        add_select_mutations(self.source, self.kind, &input, &mut self.mutations);
        for branch in &input.branches {
            self.visit_expr(&branch.future);
            if let Some(condition) = &branch.condition {
                self.visit_expr(condition);
            }
            self.visit_expr(&branch.handler);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.with_generic_bindings(&item.sig.generics, |visitor| {
            visit::visit_item_fn(visitor, item)
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.with_generic_bindings(&item.generics, |visitor| {
            visit::visit_item_impl(visitor, item)
        });
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.with_generic_bindings(&item.generics, |visitor| {
            visit::visit_item_trait(visitor, item)
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.with_generic_bindings(&item.sig.generics, |visitor| {
            visit::visit_impl_item_fn(visitor, item)
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.with_generic_bindings(&item.sig.generics, |visitor| {
            visit::visit_trait_item_fn(visitor, item)
        });
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let Some((_, items)) = &item.content else {
            return;
        };
        let bindings = self.bindings.for_nested_module(items);
        self.with_bindings(bindings, |visitor| {
            for item in items {
                visitor.visit_item(item);
            }
        });
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let bindings = self.bindings.with_block_items(&block.stmts);
        self.with_bindings(bindings, |visitor| visit::visit_block(visitor, block));
    }
}

impl SelectionVisitor<'_> {
    fn with_generic_bindings(&mut self, generics: &syn::Generics, visit: impl FnOnce(&mut Self)) {
        self.with_bindings(self.bindings.with_generics(generics), visit);
    }

    fn with_bindings(&mut self, bindings: Bindings, visit: impl FnOnce(&mut Self)) {
        let previous = self.bindings;
        self.bindings = bindings;
        visit(self);
        self.bindings = previous;
    }
}

fn add_select_mutations(
    source: &str,
    kind: SelectionKind,
    input: &SelectInput,
    mutations: &mut Vec<Mutation>,
) {
    let normal_count = input
        .branches
        .iter()
        .filter(|branch| !branch.fallback)
        .count();
    for branch in &input.branches {
        let eligible = match kind {
            SelectionKind::Case => !branch.fallback && input.branches.len() >= 2,
            SelectionKind::Default => branch.fallback && normal_count > 0,
        };
        if !eligible {
            continue;
        }
        let Some(range) = source_range(source, branch.start, branch.end) else {
            continue;
        };
        mutations.push(Mutation::new(range, "").requiring_compile_validation());
    }
}

struct SelectInput {
    branches: Vec<SelectBranch>,
}

struct SelectBranch {
    fallback: bool,
    start: Span,
    end: Span,
    future: Expr,
    condition: Option<Expr>,
    handler: Expr,
}

impl Parse for SelectInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        parse_biased_prefix(input)?;
        let mut branches = Vec::new();
        let mut found_fallback = false;
        while !input.is_empty() {
            let branch = if input.peek(Token![else]) {
                if found_fallback {
                    return Err(input.error("a Tokio selection can have one fallback"));
                }
                found_fallback = true;
                parse_fallback(input)?
            } else {
                if found_fallback {
                    return Err(input.error("the Tokio fallback must be last"));
                }
                parse_case(input)?
            };
            branches.push(branch);
        }
        if branches.is_empty() {
            return Err(input.error("a Tokio selection needs a branch"));
        }
        Ok(Self { branches })
    }
}

fn parse_biased_prefix(input: ParseStream<'_>) -> syn::Result<()> {
    if !input.peek(syn::Ident) || !input.peek2(Token![;]) {
        return Ok(());
    }
    let name: syn::Ident = input.parse()?;
    if name != "biased" {
        return Err(syn::Error::new(
            name.span(),
            "unsupported Tokio select prefix",
        ));
    }
    input.parse::<Token![;]>()?;
    Ok(())
}

fn parse_case(input: ParseStream<'_>) -> syn::Result<SelectBranch> {
    let pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
    input.parse::<Token![=]>()?;
    let future = input.parse()?;
    let condition = if input.peek(Token![,]) && input.peek2(Token![if]) {
        input.parse::<Token![,]>()?;
        input.parse::<Token![if]>()?;
        Some(input.parse()?)
    } else {
        None
    };
    input.parse::<Token![=>]>()?;
    let handler: Expr = input.parse()?;
    let comma = input.parse::<Option<Token![,]>>()?;
    Ok(SelectBranch {
        fallback: false,
        start: pattern.span(),
        end: comma.map_or_else(|| handler.span(), |comma| comma.span),
        future,
        condition,
        handler,
    })
}

fn parse_fallback(input: ParseStream<'_>) -> syn::Result<SelectBranch> {
    let keyword = input.parse::<Token![else]>()?;
    input.parse::<Token![=>]>()?;
    let handler: Expr = input.parse()?;
    let comma = input.parse::<Option<Token![,]>>()?;
    Ok(SelectBranch {
        fallback: true,
        start: keyword.span,
        end: comma.map_or_else(|| handler.span(), |comma| comma.span),
        future: syn::parse_quote!(()),
        condition: None,
        handler,
    })
}

fn is_tokio_select(path: &syn::Path, bindings: Bindings) -> bool {
    let names = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    names == ["tokio", "select"] && bindings.tokio_path_available(path.leading_colon.is_some())
}

fn source_range(source: &str, start: Span, end: Span) -> Option<std::ops::Range<usize>> {
    let start = start.byte_range().start;
    let end = end.byte_range().end;
    (start < end
        && end <= source.len()
        && source.is_char_boundary(start)
        && source.is_char_boundary(end))
    .then_some(start..end)
}
