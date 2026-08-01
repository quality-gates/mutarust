# Command Guide

## Mutator Listing

List the available mutation operators:

```text
mutarust --list-mutators
```

The first built-in mutator, `conditional/bool-literal`, changes each Rust
Boolean literal token between `true` and `false`, including macro input.
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

Use `--run-mutant-id ID` to run one stable mutant ID. This mode prints only
the selected mutant evidence. It does not print the normal summary or apply
score gates.

Use `--match REGEXP` to limit mutations to functions with matching names. Use
`--config FILE` with `exclude_dirs` and `ignore_source_lines` to limit source
scope. See the [configuration guide](config.md#source-selection) for these
rules and for the file-local mutation-disable annotations.

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
test controls, or `--do-not-remove-tmp-folder`. `--no-exec` cannot be used
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
