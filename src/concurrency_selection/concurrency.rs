use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, Stmt};

use super::bindings::Bindings;
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct ConcurrencyMutator;

impl Mutator for ConcurrencyMutator {
    fn name(&self) -> &str {
        "concurrency/goroutine-remove"
    }

    fn mutations_from_parsed(&self, source: &str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = ConcurrencyVisitor {
            source,
            async_context: false,
            bindings: Bindings::for_crate(&file.items),
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
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
        let bindings = self.bindings.for_nested_module(items);
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
        self.with_bindings(self.bindings.with_generics(generics), visit);
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
    (names == ["std", "thread", "spawn"] && bindings.standard_path_available(root_qualified))
        || (!root_qualified && bindings.standard_thread_imported() && names == ["thread", "spawn"])
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
    supported && bindings.tokio_path_available(function.path.leading_colon.is_some())
}
