//! Public support for the `mutarust` command.

mod configuration;
mod discovery;
mod evidence;
mod execution;
mod filter;
mod mutator;

pub use configuration::{CommandSettings, Configuration, ConfigurationError};
pub use discovery::{SourceError, find_rust_sources};
pub use execution::{
    DEFAULT_TEST_TIMEOUT, MutationResult, MutationRun, MutationState, MutatorSummary, RunError,
    TestExecution, run_mutation_tests, run_mutation_tests_with_test_execution,
    run_mutation_tests_with_timeout, run_mutation_tests_with_timeout_for_mutant,
    run_mutation_tests_with_timeout_for_mutant_and_filters,
};
pub use filter::SourceFilters;
pub use mutator::{Mutation, Mutator, Registry, RegistryBuilder, RegistryError};

/// The package version for the installed command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
