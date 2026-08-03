# CI Integration

## GitHub Actions

A minimal workflow that gates on mutation score:

```yaml
name: Mutation Testing
on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  mutation:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@f548e57e544e1ff5a4c46bf1e1b8685f8e4a348a # v4.2.0
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@2c7215f132e9ebf062739d9130488b56d53c060c
        with:
          toolchain: stable
          components: llvm-tools-preview
      - run: cargo install cargo-llvm-cov --locked
      - run: cargo build --release --locked
      - run: |
          ./target/release/mutarust \
            --coverage \
            --min-msi 75 \
            --min-covered-msi 80 \
            --logger-github \
            .
```

`--logger-github` prints escaped mutants as `::warning` annotations on the
pull request.

For a high-assurance environment, pin actions to full commit identifiers. Use
Dependabot to keep the pins current.

## Adopt gates on a legacy crate

If escaped mutants already exist, record them first:

```text
mutarust --update-baseline .
git add mutarust-baseline.json
git commit -m "chore: establish mutation baseline"
```

Then fail only when a new mutant escapes:

```yaml
- run: |
    ./target/release/mutarust \
      --baseline mutarust-baseline.json \
      --fail-on-escaped \
      .
```

After tests kill the known escapes, update the baseline again and raise the
score gates.

## Pull-request mode

Limit mutation to changed lines:

```yaml
- uses: actions/checkout@f548e57e544e1ff5a4c46bf1e1b8685f8e4a348a # v4.2.0
  with:
    fetch-depth: 0

- run: |
    ./target/release/mutarust \
      --git-diff-lines \
      --git-diff-base origin/main \
      --ignore-msi-with-no-mutations \
      --min-msi 75 \
      --logger-github \
      .
```

`--ignore-msi-with-no-mutations` returns exit value 0 when the change has no
mutable lines.

## Exit values

| Code | Meaning |
| --- | --- |
| 0 | Gates passed, or no mutants with `--ignore-msi-with-no-mutations` |
| 1 | Command error |
| 4 | A quality gate failed |

## Recommended thresholds

| Maturity | `--min-msi` | `--min-covered-msi` |
| --- | --- | --- |
| Legacy crate | baseline only | baseline only |
| Active work | 60 | 75 |
| Stable library | 75 | 80 |
| High assurance | 90 | 95 |

## Optional local hooks

This repository supplies optional hooks in `githooks/`. They are not active by
default. To enable them once:

```text
git config core.hooksPath githooks
```

`pre-commit` mirrors the fast format, Clippy, and messrust checks.
`pre-push` mirrors changed-line self-mutation against `origin/main`.
