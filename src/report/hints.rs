const MUTATOR_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "arithmetic/assign_invert",
        "Inverts a compound assignment operator (e.g. += becomes -=)",
    ),
    (
        "arithmetic/assignment",
        "Swaps an assignment operator for a different one (e.g. = becomes +=)",
    ),
    (
        "arithmetic/base",
        "Swaps an arithmetic operator (+, -, *, /) for a different one",
    ),
    (
        "arithmetic/bitwise",
        "Swaps a bitwise operator (&, |, ^, <<, >>) for a different one",
    ),
    (
        "arithmetic/negate",
        "Removes a unary minus from a numeric value",
    ),
    ("branch/case", "Replaces a match arm body with a no-op"),
    ("branch/else", "Removes an else-block body"),
    (
        "branch/if",
        "Removes an if-block body so the condition becomes a no-op",
    ),
    (
        "composite/field-clear",
        "Clears a struct field so it uses a default value",
    ),
    (
        "concurrency/goroutine-remove",
        "Runs a discarded thread or Tokio task immediately, removing concurrency",
    ),
    (
        "conditional/bool-literal",
        "Swaps a hardcoded boolean literal (true↔false) in an assignment or function argument",
    ),
    (
        "conditional/negated",
        "Negates a boolean or comparison condition (e.g. == becomes !=, < becomes >=)",
    ),
    (
        "conditional/not",
        "Removes the logical-NOT operator from a negated condition (!x becomes x)",
    ),
    (
        "expression/comparison",
        "Replaces a comparison operator with a boundary variant (e.g. < becomes <=)",
    ),
    (
        "expression/context-nil",
        "Replaces a Some(...) argument with None, bypassing an optional value",
    ),
    (
        "expression/errorf-wrap",
        "Removes a standard error source link so the returned error no longer preserves its cause",
    ),
    (
        "expression/logical",
        "Swaps a logical operator (&& becomes ||, or vice versa)",
    ),
    (
        "expression/recover-clear",
        "Makes a supported panic catch resume the panic instead of recovering",
    ),
    (
        "expression/string-literal",
        "Replaces a non-empty string literal in an == or != comparison with an empty string",
    ),
    (
        "loop/break",
        "Swaps break and continue, changing loop control flow",
    ),
    ("loop/condition", "Changes the loop's termination condition"),
    (
        "loop/range_break",
        "Inserts a break at the start of a for-loop body",
    ),
    ("numbers/decrementer", "Decrements a numeric literal by 1"),
    (
        "numbers/float-negate",
        "Replaces a nonzero floating-point literal with 0.0",
    ),
    ("numbers/incrementer", "Increments a numeric literal by 1"),
    (
        "select/case-remove",
        "Removes a branch from a Tokio selection, reducing channel handling paths",
    ),
    (
        "select/default-remove",
        "Removes the else branch from a Tokio selection",
    ),
    (
        "statement/defer-remove",
        "Removes an explicit drop so cleanup moves to the end of its scope",
    ),
    (
        "statement/remove",
        "Removes a statement entirely, dropping its side effect or return value",
    ),
    (
        "statement/remove-self-assign",
        "Removes a self-assignment statement (e.g. x = x)",
    ),
    (
        "statement/return",
        "Replaces a return value with its default value",
    ),
];

const KILL_HINTS: &[(&str, &str)] = &[
    (
        "arithmetic/assign_invert",
        "Write a test that asserts the accumulated result after the operation — inverting += to -= produces a different total",
    ),
    (
        "arithmetic/assignment",
        "Write a test that asserts the exact value after the assignment — different operators produce different results",
    ),
    (
        "arithmetic/base",
        "Write a test with specific numeric inputs and assert the exact output — boundary values expose operator swaps best",
    ),
    (
        "arithmetic/bitwise",
        "Write a test with inputs where different bitwise operators produce distinct results and assert the exact output",
    ),
    (
        "arithmetic/negate",
        "Write a test that asserts the sign or magnitude of the result — negation flips positive to negative",
    ),
    (
        "branch/case",
        "Write a test that enters this match arm and asserts the output or side effect it produces",
    ),
    (
        "branch/else",
        "Write a test where the else path is taken and assert its expected result",
    ),
    (
        "branch/if",
        "Write a test that enters this branch and asserts the output or side effect it produces",
    ),
    (
        "composite/field-clear",
        "Think about what a caller observes if this field were left unset. Write a test that drives the code via its public API and asserts the behaviour that depends on this field's value",
    ),
    (
        "concurrency/goroutine-remove",
        "Write a test that asserts concurrent behaviour — for example a channel receive, a timing constraint, or a race-detector hit",
    ),
    (
        "conditional/bool-literal",
        "Think about what a caller would observe if this flag were wrong. Write a test that drives the code via its public API with both values and asserts the different outcomes that a correct caller should see",
    ),
    (
        "conditional/negated",
        "Write tests that exercise both the true and false paths of this condition and assert different outcomes for each",
    ),
    (
        "conditional/not",
        "Think about what changes when the condition is satisfied vs not. Write tests that drive the code through both paths via its public API and assert the distinct outcomes a caller would see",
    ),
    (
        "expression/comparison",
        "Write tests at the exact boundary value — one that satisfies the condition and one that doesn't — and assert different outcomes",
    ),
    (
        "expression/context-nil",
        "Write a test that passes None where Some was required and asserts the function handles the missing value",
    ),
    (
        "expression/errorf-wrap",
        "Write a test that asserts the returned error still exposes its cause through the public API",
    ),
    (
        "expression/logical",
        "Write tests where only one operand is true/false so && and || produce different outcomes",
    ),
    (
        "expression/recover-clear",
        "Write a test that triggers the panic and asserts the recovery behaviour a caller would observe",
    ),
    (
        "expression/string-literal",
        "Think about what a caller expects when this string matches vs doesn't. Write a test that supplies a value that should match, and one that should not, and asserts the different outcomes a caller would see through the public API",
    ),
    (
        "loop/break",
        "Write a test that asserts the loop terminates at the right iteration",
    ),
    (
        "loop/condition",
        "Write a test with a known input and assert the exact number of loop iterations or the final state",
    ),
    (
        "loop/range_break",
        "Write a test that asserts the loop stops at the correct element",
    ),
    (
        "numbers/decrementer",
        "Write a test that asserts the exact numeric value",
    ),
    (
        "numbers/float-negate",
        "Write a test that asserts the sign or exact value of the float result",
    ),
    (
        "numbers/incrementer",
        "Write a test that asserts the exact numeric value — off-by-one mutations are killed by precise equality assertions",
    ),
    (
        "select/case-remove",
        "Write a test that exercises the removed selection branch and asserts the expected receive or resulting action",
    ),
    (
        "select/default-remove",
        "Write a test where no selection branch is ready and assert the else-path behaviour",
    ),
    (
        "statement/defer-remove",
        "Think about what a caller would observe if cleanup happened too early. Write a test that checks the state visible to a caller after the function returns",
    ),
    (
        "statement/remove",
        "Write a test that asserts the side effect or state change this statement produces",
    ),
    (
        "statement/remove-self-assign",
        "Write a test that asserts the value is unchanged after a self-assignment — any mutation would alter it",
    ),
    (
        "statement/return",
        "Write a test that asserts the exact return value — default-value substitutions are caught by equality assertions",
    ),
];

/// Returns the plain-language description for a mutator name.
pub(super) fn mutator_description(mutator: &str) -> Option<&'static str> {
    lookup(MUTATOR_DESCRIPTIONS, mutator)
}

/// Returns the kill-test hint for a mutator name.
pub(super) fn kill_hint(mutator: &str) -> Option<&'static str> {
    lookup(KILL_HINTS, mutator)
}

fn lookup(
    entries: &'static [(&'static str, &'static str)],
    mutator: &str,
) -> Option<&'static str> {
    entries
        .binary_search_by_key(&mutator, |entry| entry.0)
        .ok()
        .map(|index| entries[index].1)
}
