# Command Guide

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
source files. Add a trailing `...` to select all nested source files.

A bare target name selects a workspace package when that name exists. Use a
relative path such as `./name` to select a directory with the same name.

Mutarust returns absolute paths in sorted order. It removes duplicate paths.
It excludes test, benchmark, example, fixture, vendor, generated, build-output,
hidden, and `build.rs` sources during recursive discovery. It also excludes
files that end in `_test.rs`.

An explicit file or directory target can select test, fixture, vendor, or
generated source. Broader package, workspace, and directory selection excludes
these sources by default.

Mutation execution is not available yet. This command lets users confirm the
source scope before that work is added.
