use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::error_panic_cleanup::{path_names_are, standard_crates};
use crate::mutator::span_range;
use crate::{Mutation, Mutator};

pub(crate) struct RecoveryMutator;

impl Mutator for RecoveryMutator {
    fn name(&self) -> &str {
        "expression/recover-clear"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        if !standard_crates(&file.items).std_available {
            return Vec::new();
        }
        let mut visitor = RecoveryVisitor {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

struct RecoveryVisitor<'source> {
    source: &'source str,
    mutations: Vec<Mutation>,
}

impl<'ast> Visit<'ast> for RecoveryVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        self.record_catch_unwind(call);
        visit::visit_expr_call(self, call);
    }
}

impl RecoveryVisitor<'_> {
    fn record_catch_unwind(&mut self, call: &syn::ExprCall) -> Option<()> {
        if !catch_unwind_call(call) {
            return None;
        }
        let range = span_range(self.source, call.span())?;
        let original = self.source.get(range.clone())?;
        let replacement = propagate_panics(original);
        self.mutations
            .push(Mutation::new(range, replacement).requiring_compile_validation());
        Some(())
    }
}

fn catch_unwind_call(call: &syn::ExprCall) -> bool {
    if call.args.len() != 1 {
        return false;
    }
    let syn::Expr::Path(function) = call.func.as_ref() else {
        return false;
    };
    if function.qself.is_some() || function.path.leading_colon.is_none() {
        return false;
    }
    path_names_are(&function.path, &["std", "panic", "catch_unwind"])
}

fn propagate_panics(call: &str) -> String {
    format!(
        "match {call} {{ ::core::result::Result::Ok(value) => ::core::result::Result::<_, ::std::boxed::Box<dyn ::core::any::Any + ::core::marker::Send>>::Ok(value), ::core::result::Result::Err(payload) => ::std::panic::resume_unwind(payload), }}"
    )
}
