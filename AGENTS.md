Only report information in ASD-STE100 Simplified Technical English grounded in correct domain language from CONTEXT.md.  

Strongly recommend running `mutarust` inside a capped Docker container (`--cpus=1 --memory=512m --network=none`) so wall-time and resource claims stay comparable and host CPUs/RAM cannot inflate results; build a Linux binary in the image (a host macOS binary will not run).

## Agent skills

### Issue tracker

Use GitHub Issues for issues and PRDs. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

Use a single-context layout. See `docs/agents/domain.md`.

## Cursor Cloud specific instructions

`mutarust` is a Rust command-line tool. It has one service: the `mutarust`
binary. There is no web server and no database.

Use the standard Cargo commands. The CI workflows in `.github/workflows/`
define them:

- Build: `cargo build` (dev) or `cargo build --release --locked`.
- Test: `cargo test --workspace --all-targets --all-features --locked`.
- Lint: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Docs: `cargo doc --workspace --all-features --no-deps --locked`.
- Run: `cargo run -- --help`, or run `target/debug/mutarust` after a build.

Non-obvious notes:

- `cargo test` is slow. The `tests/install_smoke.rs` suite builds and packages
  the crate in temporary directories. A full test run needs about 3 to 4
  minutes.
- To run `mutarust` for a demo, point it at a Cargo crate outside the
  repository. `mutarust` excludes any target path that contains `fixture`,
  `fixtures`, `test`, `vendor`, `generated`, or `build.rs`. A crate inside
  `tests/fixtures/` is not selected.
- The CI production-code lint uses `messrust`. That tool is not installed by
  default. Install it with `cargo install messrust --version 0.1.0 --locked`.
  Then run `messrust src text codesize,design --ignore-tests --strict`.
- The `--coverage` and `--per-test` options need `cargo-llvm-cov` and the
  `llvm-tools-preview` component. These are not installed by default. See
  `docs/cli.md`.
