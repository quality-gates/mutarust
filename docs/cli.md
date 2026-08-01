# Command Guide

## Source Listing

List the Rust production source files that Mutarust selects:

```text
mutarust --list-files [TARGET]...
```

With no target after `--list-files`, Mutarust selects the current directory
recursively. Each target can be an existing Rust source file, an existing
directory, or a package name in the current Cargo workspace. A directory or
package target selects its direct source files. Add a trailing `...` to select
all nested source files.

Mutarust returns absolute paths in sorted order. It removes duplicate paths.
It excludes test, benchmark, example, fixture, vendor, generated, build-output,
hidden, and `build.rs` sources during recursive discovery. It also excludes
files that end in `_test.rs`.

An explicit file or directory target can select vendor or generated source.

Mutation execution is not available yet. This command lets users confirm the
source scope before that work is added.
