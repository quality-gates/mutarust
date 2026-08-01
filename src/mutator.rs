use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use proc_macro2::{Span, TokenStream, TokenTree};

/// A source replacement produced by a mutator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mutation {
    range: Range<usize>,
    replacement: String,
}

impl Mutation {
    /// Creates one source replacement.
    pub fn new(range: Range<usize>, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    /// Returns a changed source string when this mutation has a valid range.
    pub fn apply(&self, source: &str) -> Option<String> {
        if self.range.start > self.range.end {
            return None;
        }
        let before = source.get(..self.range.start)?;
        let after = source.get(self.range.end..)?;
        Some(format!("{before}{}{after}", self.replacement))
    }

    /// Returns the replaced byte range and the replacement source text.
    pub fn identity(&self) -> (Range<usize>, &str) {
        (self.range.clone(), &self.replacement)
    }
}

/// Produces source mutations for one named mutation operator.
pub trait Mutator: Send + Sync {
    /// Returns the stable name of this mutator.
    fn name(&self) -> &str;

    /// Returns mutations for the supplied Rust source text.
    fn mutations(&self, source: &str) -> Vec<Mutation>;
}

/// Builds a registry of named mutators.
pub struct RegistryBuilder {
    mutators: BTreeMap<String, Box<dyn Mutator>>,
}

impl RegistryBuilder {
    /// Creates an empty mutator registry builder.
    pub fn new() -> Self {
        Self {
            mutators: BTreeMap::new(),
        }
    }

    /// Creates a builder with all mutators supplied by Mutarust.
    pub fn with_builtins() -> Self {
        Self::new()
            .register(BoolLiteral)
            .expect("built-in mutator registration must be valid")
    }

    /// Adds a mutator to this builder.
    pub fn register(mut self, mutator: impl Mutator + 'static) -> Result<Self, RegistryError> {
        let name = mutator.name();
        validate_mutator_name(name)?;
        if self.mutators.contains_key(name) {
            return Err(RegistryError::Duplicate(name.to_owned()));
        }
        self.mutators.insert(name.to_owned(), Box::new(mutator));
        Ok(self)
    }

    /// Returns the registry built from the added mutators.
    pub fn build(self) -> Registry {
        Registry {
            mutators: self.mutators,
        }
    }
}

impl Default for RegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A sorted registry of named mutators.
pub struct Registry {
    mutators: BTreeMap<String, Box<dyn Mutator>>,
}

impl Registry {
    /// Returns the Rust mutators that Mutarust supplies.
    pub fn builtins() -> Self {
        RegistryBuilder::with_builtins().build()
    }

    /// Returns one mutator by its stable name.
    pub fn get(&self, name: &str) -> Option<&dyn Mutator> {
        self.mutators.get(name).map(Box::as_ref)
    }

    /// Returns all mutator names in sorted order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.mutators.keys().map(String::as_str)
    }
}

/// Describes an invalid mutator registration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryError {
    /// The name does not use lower-case slash-separated words.
    InvalidName(String),
    /// A mutator has an existing name.
    Duplicate(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(formatter, "invalid mutator name: {name}"),
            Self::Duplicate(name) => write!(formatter, "duplicate mutator name: {name}"),
        }
    }
}

impl std::error::Error for RegistryError {}

struct BoolLiteral;

impl Mutator for BoolLiteral {
    fn name(&self) -> &str {
        "conditional/bool-literal"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(tokens) = source.parse::<TokenStream>() else {
            return Vec::new();
        };
        boolean_mutations(source, tokens)
    }
}

fn boolean_mutations(source: &str, tokens: TokenStream) -> Vec<Mutation> {
    let mut mutations = Vec::new();
    collect_boolean_mutations(source, tokens, &mut mutations);
    mutations
}

fn collect_boolean_mutations(source: &str, tokens: TokenStream, mutations: &mut Vec<Mutation>) {
    for token in tokens {
        match token {
            TokenTree::Ident(identifier) => add_boolean_mutation(source, identifier, mutations),
            TokenTree::Group(group) => {
                collect_boolean_mutations(source, group.stream(), mutations);
            }
            _ => {}
        }
    }
}

fn add_boolean_mutation(
    source: &str,
    identifier: proc_macro2::Ident,
    mutations: &mut Vec<Mutation>,
) {
    let replacement = match identifier.to_string().as_str() {
        "true" => "false",
        "false" => "true",
        _ => return,
    };
    if let Some(range) = span_range(source, identifier.span()) {
        mutations.push(Mutation::new(range, replacement));
    }
}

fn validate_mutator_name(name: &str) -> Result<(), RegistryError> {
    name.split('/')
        .all(valid_mutator_name_part)
        .then_some(())
        .ok_or_else(|| RegistryError::InvalidName(name.to_owned()))
}

fn valid_mutator_name_part(part: &str) -> bool {
    !part.is_empty()
        && part.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn span_range(source: &str, span: Span) -> Option<Range<usize>> {
    let range = span.byte_range();
    (range.start < range.end
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end))
    .then_some(range)
}
