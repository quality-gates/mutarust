use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, GenericParam, Item, Stmt, Token, UseTree};

use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct ConcurrencyMutator;

impl Mutator for ConcurrencyMutator {
    fn name(&self) -> &str {
        "concurrency/goroutine-remove"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = ConcurrencyVisitor {
            source,
            async_context: false,
            bindings: Bindings::for_module(&file.items),
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct ConcurrencyVisitor<'source> {
    source: &'source str,
    async_context: bool,
    bindings: Bindings,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for ConcurrencyVisitor<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if let Stmt::Expr(Expr::Call(call), Some(_)) = statement {
            self.add_standard_spawn(call);
            self.add_tokio_spawn(call);
        }
        visit::visit_stmt(self, statement);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.with_generic_bindings(&item.sig.generics, |visitor| {
            visitor.with_async_context(item.sig.asyncness.is_some(), |visitor| {
                visit::visit_item_fn(visitor, item)
            });
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
            visitor.with_async_context(item.sig.asyncness.is_some(), |visitor| {
                visit::visit_impl_item_fn(visitor, item)
            });
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.with_generic_bindings(&item.sig.generics, |visitor| {
            visitor.with_async_context(item.sig.asyncness.is_some(), |visitor| {
                visit::visit_trait_item_fn(visitor, item)
            });
        });
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let Some((_, items)) = &item.content else {
            return;
        };
        let bindings = Bindings::for_module(items);
        self.with_bindings(bindings, |visitor| {
            visitor.with_async_context(false, |visitor| {
                for item in items {
                    visitor.visit_item(item);
                }
            });
        });
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let bindings = self.bindings.with_block_items(&block.stmts);
        self.with_bindings(bindings, |visitor| visit::visit_block(visitor, block));
    }

    fn visit_expr_async(&mut self, expression: &'ast syn::ExprAsync) {
        self.with_async_context(true, |visitor| visit::visit_expr_async(visitor, expression));
    }

    fn visit_expr_closure(&mut self, expression: &'ast syn::ExprClosure) {
        self.with_async_context(expression.asyncness.is_some(), |visitor| {
            visit::visit_expr_closure(visitor, expression)
        });
    }
}

impl ConcurrencyVisitor<'_> {
    fn add_standard_spawn(&mut self, call: &ExprCall) {
        if call.args.len() != 1 || !is_standard_spawn(&call.func, self.bindings) {
            return;
        }
        let argument = &call.args[0];
        let Some(argument_range) = span_range(self.source, argument.span()) else {
            return;
        };
        let Some(call_range) = span_range(self.source, call.span()) else {
            return;
        };
        let argument = &self.source[argument_range];
        self.mutations
            .push(Mutation::new(call_range, format!("({argument})()")));
    }

    fn add_tokio_spawn(&mut self, call: &ExprCall) {
        if !self.async_context || call.args.len() != 1 || !is_tokio_spawn(&call.func, self.bindings)
        {
            return;
        }
        let argument = &call.args[0];
        let Some(argument_range) = span_range(self.source, argument.span()) else {
            return;
        };
        let Some(call_range) = span_range(self.source, call.span()) else {
            return;
        };
        let argument = &self.source[argument_range];
        self.mutations
            .push(Mutation::new(call_range, format!("({argument}).await")));
    }

    fn with_async_context(&mut self, enabled: bool, visit: impl FnOnce(&mut Self)) {
        let previous = self.async_context;
        self.async_context = enabled;
        visit(self);
        self.async_context = previous;
    }

    fn with_bindings(&mut self, bindings: Bindings, visit: impl FnOnce(&mut Self)) {
        let previous = self.bindings;
        self.bindings = bindings;
        visit(self);
        self.bindings = previous;
    }

    fn with_generic_bindings(&mut self, generics: &syn::Generics, visit: impl FnOnce(&mut Self)) {
        let mut bindings = self.bindings;
        for parameter in &generics.params {
            let GenericParam::Type(parameter) = parameter else {
                continue;
            };
            bindings.shadow(parameter.ident.to_string().as_str());
        }
        self.with_bindings(bindings, visit);
    }
}

fn is_standard_spawn(function: &Expr, bindings: Bindings) -> bool {
    let Expr::Path(function) = function else {
        return false;
    };
    let names = function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    let root_qualified = function.path.leading_colon.is_some();
    (names == ["std", "thread", "spawn"] && (root_qualified || !bindings.std_shadowed))
        || (function.path.leading_colon.is_none()
            && bindings.thread_imported
            && names == ["thread", "spawn"])
}

fn is_tokio_spawn(function: &Expr, bindings: Bindings) -> bool {
    let Expr::Path(function) = function else {
        return false;
    };
    let names = function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    let supported = names == ["tokio", "spawn"] || names == ["tokio", "task", "spawn"];
    supported && (function.path.leading_colon.is_some() || !bindings.tokio_shadowed)
}

#[derive(Clone, Copy, Default)]
struct Bindings {
    std_shadowed: bool,
    tokio_shadowed: bool,
    thread_imported: bool,
}

impl Bindings {
    fn for_module(items: &[Item]) -> Self {
        let mut bindings = Self::default();
        bindings.add_scope_items(items.iter());
        bindings
    }

    fn with_block_items(mut self, statements: &[Stmt]) -> Self {
        self.add_scope_items(statements.iter().filter_map(|statement| match statement {
            Stmt::Item(item) => Some(item),
            _ => None,
        }));
        self
    }

    fn add_scope_items<'item>(&mut self, items: impl Iterator<Item = &'item Item> + Clone) {
        for item in items.clone() {
            self.add_type_bindings(item);
        }
        let mut thread_binding = ThreadBinding::default();
        for item in items {
            thread_binding.add_item(item);
        }
        if thread_binding.present {
            self.thread_imported = thread_binding.standard
                && !thread_binding.other
                && (thread_binding.root_qualified || !self.std_shadowed);
        }
    }

    fn add_type_bindings(&mut self, item: &Item) {
        if let Some(name) = type_binding_name(item) {
            self.shadow(name.to_string().as_str());
            return;
        }
        if let Item::ExternCrate(item) = item {
            self.add_extern_crate(item);
        }
        if let Item::Use(item) = item {
            self.add_use_names(item);
        }
    }

    fn add_extern_crate(&mut self, item: &syn::ItemExternCrate) {
        let local = item
            .rename
            .as_ref()
            .map_or(&item.ident, |(_, rename)| rename)
            .to_string();
        if local == "std" && item.ident != "std" {
            self.std_shadowed = true;
        }
        if local == "tokio" && item.ident != "tokio" {
            self.tokio_shadowed = true;
        }
    }

    fn add_use_names(&mut self, item: &syn::ItemUse) {
        let mut names = Vec::new();
        collect_use_names(&item.tree, &mut names);
        for name in names {
            if name != "thread" {
                self.shadow(&name);
            }
        }
    }

    fn shadow(&mut self, name: &str) {
        match name {
            "std" => self.std_shadowed = true,
            "tokio" => self.tokio_shadowed = true,
            "thread" => self.thread_imported = false,
            _ => {}
        }
    }
}

#[derive(Default)]
struct ThreadBinding {
    present: bool,
    standard: bool,
    other: bool,
    root_qualified: bool,
}

impl ThreadBinding {
    fn add_item(&mut self, item: &Item) {
        if let Some(name) = type_binding_name(item) {
            self.add_named_item(name);
            return;
        }
        match item {
            Item::ExternCrate(item) => {
                let local = item
                    .rename
                    .as_ref()
                    .map_or(&item.ident, |(_, rename)| rename);
                self.add_named_item(local);
            }
            Item::Use(item) => {
                let mut path = Vec::new();
                self.add_use_tree(&item.tree, &mut path, item.leading_colon.is_some());
            }
            _ => {}
        }
    }

    fn add_named_item(&mut self, name: &syn::Ident) {
        if name == "thread" {
            self.present = true;
            self.other = true;
        }
    }

    fn add_use_tree(&mut self, tree: &UseTree, path: &mut Vec<String>, root_qualified: bool) {
        match tree {
            UseTree::Path(node) => {
                path.push(node.ident.to_string());
                self.add_use_tree(&node.tree, path, root_qualified);
                path.pop();
            }
            UseTree::Name(node) => {
                path.push(node.ident.to_string());
                self.add_use_leaf(path, node.ident == "thread", root_qualified);
                path.pop();
            }
            UseTree::Rename(node) => {
                path.push(node.ident.to_string());
                self.add_use_leaf(path, node.rename == "thread", root_qualified);
                path.pop();
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use_tree(tree, path, root_qualified);
                }
            }
            UseTree::Glob(_) => {
                self.present = true;
                self.other = true;
            }
        }
    }

    fn add_use_leaf(&mut self, path: &[String], binds_thread: bool, root_qualified: bool) {
        if !binds_thread {
            return;
        }
        self.present = true;
        if path.iter().map(String::as_str).eq(["std", "thread"]) {
            self.standard = true;
            self.root_qualified |= root_qualified;
        } else {
            self.other = true;
        }
    }
}

fn type_binding_name(item: &Item) -> Option<&syn::Ident> {
    match item {
        Item::Enum(item) => Some(&item.ident),
        Item::Mod(item) => Some(&item.ident),
        Item::Struct(item) => Some(&item.ident),
        Item::Trait(item) => Some(&item.ident),
        Item::TraitAlias(item) => Some(&item.ident),
        Item::Type(item) => Some(&item.ident),
        Item::Union(item) => Some(&item.ident),
        _ => None,
    }
}

fn collect_use_names(tree: &UseTree, names: &mut Vec<String>) {
    match tree {
        UseTree::Path(path) => collect_use_names(&path.tree, names),
        UseTree::Name(name) => names.push(name.ident.to_string()),
        UseTree::Rename(rename) => names.push(rename.rename.to_string()),
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_names(tree, names);
            }
        }
        UseTree::Glob(_) => {
            names.extend(["std", "tokio", "thread"].map(str::to_owned));
        }
    }
}

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

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let mut visitor = SelectionVisitor {
            source,
            kind: self.kind,
            bindings: Bindings::for_module(&file.items),
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
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
        self.with_generics(&item.sig.generics, |visitor| {
            visit::visit_item_fn(visitor, item)
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.with_generics(&item.generics, |visitor| {
            visit::visit_item_impl(visitor, item)
        });
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.with_generics(&item.generics, |visitor| {
            visit::visit_item_trait(visitor, item)
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.with_generics(&item.sig.generics, |visitor| {
            visit::visit_impl_item_fn(visitor, item)
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.with_generics(&item.sig.generics, |visitor| {
            visit::visit_trait_item_fn(visitor, item)
        });
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let Some((_, items)) = &item.content else {
            return;
        };
        self.with_bindings(Bindings::for_module(items), |visitor| {
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
    fn with_generics(&mut self, generics: &syn::Generics, visit: impl FnOnce(&mut Self)) {
        let mut bindings = self.bindings;
        for parameter in &generics.params {
            let GenericParam::Type(parameter) = parameter else {
                continue;
            };
            bindings.shadow(parameter.ident.to_string().as_str());
        }
        self.with_bindings(bindings, visit);
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
    names == ["tokio", "select"] && (path.leading_colon.is_some() || !bindings.tokio_shadowed)
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
