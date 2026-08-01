//! Public support for the `mutarust` command.

mod discovery;
mod execution;
mod mutator;

pub use discovery::{SourceError, find_rust_sources};
pub use execution::{
    DEFAULT_TEST_TIMEOUT, MutationResult, MutationRun, MutationState, RunError, run_mutation_tests,
    run_mutation_tests_with_timeout,
};
pub use mutator::{Mutation, Mutator, Registry, RegistryBuilder, RegistryError};

/// The package version for the installed command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
