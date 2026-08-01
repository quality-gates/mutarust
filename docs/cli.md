# Command Guide

## Source Listing

List the Rust production source files that Mutarust selects:

```text
mutarust --list-files [TARGET]...
```

With no target after `--list-files`, Mutarust selects the current directory.
Each target can be an existing Rust source file, an existing directory, or a
package name in the current Cargo workspace. A directory target is recursive.
Use a trailing `...` on a local directory target for the same recursive
selection form.

Mutarust returns absolute paths in sorted order. It removes duplicate paths.
It excludes test, benchmark, example, fixture, build-output, hidden, and
`build.rs` sources. It also excludes files that end in `_test.rs`.

Mutation execution is not available yet. This command lets users confirm the
source scope before that work is added.
