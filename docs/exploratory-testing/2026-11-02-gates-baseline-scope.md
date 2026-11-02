# Exploratory testing: gates, baselines, and changed lines

Date: 2026-11-02.

## Result

No product bug was confirmed in these journeys. No issue was filed. The pass
used commit `1879275`, after the merge of pull request #231. It did not test
the changes in the open pull requests. The existing
[worker lock issue #216](https://github.com/quality-gates/mutarust/issues/216)
and [report path issue #199](https://github.com/quality-gates/mutarust/issues/199)
were not reproduced or refiled in this pass.

## Setup

The pass built the Linux debug binary with `cargo build --locked -j 2` in a
container based on `rust:1.93-slim`. The container had two CPUs and 2 GiB of
memory. This differs from the repository recommendation of one CPU and 512 MiB.
The pass makes no timing comparison. The build used the repository `Cargo.lock`.
The runs used one Mutarust worker and had no network access. Cargo used offline mode for the consumer
package. Cargo cache, build files, temporary mutation areas, and the consumer
package were inside the worktree. The consumer package had no external
dependencies.

The [replay script](2026-11-02-gates-baseline-scope/replay.sh) creates the
consumer package and a local Git repository. It selects only
`conditional/bool-literal`. The source has two functions, and a unit test
checks one of them. The script creates a clean package at the start and
resets the source before the changed-line journey. The script records command
output and report content in a new directory under `.scratch`. The files linked
below record the second run. It matched the first run except for the
agent-ready report timestamp. The [comparison hashes](2026-11-02-gates-baseline-scope/replay-comparison.txt)
exclude that timestamp. After the replay, the script was changed to write to a
new scratch directory instead of replacing the evidence. A third run of the
changed script matched the recorded output, apart from the timestamp. Use a
disposable worktree. To replay, build the image and binary, then run the script
without network access:

```sh
docker build -t mutarust-explore docs/exploratory-testing/2026-11-02-gates-baseline-scope
mkdir -p .scratch/cargo .scratch/tmp .scratch/home
docker run --rm --cpus=2 --memory=2g --user "$(id -u):$(id -g)" -v "$PWD:/work" -w /work \
  -e HOME=/work/.scratch/home -e CARGO_HOME=/work/.scratch/cargo \
  -e CARGO_TARGET_DIR=/work/.scratch/target -e TMPDIR=/work/.scratch/tmp \
  mutarust-explore cargo build --locked -j 2
docker run --rm --cpus=2 --memory=2g --network=none --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" -w /work -e HOME=/work/.scratch/home \
  -e CARGO_HOME=/work/.scratch/cargo \
  mutarust-explore sh docs/exploratory-testing/2026-11-02-gates-baseline-scope/replay.sh
```

The first attempt used an image without Git and stopped before the journeys.
After Git was installed, the first source used `pub fn checked() -> bool { true }`
and `pub fn unchecked() -> bool { true }`. That produced zero mutants. The [initial output](2026-11-02-gates-baseline-scope/initial-empty.txt)
records the result. The [mutator guide](../mutators.md) limits this mutator to
specific expression positions, so this was not classified as a product bug.
The replay script uses local initializers instead.

## Journey 1: reports and the mutation score gate

The goal was to find a weak test and use the mutation score as a gate. With one tested
function and one untested function, the normal run reported one killed and
one escaped mutant, a 50% mutation score, and exit value 0. The full, compact,
agent-ready, and GitLab JSON outputs showed one escaped mutant. The three
reports that contain stable mutant IDs showed the same ID. The HTML report
contained the untested function. A second run with
`--min-msi 100` returned 4. The source SHA-256 before and after the runs was
the same. See [journey 1 output](2026-11-02-gates-baseline-scope/journey1.txt).

## Journey 2: accept an existing escape

The goal was to accept one known escape without accepting a new one. The
baseline update wrote the stable mutant ID of the escape. A run with that baseline and
`--fail-on-escaped` returned 0. After the script added another untested
function, the same command returned 4 and reported one new escaped mutant.
The baseline file still contained only the original ID. See
[journey 2 output](2026-11-02-gates-baseline-scope/journey2.txt).

## Journey 3: select changed lines

The goal was to test only the changed source. A dry run against `HEAD` with no
source changes selected zero mutants. After the script added an untested
function, a normal changed-line run selected one escaped mutant. The recorded
Git diff before and after the run was the same. See
[journey 3 output](2026-11-02-gates-baseline-scope/journey3.txt).

## Limits and observations

The pass used one mutator and one small crate with Rust 1.93. It did not
exercise LLVM coverage, per-test coverage, parallel workers, custom test
commands, or large workspaces. The changed-line journey used an unstaged
change against `HEAD`; it did not test staged changes, commits, or renames.
The baseline journey did not move the original escaped line. It did not verify HTML rendering in
a browser or validate every field against the published schemas. The report
checks read the files and compared their visible counts and IDs. The empty
first run illustrates that a selected mutator does not necessarily change every
literal. The mutator guide documents the supported positions.
