# Release

Run all required CI jobs on `main`.

Run the locked package check:

```text
cargo package --locked
```

Publish only after the package and clean-install checks pass:

```text
cargo publish --locked
```

Install the published version into a new Cargo root. Run `mutarust --help` and
`mutarust --version` from that root.
