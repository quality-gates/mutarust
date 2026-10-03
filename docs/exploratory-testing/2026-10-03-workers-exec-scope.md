# Exploratory testing: workers, custom test commands, and source scope (2026-10-03)

This report gives the results of one exploratory testing pass. The pass used
the `mutarust` command, which is the supported interface, on small scratch
Cargo packages. It examined areas that the
[2026-09-26 pass](2026-09-26-core-journeys.md) did not explore. The pass found
two confirmed bugs:

- [#216](https://github.com/quality-gates/mutarust/issues/216): Parallel
  workers share one Cargo build lock, so builds run one at a time.
- [#217](https://github.com/quality-gates/mutarust/issues/217): An
  `exclude_dirs` entry that starts with `./` excludes nothing.

## Set-up

| Item | Value |
| --- | --- |
| Build | mutarust 0.2.1, commit `e212c49`. Linux release binary (`cargo build --release --locked`) in `rust:1.85-slim`. See [et.Dockerfile](2026-10-03-workers-exec-scope/et.Dockerfile). |
| Limits | `--cpus=1 --memory=512m --network=none` by default. The worker runs used `--cpus=2 --memory=1g` or `--cpus=4 --memory=3g`. See [run.sh](2026-10-03-workers-exec-scope/run.sh). |
| Starting state | Four new Git repositories: `pricing` (four functions, three weak tests), `scope` (source annotations, a `cfg` item, a `src/gen` directory, an untested file), `slow` (one test that sleeps 4 s), and `heavy` (3000 functions from `gen.py`, 1500 of them generic, that take long to compile). Sources are in `2026-10-03-workers-exec-scope/packages/`. |
| Configuration | No configuration file, except where a step names `--config`. |
| Evidence | `docs/exploratory-testing/2026-10-03-workers-exec-scope/` |

Readiness check: `mutarust --version` printed `mutarust 0.2.1` in the capped
container.

## Journeys

### J1: Use a custom test command

Goal: the user runs their own test command for each mutant with `--exec`. The
exit value of the command sets the mutant state, as `docs/cli.md` specifies.
The user then examines the mutants with `--no-exec` and
`--do-not-remove-tmp-folder`.

| Step | Result | Evidence |
| --- | --- | --- |
| `--exec ./check.sh` (exit 0 kills, exit 1 lets escape) | 27 mutants: 16 killed, 11 escaped. The command ran once for each mutant in a new copied workspace. All seven environment values were correct. The user tree is not changed. | [j1-exec.txt](2026-10-03-workers-exec-scope/j1-exec.txt), [j1-exec-env.log](2026-10-03-workers-exec-scope/j1-exec-env.log) |
| Exit values 2, 3, 7, and a command that does not stop (`--timeout 2`) | 2 gave `skipped`. 3 and 7 gave `errored` with the exit value. The timeout gave `errored` with "timed out after 2 seconds". No child process remained. `MUTATE_ORIGINAL` and `MUTATE_CHANGED` differed by one line for each mutant. | [j1-exec-codes.txt](2026-10-03-workers-exec-scope/j1-exec-codes.txt) |
| Missing, empty, and incorrectly quoted commands; `--exec` with `--test-flags`, `--timeout-coefficient`, `--coverage`, `--no-exec` | Each gave exit value 3 with a clear cause, before mutation. | [j1-exec-errors.txt](2026-10-03-workers-exec-scope/j1-exec-errors.txt) |
| `--no-exec`, then `--do-not-remove-tmp-folder --workers 1` | Each mutant area was printed and kept. Each area contained exactly the one changed line. | [j1-noexec-keep.txt](2026-10-03-workers-exec-scope/j1-noexec-keep.txt) |

### J2: Limit the mutation scope

Goal: the user stops mutation of selected code with source annotations,
configuration fields, `--match`, and `--enable`.

| Step | Result | Evidence |
| --- | --- | --- |
| All mutants (`--exec false` makes each mutant escape, so each diff shows) | 22 mutants. `mutator-disable-func` (also above a doc comment and an attribute, and on an `impl` method), `mutator-disable-next-line`, and `mutator-disable-regexp` removed exactly the specified mutants. | [j2-all.txt](2026-10-03-workers-exec-scope/j2-all.txt) |
| `exclude_dirs: [src/gen]` with `skip_with_cfg: true` | `src/gen` and the `#[cfg(feature = "fast")]` function were not mutated. | [j2-config.txt](2026-10-03-workers-exec-scope/j2-config.txt) |
| `skip_without_test: true` | Only `src/lib.rs` (the file with `#[cfg(test)]`) was mutated. | [j2-config.txt](2026-10-03-workers-exec-scope/j2-config.txt) |
| `enable_mutators`, `disable_mutators`, `ignore_source_lines`; then `--enable` | The correct mutators and lines. `--enable` replaced `enable_mutators`. The disable list and ignored lines stayed. | [j2-config.txt](2026-10-03-workers-exec-scope/j2-config.txt) |
| `--match "^(base\|legacy)$"` | Only `base` was mutated. `legacy` has a `mutator-disable-regexp` annotation. | [j2-config.txt](2026-10-03-workers-exec-scope/j2-config.txt) |
| `exclude_dirs: [./src/gen/]` | **Failure (B2).** No source was excluded, and no warning was given. | [j2-config.txt](2026-10-03-workers-exec-scope/j2-config.txt), [j2-exclude-dirs.txt](2026-10-03-workers-exec-scope/j2-exclude-dirs.txt) |

### J3: Run faster with parallel workers, and read the reports

Goal: the user sets `--workers` to make a run faster. The result must be the
same as a run with one worker. The user then reads the HTML report and uses
the syntax tree and Bash completion helpers.

| Step | Result | Evidence |
| --- | --- | --- |
| Default worker count with `--cpus=1` | 1 worker. Mutarust uses the container CPU limit, not the 8 host CPUs. | [j3-default-workers.txt](2026-10-03-workers-exec-scope/j3-default-workers.txt) |
| `pricing`, `--workers 1` and `--workers 2`, with all reports | The mutant states and the reports agree. **Failure (B1).** The 2-worker output shows "Blocking waiting for file lock on build directory" for 5 of 5 skipped mutants. | [j3-pricing-w1.txt](2026-10-03-workers-exec-scope/j3-pricing-w1.txt), [j3-pricing-w2.txt](2026-10-03-workers-exec-scope/j3-pricing-w2.txt) |
| `slow` (tests take 4 s), 1 and 4 workers | 41 s and 17 s. The test runs are parallel. | [j3-parallel-timing.txt](2026-10-03-workers-exec-scope/j3-parallel-timing.txt) |
| `heavy` (long compile), 1 and 4 workers | **Failure (B1).** 151 s with 1 worker and 288 s with 4 workers. In a 4-worker replay (201 s), 2 of 14 mutants were `errored` with "cargo test timed out after 60 seconds". In 385 of 387 samples with 4 active `cargo test` processes, only one `rustc` process ran. | [j3-parallel-build-timing.txt](2026-10-03-workers-exec-scope/j3-parallel-build-timing.txt), [j3-heavy-w4-guarded-output.txt](2026-10-03-workers-exec-scope/j3-heavy-w4-guarded-output.txt), [j3-rustc-concurrency.txt](2026-10-03-workers-exec-scope/j3-rustc-concurrency.txt) |
| HTML report | Self-contained (no external URL). The counts, the per-mutator table, and each escaped mutant with its ID, blacklist checksum, line, and diff agree with the terminal. | [j3-pricing-w1.txt](2026-10-03-workers-exec-scope/j3-pricing-w1.txt), [mutarust-report.html](2026-10-03-workers-exec-scope/j3-pricing-w1-reports/mutarust-report.html) |
| `--print-ast src/other.rs`; `GO_FLAGS_COMPLETION=1 mutarust --lo` | The path, then the syntax tree. The completion gave the four `--logger-*` options and exit value 2, as documented. | [j3-ast-completion.txt](2026-10-03-workers-exec-scope/j3-ast-completion.txt) |

## Confirmed bugs

### B1: Parallel workers share one Cargo build lock ([#216](https://github.com/quality-gates/mutarust/issues/216))

- User impact: more workers do not make builds faster. In a crate with a long
  compile, 4 workers were slower than 1 worker. The time that a worker waits
  for the lock counts in the test timeout. Thus mutants can become `errored`,
  and errored mutants count as successful in the mutation score.
- Starting conditions: a new Cargo package with one function and one test.
  No configuration. `--cpus=2`.
- Replay: [replay-shared-lock.sh](2026-10-03-workers-exec-scope/replay-shared-lock.sh).
- Expected: each worker has its own `.cargo-lock` file. `docs/cli.md` says
  "Each worker keeps one isolated mutation area". The `WarmBuilds` comment in
  `src/execution.rs` says "no two workers share a writable target dir".
- Actual: `target/debug/.cargo-lock` in each worker area is a symbolic link
  to the lock file of the clean-suite area. Cargo prints "Blocking waiting for
  file lock on build directory".
- Repeat observations: two replays from a clean state
  ([replay-shared-lock.txt](2026-10-03-workers-exec-scope/replay-shared-lock.txt)),
  one poll during a normal run
  ([j3-lock-normal.txt](2026-10-03-workers-exec-scope/j3-lock-normal.txt)),
  and three `heavy` runs with 4 workers.
- Note: the first 4-worker `heavy` run gave a score of 64.29%. The 1-worker
  run gave 42.86%. The mutant records of the 64.29% run are lost (see
  [Limits of this pass](#limits-of-this-pass)). A replay with the default
  timeout gave 2 errored mutants. A replay with `--timeout 600` gave no
  errored mutants and 42.86%.

### B2: An `exclude_dirs` entry that starts with `./` excludes nothing ([#217](https://github.com/quality-gates/mutarust/issues/217))

- User impact: a user who writes `./src/gen` thinks that the directory is
  excluded. Mutarust mutates it and gives no warning.
- Starting conditions: a Cargo package with `src/lib.rs` and `src/gen/mod.rs`.
- Replay: [replay-exclude-dirs.sh](2026-10-03-workers-exec-scope/replay-exclude-dirs.sh).
- Expected: `./src/gen` excludes the same sources as `src/gen`, or Mutarust
  rejects it. `docs/config.md` says "A relative prefix is relative to the
  Cargo workspace root."
- Actual: `src/gen`, `src/gen/`, and the absolute path exclude the directory.
  `./src/gen` and `./src/gen/` do not. The probable cause is the
  component-wise `Path::starts_with` in `src/filter.rs`.
- Repeat observations: two replays from a clean state
  ([replay-exclude-dirs.txt](2026-10-03-workers-exec-scope/replay-exclude-dirs.txt))
  and two runs in the `scope` package.

## Unresolved candidates

- With `--do-not-remove-tmp-folder --workers 1`, Mutarust kept one area for
  each mutant (3 areas, 7.7 MB each), not one area for the worker.
  `docs/cli.md` says that each worker keeps one isolated mutation area, but a
  retained area must also show one mutant. This pass did not find which
  behaviour is intended. See
  [j1-noexec-keep.txt](2026-10-03-workers-exec-scope/j1-noexec-keep.txt).

## Rejected candidates

- In the terminal per-mutator table, the `Killed` column includes errored
  mutants. The 2026-09-26 pass rejected this with the same evidence.
- A run where all mutants are `errored` gives a score of 100%
  ([j1-exec-codes.txt](2026-10-03-workers-exec-scope/j1-exec-codes.txt)).
  `docs/cli.md` defines the score as the killed, errored, and skipped count
  divided by the full mutant count.

## Usability observations

These are observations from the journeys. The suggestions are not bugs.

- For a skipped mutant with retained areas, Mutarust adds the area to the
  last compiler error line: `... due to 1 previous error; mutation area:
  /tmp/mutarust-29-2`. Other states show the area on a separate line.
  Suggestion: use a separate line for all states.
- The HTML report shows "0% Covered-code mutation score" for a run without
  `--coverage`. A user can think that no covered code was tested.
  Suggestion: show that coverage was not collected.
- The `--exec` exit values are opposite to the usual test convention: exit 0
  kills the mutant. This agrees with `docs/cli.md`. A user who
  uses `cargo test` directly as the command gets each surviving mutant as
  `killed`.
- With 4 workers, the `heavy` run used 2.1 GB of `/tmp` for 14 mutants.

## Areas not explored

- Custom mutators through the library API (`RegistryBuilder`).
- `--test-recursive` and `--test-flags` with a multi-package workspace.
- `--timeout-coefficient` with parallel workers.
- The HTML report in a browser. The pass examined its text only.
- macOS host binaries. The pass used only Linux containers.

## Limits of this pass

- The host disk became full during the first 4-worker `heavy` run. The
  OrbStack Docker engine stopped. The score and wall time of that run are
  recorded, but its mutant records are lost. The
  pass removed the repository `target/` directory (1.8 GB of build output)
  to continue. Later 4-worker runs used [guard.sh](2026-10-03-workers-exec-scope/guard.sh),
  which records the peak `/tmp` use and stops Mutarust above 3 GB.
- The `heavy` timings come from one host with other loads. The 151 s
  1-worker run took 75 s in a replay. Thus the timings show a direction, not
  an exact value.
- The process sampler ([rustc-sampler.sh](2026-10-03-workers-exec-scope/rustc-sampler.sh))
  samples every 0.5 s. It can miss short `rustc` processes.
- Git operations in the container used `safe.directory "*"` for the mounted
  scratch repositories. This setting does not change Mutarust behaviour.
