# Release

## Before you publish

Run the required CI jobs on `main`:

- Quality: format, Clippy, MSRV and stable tests, release build, docs, package
- Messrust: strict `codesize` and `design` on production source
- Security: Rust advisory check
- Mutation: full approved production scope
- Docs: user guide build

## Package check

```text
cargo package --locked
```

Install the package artifact into a clean Cargo root. Run help and version
checks from that root.

## Publish

Publish only after the package and clean-install checks pass:

```text
cargo publish --locked
```

Do not print, copy, or store the crates.io token.

## After publication

Install the published version into a new Cargo root:

```text
cargo install mutarust --root /tmp/mutarust-verify --locked
/tmp/mutarust-verify/bin/mutarust --help
/tmp/mutarust-verify/bin/mutarust --version
```

Record the published crate and the installation evidence in the release notes.
