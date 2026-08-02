# mutarust

`mutarust` is a mutation testing tool for Rust.

The first release work is in progress. Install the command from a source checkout:

```text
cargo install --path .
mutarust --help
```

List Rust production source files before mutation testing:

```text
mutarust --list-files .
```

Print the parsed Rust syntax for a selected source:

```text
mutarust --print-ast src/lib.rs
```

See the [command guide](docs/cli.md) for target rules, the
[mutator reference](docs/mutators.md) for the stable Rust operators, the
[configuration guide](docs/config.md) for YAML mutation policy, and the
[report schemas](docs/json-outputs.md) for JSON, HTML, and agent-ready results.

## License

MIT.
