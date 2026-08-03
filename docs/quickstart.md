# Quick Start

## 1. Install

```text
cargo install mutarust
```

Rust 1.85 or later is required.

## 2. Run against your crate

```text
mutarust .
```

Each mutant prints `Killed` when tests detect it, or `Escaped` when tests miss
it. An escaped mutant also prints a source diff.

## 3. Add coverage awareness

Without `--coverage`, uncovered source can lower the total score. With
`--coverage`, mutants on untested lines get the `not covered` state. The
covered-code score ignores those mutants.

```text
mutarust --coverage .
```

Install `cargo-llvm-cov` and the `llvm-tools-preview` component first. See the
[command guide](cli.md).

## 4. Set quality gates

```text
mutarust --coverage --min-msi 75 --min-covered-msi 80 .
```

Exit value 4 means a gate failed. Exit value 0 means all gates passed.

## 5. Reduce noise

Use `--quiet` to show only escaped mutants and the summary:

```text
mutarust --quiet --coverage .
```

## 6. Limit to changed lines

```text
mutarust \
  --git-diff-lines \
  --git-diff-base main \
  --ignore-msi-with-no-mutations \
  --min-msi 75 \
  .
```

## 7. Get agent-ready suggestions

```text
mutarust --logger-agentic-json --quiet .
```

Feed `mutarust-agentic.json` to an agent for targeted test suggestions.
