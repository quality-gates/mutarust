//! Public support for the `mutarust` command.

mod discovery;
mod mutator;

pub use discovery::{SourceError, find_rust_sources};
pub use mutator::{Mutation, Mutator, Registry, RegistryBuilder, RegistryError};

/// The package version for the installed command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
