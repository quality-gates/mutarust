//! Public support for the `mutarust` command.

mod discovery;

pub use discovery::{SourceError, find_rust_sources};

/// The package version for the installed command.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
