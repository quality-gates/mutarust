# Changelog

This file records important user changes.

## Unreleased

### Added

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
