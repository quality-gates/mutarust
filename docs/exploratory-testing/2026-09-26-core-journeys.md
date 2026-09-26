# Exploratory testing: core journeys (2026-09-26)

This report gives the results of one exploratory testing pass. The pass used
the `mutarust` command, which is the supported interface, on small scratch
Cargo packages. The pass found two confirmed bugs:

- [#198](https://github.com/quality-gates/mutarust/issues/198): Users cannot
  get a blacklist checksum from any Mutarust output.
- [#199](https://github.com/quality-gates/mutarust/issues/199): CI report paths
  are relative to the Cargo workspace, not the Git repository.

## Set-up

| Item | Value |
| --- | --- |
| Build | mutarust 0.1.9, commit `406beab`. Linux release binary (`cargo build --release --locked`) in `rust:1.85-slim`. See [et.Dockerfile](2026-09-26-core-journeys/et.Dockerfile). |
| Coverage build | Same image, with `llvm-tools-preview` and `cargo-llvm-cov` 0.6.16. See [cov.Dockerfile](2026-09-26-core-journeys/cov.Dockerfile). |
| Re-check | The two bugs were also replayed on `main` at `e599200` (after #197). See [replay-e599200.txt](2026-09-26-core-journeys/replay-e599200.txt). |
| Limits | `--cpus=1 --memory=512m --network=none` for each run. The coverage runs used `--memory=1g`. See [run.sh](2026-09-26-core-journeys/run.sh). |
| Starting state | A new Git repository with the `pricing` package: four functions and three weak unit tests. A two-member Cargo workspace (`core`, `app`) with tests in `tests/`. A `spin` package with a loop that does not stop when mutated. |
| Configuration | No configuration file, except where a step names `--config`. |
| Evidence | `docs/exploratory-testing/2026-09-26-core-journeys/` |

Readiness check: `mutarust --version` printed `mutarust 0.1.9` in the capped
container.

## Journeys

### J1: Find weak tests and apply a score gate

Goal: the user runs `mutarust .`, sees the escaped mutants, and sets a score
gate. The tool must not change the user source tree.

| Step | Result | Evidence |
| --- | --- | --- |
| `--dry-run`, `--list-files`, then `mutarust .` | 38 mutants: 14 killed, 20 escaped, 4 skipped. Score 47.37% = (14 + 4) / 38, as `docs/cli.md` specifies. Each escaped mutant is a real gap in the tests. `git status` is clean. No `target/` or `Cargo.lock` in the user package. | [j1-first-run.txt](2026-09-26-core-journeys/j1-first-run.txt) |
| `--min-msi 48`, then `--min-msi 47` | Exit value 4 with a clear message, then exit value 0. The limit is correct. | [j1-gates-reports.txt](2026-09-26-core-journeys/j1-gates-reports.txt) |
| All report outputs | `report.json`, `mutarust-report.html`, `mutarust-summary.json`, `mutarust-agentic.json`, and `mutarust-gitlab.json` were written. Their counts agree with the terminal. | [j1-gates-reports.txt](2026-09-26-core-journeys/j1-gates-reports.txt) |
| `--coverage --min-covered-msi 60` | 10 mutants are not covered. They are exactly the lines that no test calls (`sum`, `return 99`). Covered-code score 57.14% = 16 / 28. Exit value 4. `--per-test` gives the same results as the run without coverage. | [j1-coverage.txt](2026-09-26-core-journeys/j1-coverage.txt) |
| Two-member workspace, `mutarust ...` | Correct source names (`core/src/lib.rs`). The tests in `tests/` killed the mutants. The user tree is not changed. | [j1-workspace.txt](2026-09-26-core-journeys/j1-workspace.txt) |
| Mutant that does not stop, `--timeout 5` | 5 errored mutants with "timed out after 5 seconds". No child process remains. `/tmp` is empty after the run. | [j1-timeout.txt](2026-09-26-core-journeys/j1-timeout.txt) |
| Incorrect input | An unknown configuration field, `--workers 0`, an incorrect `--output-statuses`, an unknown package, and `--min-msi 101` each give exit value 3 with a clear cause. | [j1-config-errors.txt](2026-09-26-core-journeys/j1-config-errors.txt) |

### J2: Accept the current escaped mutants on a legacy crate

Goal: record a baseline. Then fail only on new escaped mutants. Accept one
false positive with `--blacklist`.

| Step | Result | Evidence |
| --- | --- | --- |
| `--update-baseline`, then `--baseline ... --fail-on-escaped` | 20 IDs were written. The next run gave exit value 0. | [j2-baseline.txt](2026-09-26-core-journeys/j2-baseline.txt) |
| Add a new function at the top of the file (all lines move) | The old IDs stay the same. 3 new escaped mutants cause exit value 4. | [j2-baseline-new-code.txt](2026-09-26-core-journeys/j2-baseline-new-code.txt) |
| `--run-mutant-id` with a real ID and with an unknown ID | The real ID shows only that mutant. The unknown ID gives exit value 3 with a clear message. | [j2-baseline.txt](2026-09-26-core-journeys/j2-baseline.txt) |
| `--blacklist` with the printed ID | **Failure (B1).** No effect and no warning. No output contains a checksum. | [j2-blacklist.txt](2026-09-26-core-journeys/j2-blacklist.txt) |

### J3: Test only changed lines in CI

Goal: in a pull request, mutate only the changed lines. Show the escaped
mutants as CI annotations, and apply a score gate.

| Step | Result | Evidence |
| --- | --- | --- |
| Feature branch, `--git-diff-lines --git-diff-base main` | Only lines 10 and 24 (the changed lines) were mutated. A comment-only change gave no mutants. | [j3-git-diff.txt](2026-09-26-core-journeys/j3-git-diff.txt), [j3-git-diff-run.txt](2026-09-26-core-journeys/j3-git-diff-run.txt) |
| Clone with `origin/HEAD`, default base, `--logger-github --min-msi 50` | The default base came from `origin/HEAD`. The run gave warnings and exit value 4. On `main` with no changes, the run gave exit value 4 without `--ignore-msi-with-no-mutations` and exit value 0 with it, as documented. | [j3-ci-clone.txt](2026-09-26-core-journeys/j3-ci-clone.txt) |
| Package in a repository subdirectory (`crates/pricing`) | **Failure (B2).** The line selection is correct, but the warnings and report paths are `src/lib.rs`, not `crates/pricing/src/lib.rs`. | [j3-subdir-crate.txt](2026-09-26-core-journeys/j3-subdir-crate.txt), [j3-subdir-git-diff.txt](2026-09-26-core-journeys/j3-subdir-git-diff.txt) |

## Confirmed bugs

### B1: Users cannot get a blacklist checksum ([#198](https://github.com/quality-gates/mutarust/issues/198))

- User impact: the documented `--blacklist` journey fails. If a user puts the
  printed `ID:` value in the file, it has no effect and Mutarust gives no
  warning.
- Starting conditions: a new Cargo package with one weak test. No
  configuration.
- Replay: [replay-blacklist.sh](2026-09-26-core-journeys/replay-blacklist.sh).
- Expected: an output gives the checksum for each mutant, or the documents
  tell how to get it. `README.md` says "Blacklist a known false positive by
  checksum".
- Actual: the terminal output, `--debug`, and all six report outputs do not
  contain the checksum. The blacklist works only with the MD5 value that the
  user calculates from the format in `src/evidence.rs`.
- Repeat observations: two replays from a clean state on `406beab`, one on
  `e599200`, and one in the scratch package. See
  [replay-blacklist.txt](2026-09-26-core-journeys/replay-blacklist.txt).

### B2: Report paths are relative to the Cargo workspace ([#199](https://github.com/quality-gates/mutarust/issues/199))

- User impact: the documented CI command, used in a repository that has the
  Rust package in a subdirectory, gives GitHub warnings and GitLab Code
  Quality findings for a path that does not exist. The pull request cannot
  show them on the file.
- Starting conditions: a Git repository with one Cargo package at
  `rust/calc/`.
- Replay: [replay-paths.sh](2026-09-26-core-journeys/replay-paths.sh).
- Expected: `file=rust/calc/src/lib.rs`. `docs/json-outputs.md` says "Source
  names are repository-relative".
- Actual: `file=src/lib.rs` in the GitHub warnings, `mutarust-gitlab.json`,
  and `mutarust-agentic.json`. The cause is `source_root` in
  `src/execution.rs`, which is the Cargo workspace root.
- Repeat observations: two replays from a clean state on `406beab`, one on
  `e599200`, and three runs in the scratch repository. See
  [replay-paths.txt](2026-09-26-core-journeys/replay-paths.txt).

## Unresolved candidates

None.

## Rejected candidates

- `test_files` is empty in `mutarust-agentic.json` when the tests are in
  `#[cfg(test)]` in the same file. `docs/json-outputs.md` defines the field as
  "Nearby test file paths". The result agrees with the documents.
- `coveredCodeMsi` is `0.0` without `--coverage`. The documented example shows
  the same value.
- In the terminal per-mutator table, the `Killed` column includes errored
  mutants (7 in the table, 2 in the summary, in `j1-timeout.txt`).
  `docs/json-outputs.md` defines this for `mutatorStats`, "as in Mutago".
- A `--git-diff-lines` run stopped with "clean cargo test failed". The cause
  was the test scenario: the change broke a test. Mutarust must stop before
  mutation in this condition, as `docs/cli.md` says. That output was replaced
  by the next run and is not kept as a file.

## Usability observations

These are observations from the journeys. The suggestions are not bugs.

- The `--fail-on-escaped` failure says "3 new mutant(s) escaped" but does not
  identify them. The result lines do not mark new and accepted mutants
  differently. Suggestion: print the new IDs with the failure message.
- A skipped mutant prints the full compiler error, which can be 20 lines for
  each mutant. In the first run, the compiler errors were 56 of 259
  lines.
- In a repository with no `origin/HEAD`, `--git-diff-lines` on a feature
  branch uses the current branch as the base. It finds only uncommitted
  changes and reports 0 mutants with exit value 0. This agrees with the
  documents, but a user can think that the run passed.
- `docs/cli.md` does not say that the terminal per-mutator `Killed` column
  includes errored mutants.

## Areas not explored

- `--exec` custom test commands, `--no-exec`, `--do-not-remove-tmp-folder`.
- `--test-flags`, `--test-recursive`, `--timeout-coefficient`, `--workers`
  more than 1 (all runs used one CPU).
- Custom mutators, `--print-ast`, Bash completion, source annotations.
- The HTML report was checked for its presence only. Its content was not
  examined in a browser.
- macOS host binaries. The pass used only Linux containers.

## Limits of this pass

- Each run used one CPU and 512 MB of memory. Parallel-worker behaviour was
  not examined.
- Git operations in the container used `safe.directory "*"` for the mounted
  scratch repository. This setting does not change Mutarust behaviour.
- The coverage image build needed network access. The coverage runs did not.
