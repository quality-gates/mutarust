# mutarust

`mutarust` is a mutation testing tool for Rust.

Install a released version with Cargo:

```text
cargo install mutarust
mutarust --help
```

For a source checkout:

```text
cargo install --path .
```

List Rust production source files before mutation testing:

```text
mutarust --list-files .
```

Print the parsed Rust syntax for a selected source:

```text
mutarust --print-ast src/lib.rs
```

## Documentation

- [Install](docs/install.md)
- [Quick Start](docs/quickstart.md)
- [Command guide](docs/cli.md)
- [Configuration](docs/config.md)
- [Mutators](docs/mutators.md)
- [Reports](docs/json-outputs.md)
- [Custom mutators](docs/custom-mutators.md)
- [CI integration](docs/ci.md)
- [Release](docs/release.md)
- [Domain glossary](docs/glossary.md)
- [Mutago v2.7.7 parity table](docs/parity.md)

Optional local hooks live in `githooks/`. They are not active by default. Enable
them with `git config core.hooksPath githooks`.

## License

MIT.
