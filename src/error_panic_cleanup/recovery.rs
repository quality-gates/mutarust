use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::error_panic_cleanup::crate_root_aliases;
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
        if crate_root_aliases(&file.items).1 {
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
        if catch_unwind_call(call)
            && let Some(range) = span_range(self.source, call.span())
            && let Some(original) = self.source.get(range.clone())
        {
            let replacement = propagate_panics(original);
            self.mutations
                .push(Mutation::new(range, replacement).requiring_compile_validation());
        }
        visit::visit_expr_call(self, call);
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
    function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(["std", "panic", "catch_unwind"].map(str::to_owned))
}

fn propagate_panics(call: &str) -> String {
    format!(
        "match {call} {{ ::core::result::Result::Ok(value) => ::core::result::Result::<_, ::std::boxed::Box<dyn ::core::any::Any + ::core::marker::Send>>::Ok(value), ::core::result::Result::Err(payload) => ::std::panic::resume_unwind(payload), }}"
    )
}
