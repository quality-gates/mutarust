# Command Guide

## Mutator Listing

List the available mutation operators:

```text
mutarust --list-mutators
```

The built-in mutators use stable Mutago names and Rust expression rules. See
the [mutator reference](mutators.md) for all names, changes, and exclusions.
Run mutation testing for selected production source:

```text
mutarust [TARGET]...
```

Use a strict YAML mutation policy file for one run:

```text
mutarust --config mutarust.yml [TARGET]...
```

See the [configuration guide](config.md) for the policy fields, schema, and
command priority rules.

Mutarust first runs the applicable Cargo tests without a mutation. It stops
before mutation if this clean test suite fails. Mutarust runs each mutant in a
new temporary copy of the Cargo workspace that contains the selected source.
A test failure kills the mutant. A successful test run lets the mutant escape.
Mutarust skips a mutant that does not compile. A test-command or timeout
failure produces an errored mutant. Mutarust then removes the temporary
workspace and prints one result line for each mutant. Each result has a stable
ID. An escaped mutant also has a unified source diff. Use `--no-diffs` to hide
these diffs.

A normal run prints killed, escaped, errored, not-covered, skipped, and total
counts. It also prints the total mutation score and a sorted result table for
each mutator. The score is the killed, errored, and skipped count divided by
the full mutant count. The score is zero when no mutant exists.

Use `--min-msi PERCENT` to require a total mutation score. A failed score gate
returns exit value 4. A score equal to the required percentage passes.

## LLVM Coverage

Use `--coverage` to collect Rust line coverage before mutation. Mutarust uses
the Cargo-compatible [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
tool and its LCOV output. Install the tool and the Rust LLVM tools component
before use:

```text
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
```

Mutarust puts coverage build files in private temporary directories and removes
them before it starts the normal mutation workers. It does not write coverage
build output to the user Cargo target directory. A source line with no recorded
coverage gets the `not covered` result. Mutarust does not write that mutant or
run its tests.

With valid normal coverage, the summary also prints the covered-code mutation
score. This score is killed, errored, and skipped mutants divided by all mutants
except `not covered` mutants. Use `--min-covered-msi PERCENT` to require this
score. A failed covered-score gate returns exit value 4. A positive covered
score gate without `--coverage` returns exit value 4.

Use `--per-test` to collect one LLVM coverage report for each Cargo test and
run only the tests that cover the mutated line. This option can be used without
`--coverage`. A mutant with no per-test mapping runs the full Cargo test suite.
Per-test collection is sequential so it does not create many instrumented Cargo
target directories at one time. `--coverage` and `--per-test` cannot be used
with `--exec`, `--dry-run`, or `--no-exec`.

Use `--run-mutant-id ID` to run one stable mutant ID. This mode prints only
the selected mutant evidence. It does not print the normal summary or apply
score gates.

## Accepted Mutants

Use `--update-baseline` to write the escaped mutants from the current run to
`mutarust-baseline.json`. Use `--baseline FILE` to set another baseline path.
A missing baseline file accepts no escaped mutants. A baseline file has a
format that is compatible with Mutago:

```json
{
  "version": 1,
  "mutants": [
    {
      "id": "4582b234c128077507b7558eb62c337e",
      "file": "checked/src/lib.rs",
      "mutator": "conditional/bool-literal",
      "line": 1
    }
  ]
}
```

Use `--fail-on-escaped` to return exit value 4 only when an escaped stable
mutant ID is not in the baseline. Known escaped mutants remain visible in the
normal result output. An update writes the full current escaped set and exits
successfully before normal output and score gates.
`--update-baseline` cannot be used with `--dry-run`, `--no-exec`, or
`--run-mutant-id`, because these modes do not produce a full escaped set.

Use `--blacklist FILE` one or more times to read accepted mutation checksums.
Each non-empty file line is one 32-character lower-case hexadecimal checksum.
Mutarust does not run a matching mutant. The checksum uses only the changed
source lines, not the source path, mutator name, or line number. A checksum
therefore remains valid after unrelated source edits. Baseline, blacklist, and
one-mutant options are command options. They are not YAML policy fields.

Use `--match REGEXP` to limit mutations to functions with matching names. Use
`--config FILE` with `exclude_dirs` and `ignore_source_lines` to limit source
scope. See the [configuration guide](config.md#source-selection) for these
rules and for the file-local mutation-disable annotations.

## Git Changed-Line Selection

Use `--git-diff-lines` to mutate only production source lines changed from a
Git comparison base. Mutarust compares the base with the current work tree. It
includes committed, staged, and unstaged tracked changes. It selects lines in
added Rust source files and changed lines in renamed Rust source files. An
untracked file and a pure rename have no Git changed-line scope. It does not
select deleted lines.

Use `--git-diff-base REF` to set the base ref. This option requires
`--git-diff-lines`. Without this option, Mutarust uses the default branch from
`origin/HEAD`. If Git has no `origin/HEAD`, Mutarust uses `master`. Mutarust
stops with an error if it cannot find the Git repository, resolve the base ref,
or read the Git comparison. It does not use a wider source scope after a Git
error. Each selected source must be inside that Git repository.

If the current branch and the base branch have a merge base, Mutarust compares
that merge base with the current work tree. If they have no merge base,
Mutarust compares the base ref directly. A run with no changed mutable lines
succeeds. It reports zero mutants and does not run tests.

## Execution Modes and Cargo Controls

Use `--dry-run` to list the selected mutant count without writing mutation
areas or running tests. This command does not change the user workspace.

Use `--no-exec` to write one mutant to each isolated mutation area without
running tests. Mutarust keeps these areas and prints each path. The areas are
in the system temporary directory. Remove them when you no longer need them.

By default, Mutarust removes all isolated mutation areas after the run. Use
`--do-not-remove-tmp-folder` to keep them for inspection. Mutarust prints each
retained path. This option can use much disk space because a normal Cargo run
can create build output in each area.

Mutarust runs Cargo mutants in parallel. By default, it starts up to one worker
per logical CPU. Use `--workers COUNT` to set a positive whole-number limit.
Mutarust never starts more workers than the number of mutants. Each worker uses
one isolated mutation area and its own Cargo target directory. A higher worker
limit can use more temporary disk space. Use a lower limit when disk space is
limited. Mutarust prints result records and the final summary in the same plan
order for sequential and parallel runs. A custom `--exec` command uses one
worker so its command output cannot mix with another command output.

Each Cargo test run has a fixed 60 second timeout by default. Use
`--exec-timeout SECONDS` to set a different positive whole-second timeout.
`--timeout` is an alias. A timeout produces an errored mutant result with an
error message.

Use `--timeout-coefficient FACTOR` for an adaptive Cargo timeout. Mutarust
runs the clean tests first, finds the longest clean test duration, multiplies
it by the positive factor, rounds up to a whole second, and uses at least one
second. This option cannot be used with `--timeout`, `--exec`, or `--no-exec`.

Use `--test-flags FLAGS` to add shell-quoted Cargo test arguments to every
Cargo compile and test command. For example,
`--test-flags "--package my-package --features slow"` limits testing to one
package and enables a feature. This option cannot be used with `--exec` or
`--no-exec`.

Use `--test-recursive` to pass `--workspace` to Cargo and test every package
in the selected Cargo workspace. With `--exec`, Mutarust instead sets
`TEST_RECURSIVE=true` for the custom command. `--test-recursive` cannot be
used with `--no-exec`.

`--dry-run` cannot be used with `--no-exec`, `--exec`, timeout controls, Cargo
test controls, `--workers`, or `--do-not-remove-tmp-folder`. `--no-exec` cannot be used
with `--exec`, timeout controls, or Cargo test controls. These rules prevent a
command from silently ignoring a selected option.

## Custom Test Commands

Use `--exec COMMAND` to run a custom command for each mutant. Mutarust parses
the command with shell quotes. The command runs in the copied workspace. It
does not change the user workspace. A custom command selects its own compiler
and test action.

The command gets these environment values:

| Name | Value |
| :--- | :--- |
| `MUTATE_ORIGINAL` | The original source file in the isolated temporary workspace. |
| `MUTATE_CHANGED` | The changed source file in the copied workspace. |
| `MUTATE_PACKAGE` | The Cargo package name that owns the source. |
| `MUTATE_TIMEOUT` | The timeout in whole seconds. |
| `TEST_RECURSIVE` | `true` when `--test-recursive` is set, else `false`. |
| `MUTATE_VERBOSE` | `true` when `--verbose` is set, else `false`. |
| `MUTATE_DEBUG` | `true` when `--debug` is set, else `false`. |

Use exit value 0 to kill a mutant, 1 to let it escape, and 2 to skip it. Any
other exit value produces an errored mutant. Mutarust stops the command and
its child processes on timeout or interrupt. A missing, empty, or invalid
command returns a clear error before mutation starts.

## Source Listing

List the Rust production source files that Mutarust selects:

```text
mutarust --list-files [TARGET]...
```

With no target after `--list-files`, Mutarust selects the current directory
recursively. Each target can be an existing Rust source file, an existing
directory, a Cargo workspace directory, or a package name in the current
Cargo workspace. Mutarust uses Cargo target data for package and workspace
targets. It includes declared library and binary source paths, including paths
outside `src`. A directory, package, or workspace target selects its direct
source files. Targets that require inactive Cargo features are not selected.
Add a trailing `...` to select all nested source files.

A bare target name selects a workspace package when that name exists. Use a
relative path such as `./name` to select a directory with the same name.

Mutarust returns absolute paths in sorted order. It removes duplicate paths.
It excludes test, benchmark, example, fixture, vendor, generated, build-output,
hidden, and `build.rs` sources during recursive discovery. It also excludes
files that end in `_test.rs`.

An explicit file or directory target can select test, fixture, vendor, or
generated source. Broader package, workspace, and directory selection excludes
these sources by default.

This command lets users confirm the source scope before mutation testing.
