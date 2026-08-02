use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use proc_macro2::Span;

use crate::control_flow::ControlFlowMutator;
use crate::expression::{
    ArithmeticNegate, BinaryOperatorMutator, BoolLiteralMutator, ConditionalNotMutator,
    NumberMutator, StringLiteralMutator,
};
use crate::return_value::ReturnValueMutator;
use crate::value::ValueMutator;

/// A source replacement produced by a mutator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mutation {
    range: Range<usize>,
    change: MutationChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MutationChange {
    replacement: String,
    requires_compile_validation: bool,
}

impl Mutation {
    /// Creates one source replacement.
    pub fn new(range: Range<usize>, replacement: impl Into<String>) -> Self {
        Self {
            range,
            change: MutationChange {
                replacement: replacement.into(),
                requires_compile_validation: false,
            },
        }
    }

    pub(crate) fn requiring_compile_validation(mut self) -> Self {
        self.change.requires_compile_validation = true;
        self
    }

    pub(crate) fn requires_compile_validation(&self) -> bool {
        self.change.requires_compile_validation
    }

    /// Returns a changed source string when this mutation has a valid range.
    pub fn apply(&self, source: &str) -> Option<String> {
        if self.range.start > self.range.end {
            return None;
        }
        let before = source.get(..self.range.start)?;
        let after = source.get(self.range.end..)?;
        Some(format!("{before}{}{after}", self.change.replacement))
    }

    /// Returns the replaced byte range and the replacement source text.
    pub fn identity(&self) -> (Range<usize>, &str) {
        (self.range.clone(), &self.change.replacement)
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
            .register(ControlFlowMutator::branch_case())
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::branch_else())
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::branch_if())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::arithmetic_base())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::arithmetic_bitwise())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::arithmetic_assign_invert())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::arithmetic_assignment())
            .expect("built-in mutator registration must be valid")
            .register(ArithmeticNegate)
            .expect("built-in mutator registration must be valid")
            .register(NumberMutator::incrementer())
            .expect("built-in mutator registration must be valid")
            .register(NumberMutator::decrementer())
            .expect("built-in mutator registration must be valid")
            .register(NumberMutator::float_negate())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::conditional_negated())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::expression_comparison())
            .expect("built-in mutator registration must be valid")
            .register(BinaryOperatorMutator::expression_logical())
            .expect("built-in mutator registration must be valid")
            .register(StringLiteralMutator)
            .expect("built-in mutator registration must be valid")
            .register(BoolLiteralMutator)
            .expect("built-in mutator registration must be valid")
            .register(ConditionalNotMutator)
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::loop_break())
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::loop_condition())
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::loop_range_break())
            .expect("built-in mutator registration must be valid")
            .register(ControlFlowMutator::statement_remove())
            .expect("built-in mutator registration must be valid")
            .register(ValueMutator::composite_field_clear())
            .expect("built-in mutator registration must be valid")
            .register(ValueMutator::context_nil())
            .expect("built-in mutator registration must be valid")
            .register(ValueMutator::remove_self_assign())
            .expect("built-in mutator registration must be valid")
            .register(ReturnValueMutator)
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

    /// Removes each mutator for which `keep` returns false.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.mutators.retain(|name, _| keep(name));
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

fn validate_mutator_name(name: &str) -> Result<(), RegistryError> {
    name.split('/')
        .all(valid_mutator_name_part)
        .then_some(())
        .ok_or_else(|| RegistryError::InvalidName(name.to_owned()))
}

fn valid_mutator_name_part(part: &str) -> bool {
    !part.is_empty()
        && part.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        })
}

pub(crate) fn span_range(source: &str, span: Span) -> Option<Range<usize>> {
    let range = span.byte_range();
    (range.start < range.end
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end))
    .then_some(range)
}
