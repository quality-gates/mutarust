---
name: verify-mutant-isolation
description: Verify mutant isolation. Use when a change edits workspace copy, symbolic links, or mutant writes in src/execution.rs, when a killed or escaped result must be the mutant the test command ran, or when a run must leave the user source tree unchanged.
---

# Verify mutant isolation

Isolation means the Cargo test command reads the mutant in the temporary workspace, and the user source tree stays unchanged.

Harm: a person who runs mutarust treats an escaped mutant as a gap in the tests. If Cargo read the user source, that gap is false. If the user source tree changes, that person's code is changed.

## Property

For every layout in the bound:

- Every Cargo read of `src/lib.rs` resolves outside the user package.
- The clean Cargo test reads the original source. Each later Cargo test reads the mutant.
- Mutarust reports every mutant as killed.
- The user package snapshot is unchanged. The snapshot stores each file's bytes and each symbolic link's target.

The mutant replaces `true` with `false` in `pub fn ready() -> bool { true }`. The test command exits 0 for the original source and 1 for the mutant. Exit 1 is a test failure, so a mutant that Cargo actually reads is killed.

## Bound

Five layouts. One package. One worker. One mutant. The Cargo test command. Unix symbolic links.

- `regular`: `src/lib.rs` is a normal file.
- `file-relative`: `src/lib.rs` points at `included.rs` in the same directory.
- `file-absolute`: `src/lib.rs` points at the absolute path of `src/included.rs`.
- `dir-relative`: `src` points at `libsrc` with a relative target.
- `dir-absolute`: `src` points at the absolute path of `libsrc`.

## Coverage

Sync commit: `153ecb0b46eb637f1a972d0725a2b326c1ac4390`

File: `src/execution.rs`

Functions: `test_clean_layout`, `copy_workspace`, `copy_entry`, `copy_symbolic_link`, `create_symbolic_link`, `copied_path`, `test_candidate_with_scratch`, `WorkerScratch::write_mutant`, `WorkerScratch::reset_for_next_mutant`, `remove_non_target_entries`, `run_mutant_cargo_tests`, `run_cargo_command`, `cargo_failure_is_compile`.

Move the sync commit only after you confirm that this list still describes those functions. Move it when a covered function's behavior changes and the check still describes that function.

## Steps

### 1. Decide whether the change touches this check

Read the diff against the covered file and functions.

Completion: you have a yes or a no. On no, stop. On yes, name each covered function the diff changes.

### 2. Run the check

From the repository root, with Rust 1.85.0:

```text
cargo +1.85.0 run --quiet --locked --manifest-path verification/mutant-isolation/Cargo.toml
```

The lock file is `verification/mutant-isolation/Cargo.lock`. The checker is not part of `cargo test`.

Completion: the process exits 0, 1, or 2, and you have the full stdout. Exit 2 means the check did not run. Restore the run before you judge the code.

### 3. Judge the output

A `PASS` line means that layout holds the property. A `FAIL` line means it does not. Exit 0 means every layout holds. Exit 1 means at least one layout does not.

On the sync commit, `regular`, `file-relative`, and `dir-relative` pass. `file-absolute` and `dir-absolute` fail. Keep the property and the five layouts. That failure is a bug:

1. `copy_symbolic_link` copies `src/lib.rs` or `src` and keeps the absolute target.
2. `WorkerScratch::write_mutant` writes the mutant at the canonical path inside the temporary workspace.
3. `run_cargo_command` starts Cargo on the copied manifest.
4. Cargo opens `src/lib.rs` and follows the absolute link into the user package.
5. The command reads the original source, `pub fn ready() -> bool { true }`.
6. `run_mutant_cargo_tests` reports the mutant as escaped.

The user source tree is unchanged on this bug. The false result is the escaped mutant.

Completion: each `FAIL` line is this absolute-link bug, or a new failure you will report in the code's words. A new `PASS` on `file-absolute` or `dir-absolute` means Cargo now reads the mutant outside the user package.

### 4. Tell a bug from a modeling error

Replay a new `FAIL` against `src/execution.rs`.

A bug: Cargo's read path is inside the user package, the snapshot changed, or the state is not killed, and each of those steps can happen in the covered functions.

A modeling error: the output has no reads, or the read count is not one clean Cargo test plus one Cargo test per mutant, because `test_clean_layout` or `test_candidate_with_scratch` no longer makes that many Cargo tests. The observer follows `src/lib.rs` from the copied manifest, which is the path this package layout gives Cargo. Update the judge from the code, then rerun. Keep the property.

An intended change to the promise needs evidence in the doc comment, the tests, the changelog, or the commit message. Record the old sentence, the new sentence, and that evidence before you change the property.

Completion: you can say bug or modeling error for every `FAIL` that is not the known absolute-link bug, and you can point at the function for each step.

### 5. Move the sync commit

Set the sync commit to the commit you checked. Leave it in place when you could not run the check.

Completion: the sync commit is a commit whose covered functions you confirmed, or you left the old commit and said why.

## Outside the bound

The bound is one worker on the default Cargo path. These stay outside this run: `WorkerScratch::clone_from_template`, `remap_path_prefix_in_tree`, `replace_bytes`, `link_or_copy`, the free function `write_mutant`, `WorkerScratch::create`, a custom command, `--no-exec`, link cycles, more than one link, and Windows.

When covered code changes outside this skill, use `/maintain-formal-verification-skill`.
