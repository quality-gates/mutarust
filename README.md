# mutarust

Catch weak tests in Rust before they calcify: small source changes that should
fail the suite and do not. Mutation testing answers whether the tests would
catch a real bug of that shape — not only whether lines executed.

`mutarust` is a local CLI. It mutates Rust production source, runs your Cargo
tests against each mutant, and reports kills, escapes, and scores. Rust 1.85+.

## Quick start

```console
cargo install mutarust
mutarust .
```

That mutates the crate and prints each mutant status on stdout. Exit `0` is
clean against any configured gates, `4` means a score gate failed, and other
non-zero codes mean the tool or the test run failed.

Common next steps:

```console
mutarust --coverage .
mutarust --coverage --min-msi 75 --min-covered-msi 80 .
mutarust --quiet --coverage --logger-github .
```

Full command guide: [docs/cli.md](docs/cli.md).
Configuration: [docs/config.md](docs/config.md).
Mutators: [docs/mutators.md](docs/mutators.md).
Reports: [docs/json-outputs.md](docs/json-outputs.md).

## Install

```console
cargo install mutarust
mutarust --version
```

From a local checkout:

```console
cargo install --path .
```

## Tune the gate

Start without score floors while you learn the escape set. Add coverage so
uncovered lines do not drag the total score, then pin floors the suite can hold:

```console
mutarust --coverage --min-msi 75 --min-covered-msi 80 .
```

Put the same policy in `mutarust.yml` when thresholds need to live in the repo.
See [docs/config.md](docs/config.md).

On a legacy crate with accepted survivors, record a baseline and fail only on
new escapes:

```console
mutarust --update-baseline .
mutarust --baseline mutarust-baseline.json --fail-on-escaped .
```

## Suppress one intentional exception

Blacklist a known false positive by checksum, or skip source lines with config
patterns. See [docs/cli.md](docs/cli.md) (`--blacklist`) and
[docs/config.md](docs/config.md) (`ignore_source_lines`, `disable_mutators`).

## Drop it into CI

```yaml
# GitHub Actions
- uses: dtolnay/rust-toolchain@stable
  with:
    components: llvm-tools-preview
- run: cargo install cargo-llvm-cov mutarust --locked
- run: mutarust --coverage --min-msi 75 --min-covered-msi 80 --logger-github .
```

```yaml
# GitLab Code Quality
script: mutarust --coverage --logger-gitlab --min-msi 75 .
artifacts:
  reports:
    codequality: mutarust-gitlab.json
```

Full workflows and baseline adoption: [docs/ci.md](docs/ci.md).

## Maintainers

Install details: [docs/install.md](docs/install.md).
Release process: [docs/release.md](docs/release.md).
Domain glossary: [docs/glossary.md](docs/glossary.md).
Mutago parity table: [docs/parity.md](docs/parity.md).
Custom mutators: [docs/custom-mutators.md](docs/custom-mutators.md).

Optional local hooks live in `githooks/`. Enable with
`git config core.hooksPath githooks`.

## License

MIT. See [LICENSE](LICENSE).
