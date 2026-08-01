# Mutarust Context

## Purpose

Mutarust is a mutation test tool for Rust. It changes production source in a
small way. It then runs tests to find if the tests detect the change.

## Terms

- Mutation: One source change made for a test run.
- Mutant: The source produced by one mutation.
- Killed: A test detects a mutant.
- Escaped: Tests do not detect a mutant.
- Errored: Mutarust cannot complete a mutation run.
- Not covered: No test covers the changed source location.
- Skipped: Mutarust does not run a mutation.
- Mutation score: The proportion of successful mutation results.
- Covered-code mutation score: The mutation score for covered source only.
- Mutator: A component that produces mutations.
- Baseline: A set of accepted escaped mutant IDs.
- Blacklist: A set of accepted mutation checksums.
- Stable mutant ID: An ID from a source name, mutator name, and mutation diff.

## Invariants

- Mutarust must not change the user source tree.
- A command result must use the terms in this document.
- Production code must have no strict messrust codesize or design finding.
