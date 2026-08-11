# Changelog

This file records important user changes.

## Unreleased

### Changed

- Capped Docker bench (`--cpus=1 --memory=512m --workers 1`, 21-mutant value
  fixture): wall time about 5.10s versus the 5.46s baseline. Remaining time is
  almost all rustc/Cargo work on each mutant; see parent PRD #68 for the
  residual list toward the 2.73s bar.

- After a clean Cargo test passes, Mutarust keeps that workspace copy and its
  target directory for the first mutation worker. The first mutant no longer
  starts from a cold Cargo build for that layout.
- Parallel mutation workers each receive a private copy of the clean suite
  build. Workers never share a writable Cargo target directory.
- The default Cargo path runs one `cargo test` per mutant. Compile failures
  are still skipped and test failures are still killed from that single
  command.
- Mutation workers no longer restore and re-read the same source file between
  mutants. The original text stays in memory and only the new mutant bytes are
  written.

## 0.1.2 — 2026-08-05

A compatible fix release. The public library interface does not change.

### Fixed

- Mutation workers no longer oversubscribe the host CPU. Each Cargo build gets a
  `-j` limit so worker count times Cargo jobs stays near the logical CPU count.
- Each mutation worker reuses one workspace copy and its Cargo target directory
  across mutants. Later mutants rebuild only the changed crate instead of a full
  cold dependency build for every mutant.

## 0.1.1 — 2026-08-03

A compatible release. The public library interface does not change.

### Added

- License and version badges in `README.md`.
- A test that the packaged `LICENSE` file contains the MIT terms.

### Changed

- Dependency updates: `md5` to 0.8, `syn` to 3, and `toml` to 1.
- The `LICENSE` file now uses the standard MIT header text.

### Fixed

- Match-arm guards are again read correctly. In `syn` 3 a guard is part of the
  pattern. The cleanup mutator missed guard expressions, and the pattern
  recorder wrongly recorded guard bindings as arm bindings.
- Manifest and Cargo configuration files parse again with `toml` 1.

## 0.1.0 — 2026-08-03

First crates.io release.

### Added

- Mutation testing command and public mutator library for Rust.
- Release CI, user documentation site, and optional `githooks/` that mirror
  fast format, Clippy, messrust, and changed-line self-mutation checks without
  automatic activation.
- User guides for install, quick start, commands, configuration, mutators,
  reports, custom mutators, CI, release, and the domain glossary.
- Self-mutation CI workflow that builds the release command, mutates changed
  lines on pull requests, and mutates the approved production scope on main
  with 75 percent total and 80 percent covered-code score gates.
- Workspace copy support for symbolic links required by self-mutation.
- Default exclusion of `#[cfg(test)]` modules from mutation.
- Mutago v2.7.7 parity table and the final two expression mutators
  (`expression/remove`, `expression/error-guard`).
- Working `skip_without_test` and `skip_with_cfg` configuration policies.
- `--html-output` and `--ignore-msi-with-no-mutations` command options.
- Cargo package foundation.
- Installed command smoke test.
- Rust production-source discovery and `--list-files`.
- Public mutator registry and `--list-mutators`.
- `--print-ast` syntax-tree mode and Bash completion for documented options.
- Full `report.json` and compact `mutarust-summary.json` reports.
- Self-contained `mutarust-report.html` and agent-ready `mutarust-agentic.json` reports.
- GitHub Actions warnings and GitLab Code Quality report for escaped mutants.

### Fixed

- `--test-flags` now apply to normal LLVM coverage collection.
