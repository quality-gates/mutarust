# mutarust

`mutarust` is a mutation test tool for Rust. It makes a small change in
production source. It then runs tests. If the tests do not fail, the mutant
escaped. An escaped mutant shows a gap in the test suite.

## Why use it?

- Coverage can pass when a test does not check the behavior.
- Quality gates can stop a drop in mutation score.
- Changed-line mode keeps pull-request feedback fast.
- Reports help a person or an agent write a better test.

## Quick install

```text
cargo install mutarust
mutarust --coverage --min-msi 75 --min-covered-msi 80 .
```

See [Quick Start](quickstart.md) for a short walkthrough. See the
[domain glossary](glossary.md) for the project terms.
