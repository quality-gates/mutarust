# Coding standards

## Tests

- Strongly prefer integration tests and end-to-end tests over unit tests.
- Strongly prefer exercising real system behaviour over "the tests pass so it must work."
- Only mock third-party services we cannot control. Do not mock code we own.
- For this codebase, the default proof is: run the real CLI on a real Cargo crate (or controlled fixture workflow) and assert kill/escape/error behaviour, exit codes, and report fields — not hardcoded mutant counts.

## Comments and docs

- Code comments use ASD-STE100 Simplified Technical English.
- Ground terms in `CONTEXT.md` domain language when that file exists. Do not invent synonyms for glossary terms (mutation, mutant, killed, escaped, errored, not covered, skipped, baseline, blacklist, stable mutant ID).
- Do not write comments that only repeat what the code already makes clear.
- Do not put brittle references in README or comments (versions, line numbers, temporary paths, "as of today" claims) when those details are allowed to change.

## Common footguns

- Tautological tests (asserting the mock was called the way the test just configured it).
- Mocks of modules/services we own.
- "Green suite" treated as proof the product works for a user.
- Narrating comments and README drift magnets.
- Cheating complexity or quality gates with denser syntax, hidden branching, or indirection that does not reduce real complexity.
- Asserting exact mutant counts that churn when mutators or fixtures change.

## Rust

- Stay on the package edition and MSRV in `Cargo.toml` (`edition = "2024"`, `rust-version = "1.85"`). Do not bump casually.
- Prefer explicit `Result` at fallible boundaries. Do not `unwrap`/`expect` on input-dependent or user-project paths; reserve panics for internal bugs.
- Parse Rust with the existing `syn` / lexer stack. Do not add a second syntax stack.
- Keep production layout under `src/` modules; prefer integration tests under `tests/` over large in-module `#[cfg(test)]` suites when exercising the CLI or workspace behaviour.
- New code must be `cargo fmt`-clean and `cargo clippy`-clean with warnings denied as CI does (`-D warnings` on workspace targets).
- Match lockfile policy: this repo commits `Cargo.lock`. Use `--locked` where CI does.
- Mutarust must not modify the user's source tree as a lasting side effect. Temp copies and restore paths must leave the target clean.
- Production code must report no violations on messrust rulesets `codesize` and `design` (`messrust src text codesize,design --ignore-tests --strict`).
- Do not assert on hardcoded mutation counts. Assert behaviour: summary presence, score bounds, report field consistency, exit codes, and stable IDs where relevant.
- Prefer workspace commands from CI: `cargo test --workspace --all-targets --all-features --locked`.
