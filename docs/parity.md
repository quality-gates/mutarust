# Mutago v2.7.7 Parity Table

Reference commit: `5e268a05d68c37d3b89a56447b4e80f791f824c2` (mutago v2.7.7).

Approved test seam: the installed `mutarust` command and the published library
contract. The acceptance suite in `tests/install_smoke.rs` runs against the
Cargo package artifact.

## Command options

| Mutago option | Mutarust option | Status | Notes |
| --- | --- | --- | --- |
| `--help` / `-h` | `--help` / `-h` | Same | |
| `--version` / `-V` | `--version` / `-V` | Same | |
| `--config` | `--config` | Same | |
| `--list-files` | `--list-files` | Same | |
| `--print-ast` | `--print-ast` | Same | |
| `--list-mutators` | `--list-mutators` | Same | |
| `--exec` | `--exec` | Adapted | Cargo workspace copy; see `docs/cli.md` |
| `--exec-timeout` | `--exec-timeout` | Same | Default 60s in Mutarust; Mutago default 10s |
| `--timeout` | `--timeout` | Same | Alias for `--exec-timeout` |
| `--timeout-coefficient` | `--timeout-coefficient` | Same | |
| `--test-flags` | `--test-flags` | Adapted | Cargo test flags |
| `--test-recursive` | `--test-recursive` | Adapted | Passes `--workspace` to Cargo |
| `--workers` | `--workers` | Same | |
| `--dry-run` | `--dry-run` | Same | |
| `--no-exec` | `--no-exec` | Same | |
| `--do-not-remove-tmp-folder` | `--do-not-remove-tmp-folder` | Same | |
| `--match` | `--match` | Same | |
| `--verbose` | `--verbose` | Same | |
| `--debug` | `--debug` | Same | Never prints environment secrets |
| `--silent` | `--silent` | Same | Also available as YAML `silent_mode` |
| `--no-silent` | `--no-silent` | Mutarust addition | Overrides `silent_mode: true` |
| `--quiet` | `--quiet` | Same | |
| `--output-statuses` | `--output-statuses` | Same | |
| `--no-diffs` | `--no-diffs` | Same | |
| `--logger-summary-json` | `--logger-summary-json` | Renamed file | Writes `mutarust-summary.json` |
| `--logger-agentic-json` | `--logger-agentic-json` | Renamed file | Writes `mutarust-agentic.json` |
| `--logger-github` | `--logger-github` | Same | |
| `--logger-gitlab` | `--logger-gitlab` | Renamed file | Writes `mutarust-gitlab.json` |
| `--html-output` | `--html-output` | Same | Also YAML `html_output` |
| `--blacklist` | `--blacklist` | Same | |
| `--baseline` | `--baseline` | Same | Default file `mutarust-baseline.json` |
| `--update-baseline` | `--update-baseline` | Same | |
| `--fail-on-escaped` | `--fail-on-escaped` | Same | |
| `--run-mutant-id` | `--run-mutant-id` | Same | |
| `--min-msi` | `--min-msi` | Same | Integer 0–100 |
| `--min-covered-msi` | `--min-covered-msi` | Same | Integer 0–100 |
| `--ignore-msi-with-no-mutations` | `--ignore-msi-with-no-mutations` | Same | |
| `--enable` | `--enable` | Mutarust addition | Mutago uses YAML `enable_mutators` |
| `--disable` | `--disable` | Same | |
| `--coverage` | `--coverage` | Adapted | Uses `cargo-llvm-cov` |
| `--per-test` | `--per-test` | Adapted | Uses LLVM per-test coverage |
| `--git-diff-lines` | `--git-diff-lines` | Same | |
| `--git-diff-base` | `--git-diff-base` | Same | |
| `--noop` | (always on) | Adapted | Mutarust always runs the clean suite before mutation |

## Configuration fields

| Mutago field | Mutarust field | Status | Notes |
| --- | --- | --- | --- |
| `skip_without_test` | `skip_without_test` | Adapted | Skips files with no `#[cfg(test)]` item |
| `skip_with_build_tags` | `skip_with_cfg` | Renamed | Skips mutants in non-test `#[cfg(...)]` items |
| `json_output` | `json_output` | Same | Writes `report.json` |
| `html_output` | `html_output` | Renamed file | Writes `mutarust-report.html` |
| `silent_mode` | `silent_mode` | Same | |
| `min_msi` | `min_msi` | Same | |
| `min_covered_msi` | `min_covered_msi` | Same | |
| `exclude_dirs` | `exclude_dirs` | Same | |
| `disable_mutators` | `disable_mutators` | Same | |
| `enable_mutators` | `enable_mutators` | Same | |
| `ignore_source_lines` | `ignore_source_lines` | Same | |

Boolean fields default to `false` when omitted. Mutago’s dist file shows
`true` for the skip fields as a starting example. Mutarust does not load a
configuration file unless `--config` is set.

## Reports and annotations

| Mutago output | Mutarust output | Status |
| --- | --- | --- |
| `report.json` | `report.json` | Same field purpose |
| `mutago-summary.json` | `mutarust-summary.json` | Renamed file |
| `mutago-agentic.json` | `mutarust-agentic.json` | Renamed file |
| `mutago-report.html` | `mutarust-report.html` | Renamed file |
| `mutago-gitlab.json` | `mutarust-gitlab.json` | Renamed file |
| GitHub `::warning` | GitHub `::warning` | Same |
| `// mutator-disable-func` | `// mutator-disable-func` | Same purpose |
| `// mutator-disable-next-line` | `// mutator-disable-next-line` | Same purpose |
| `// mutator-disable-regexp` | `// mutator-disable-regexp` | Same purpose |

## CI controls

| Mutago CI control | Mutarust status | Notes |
| --- | --- | --- |
| `--fail-on-escaped` | Same | Exit 4 for new escapes |
| `--min-msi` / `--min-covered-msi` | Same | Exit 4 when below gate |
| `--ignore-msi-with-no-mutations` | Same | Pass gates when no mutant exists |
| `--logger-github` / `--logger-gitlab` | Same purpose | Renamed report files |
| `--git-diff-lines` on pull requests | Same | Documented in `docs/cli.md` |
| Self-mutation workflow | Same purpose | `.github/workflows/mutation.yml`; release binary, PR changed lines, main full approved scope, 75/80 gates |
| Release CI / docs / hooks | Same purpose | Issue #27; quality, messrust, security, package, docs site, optional `githooks/` |
| crates.io publish check | Deferred | Issue #28 |

Self-mutation CI is in `.github/workflows/mutation.yml`. Release CI, user
guides, and optional hooks are in issue #27. crates.io publication remains in
issue #28. This table maps the public command CI controls required for the
release candidate.

## Result states and exit values

| Mutago | Mutarust | Status |
| --- | --- | --- |
| KILLED | Killed | Same purpose |
| ESCAPED | Escaped | Same purpose |
| ERRORED | Errored | Same purpose |
| NOT COVERED | Not covered | Same purpose |
| SKIPPED | Skipped | Same purpose |
| Exit 0 | Exit 0 | Success / gates pass |
| Exit 1 | Exit 1 | Generic failure |
| Exit 2 | Exit 2 | Bash completion |
| Exit 3 | Exit 3 | Source / config / annotation errors |
| Exit 4 | Exit 4 | Quality-gate failure |

## Built-in mutators (33)

| Mutago name | Mutarust name | Status | Fixture |
| --- | --- | --- | --- |
| `arithmetic/assign_invert` | same | Direct | `tests/fixtures/expression` |
| `arithmetic/assignment` | same | Direct | `tests/fixtures/expression` |
| `arithmetic/base` | same | Direct | `tests/fixtures/expression` |
| `arithmetic/bitwise` | same | Direct | `tests/fixtures/expression` |
| `arithmetic/negate` | same | Direct | `tests/fixtures/expression` |
| `branch/case` | same | Adapted | `tests/fixtures/control-flow` |
| `branch/else` | same | Direct | `tests/fixtures/control-flow` |
| `branch/if` | same | Direct | `tests/fixtures/control-flow` |
| `composite/field-clear` | same | Adapted | `tests/fixtures/value` |
| `concurrency/goroutine-remove` | same | Adapted | `tests/fixtures/concurrency-selection` |
| `conditional/bool-literal` | same | Direct | `tests/fixtures/expression` |
| `conditional/negated` | same | Direct | `tests/fixtures/expression` |
| `conditional/not` | same | Direct | `tests/fixtures/expression` |
| `expression/comparison` | same | Direct | `tests/fixtures/expression` |
| `expression/context-nil` | same | Adapted | `tests/fixtures/value` |
| `expression/error-guard` | same | Adapted | `tests/fixtures/expression` |
| `expression/errorf-wrap` | same | Adapted | `tests/fixtures/error-panic-cleanup` |
| `expression/logical` | same | Direct | `tests/fixtures/expression` |
| `expression/recover-clear` | same | Adapted | `tests/fixtures/error-panic-cleanup` |
| `expression/remove` | same | Direct | `tests/fixtures/expression` |
| `expression/string-literal` | same | Direct | `tests/fixtures/expression` |
| `loop/break` | same | Direct | `tests/fixtures/control-flow` |
| `loop/condition` | same | Direct | `tests/fixtures/control-flow` |
| `loop/range_break` | same | Direct | `tests/fixtures/control-flow` |
| `numbers/decrementer` | same | Direct | `tests/fixtures/expression` |
| `numbers/float-negate` | same | Direct | `tests/fixtures/expression` |
| `numbers/incrementer` | same | Direct | `tests/fixtures/expression` |
| `select/case-remove` | same | Adapted | `tests/fixtures/concurrency-selection` |
| `select/default-remove` | same | Adapted | `tests/fixtures/concurrency-selection` |
| `statement/defer-remove` | same | Adapted | `tests/fixtures/error-panic-cleanup` |
| `statement/remove` | same | Adapted | `tests/fixtures/control-flow` |
| `statement/remove-self-assign` | same | Direct | `tests/fixtures/value` |
| `statement/return` | same | Adapted | `tests/fixtures/value` |

`expression/error-guard` collapses `is_err()` / `is_none()` to `false` and
`is_ok()` / `is_some()` to `true` in an `if` condition. This keeps the Mutago
purpose of collapsing error-presence guards.

Go operators with no Rust form (`&^`, `&^=`, unary `+`) have no Mutarust
replacement. See `docs/mutators.md`.

## Resolved conflicts

| Topic | Mutago docs | Mutago source / tests | Mutarust choice |
| --- | --- | --- | --- |
| Select mutators | Prose says empty a clause body | Source and tests remove the full clause | Follow source and tests; remove the full clause |
| `skip_without_test` default in dist | Dist file uses `true` | No config loaded → Go zero value `false` | Default `false`; document Rust `#[cfg(test)]` rule |
| `skip_with_build_tags` | Docs say “skip test files” | Source skips the production file when the paired test has build tags | Rename to `skip_with_cfg`; skip mutants in non-test `#[cfg]` items |
| Report file names | `mutago-*.json` / `.html` | Same | Use `mutarust-*` names |
| Clean-suite `--noop` | Optional flag | Optional | Always run the clean suite before mutation |
