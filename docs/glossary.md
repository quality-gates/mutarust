# Domain Glossary

These terms are the project language for mutation testing.

## Mutation

One source change made for a test run.

## Mutant

The source produced by one mutation.

## Killed

A test detects a mutant.

## Escaped

Tests do not detect a mutant.

## Errored

Mutarust cannot complete a mutation run for the mutant.

## Not covered

No test covers the changed source location.

## Skipped

Mutarust does not run a mutation.

## Mutation score

The proportion of successful mutation results. Mutarust prints this as a
percentage. JSON reports store the same value as a ratio from zero to one.

## Covered-code mutation score

The mutation score for covered source only. Mutants with the `not covered`
state are excluded from this score.

## Mutator

A component that produces mutations.

## Baseline

A set of accepted escaped mutant IDs.

## Blacklist

A set of accepted mutation checksums.

## Stable mutant ID

An ID from a source name, mutator name, and mutation diff.

## Source candidate

A Rust production source file selected as a possible input.
