# Selective CI

Maintainer note for path-based job selection in this repository. The workflow
files under `.github/workflows/` are the source of truth. Update this page when
those filters change.

Selective CI only chooses which GitHub Actions jobs run. Product mutation
semantics, CLI behavior, and score gates are unchanged.

## Safe default

Prefer run over skip when unsure. Quality, Messrust, and Security derive a
`*_or_unknown` flag that is true when:

- the heavy path class matches, or
- no light path matched (`light != true`), so a pure unclassified diff still
  runs the heavy jobs.

Skip only when the heavy class is false and light is true. Mutation uses a
top-level workflow `paths` filter instead of that flag (see below).

A false skip — a heavy job skipped when the change could affect it — is a
defect. Fix the path filters. Do not weaken the always-on gates to hide the
miss.

## Path classes

Shared light paths (docs, agent files, changelog, licenses, Markdown, git
metadata, issue templates, Dependabot config) appear in Quality, Messrust, and
Security. Exact globs live in the workflows.

| Class | Typical paths | Effect |
| --- | --- | --- |
| Light | `docs/**`, `**/*.md`, `.agents/**`, `AGENTS.md`, `CHANGELOG.md`, licenses | Heavy Rust jobs may skip when the diff is pure-light |
| Rust (Quality) | `src/**`, `tests/**`, `Cargo.toml`, `Cargo.lock`, `build.rs`, `.cargo/**`, toolchain/lint config, `quality.yml` | Runs lint, dual-toolchain tests, release build, and rustdoc |
| Package (Quality) | `README.md`, or any rust / non-light diff | Runs `cargo package` and install smoke |
| Production (Messrust) | `src/**`, `messrust.yml` | Runs strict messrust on production source |
| Dependencies (Security) | `Cargo.toml`, `Cargo.lock`, `security.yml` | Runs `cargo audit` |
| Mutation inputs | `**/*.rs`, `Cargo.toml`, `Cargo.lock`, `mutation.yml` | Workflow-level filter; mutation job runs only for these paths |
| Docs site | `docs/**`, `mkdocs.yml`, `docs.yml` | Existing MkDocs path filter (not introduced by selective CI) |

## Workflows and gates

| Workflow | Skip behavior | Always-on gate |
| --- | --- | --- |
| `quality.yml` | `lint` / `test` / `build` / `docs` need `run_rust_or_unknown`; `package` needs `run_package` | `quality-gate` — `always()`; accepts `success` or `skipped` per job; fails on any other result |
| `mutation.yml` | Top-level `paths` on mutation inputs for push and pull_request | None (workflow does not start off-path) |
| `messrust.yml` | `messrust` needs `run_messrust_or_unknown` | `messrust-gate` — path detection must succeed; job may be skipped |
| `security.yml` | `audit` needs `run_audit_or_unknown`; schedule always runs audit | `audit-gate` — same pattern as messrust |
| `docs.yml` | Pre-existing path filter on docs site inputs | None |

Branch protection should require the gate job names (`quality-gate`,
`messrust-gate`, `audit-gate`), not the skippable leaf jobs alone. Skipped
leaves must not block merge; a needed failure must still fail the gate.

## Mutation scope on main

When mutation runs, pull requests still limit mutants with `--git-diff-lines`.
Pushes to `main` still use the full approved production scope. Path filters only
decide whether the workflow starts. They do not change mutarust flags or score
thresholds.
