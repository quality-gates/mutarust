//! Public support for the `mutarust` command.

macro_rules! skip_non_expression_syntax {
    () => {
        fn visit_pat(&mut self, _pattern: &'ast syn::Pat) {}

        fn visit_type(&mut self, _kind: &'ast syn::Type) {}

        fn visit_generic_argument(&mut self, argument: &'ast syn::GenericArgument) {
            if !matches!(
                argument,
                syn::GenericArgument::Const(_) | syn::GenericArgument::AssocConst(_)
            ) {
                syn::visit::visit_generic_argument(self, argument);
            }
        }
    };
}

mod baseline;
mod blacklist;
mod concurrency_selection;
mod configuration;
mod control_flow;
mod coverage;
mod discovery;
mod evidence;
mod execution;
mod expression;
mod filter;
mod git;
mod mutator;
mod return_value;
mod value;

pub use baseline::Baseline;
pub use configuration::{CommandSettings, Configuration, ConfigurationError};
pub use discovery::{SourceError, find_rust_sources};
pub use execution::{
    CoverageControls, DEFAULT_TEST_TIMEOUT, ExecutionControls, GitDiffControls, MutationResult,
    MutationRun, MutationState, MutatorSummary, RunError, TestExecution, WorkerLimit,
    run_mutation_tests, run_mutation_tests_with_controls, run_mutation_tests_with_test_execution,
    run_mutation_tests_with_timeout, run_mutation_tests_with_timeout_for_mutant,
    run_mutation_tests_with_timeout_for_mutant_and_filters,
};
pub use filter::SourceFilters;
pub use mutator::{Mutation, Mutator, Registry, RegistryBuilder, RegistryError};

/// The package version for the installed command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
