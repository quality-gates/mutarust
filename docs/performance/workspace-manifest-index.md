# Workspace manifest index measurement

## Result

The index reduces the instruction count for artifact-prefix selection in a large synthetic workspace. It did not give a measurable end-to-end time change in the Tokio sample below. Do not use the synthetic result as a wall-time speedup claim.

## Method

Use the release build at main commit `b99a986` and the build with the manifest index. Both use Rust 1.93.1 in the same `rust:1.93-slim` container, limited to 2 CPUs and 2 GiB. Dependencies are cached. Run Callgrind 3.24.0 without tracing child processes. The count is for `mutarust`, not its `cargo metadata` subprocess. Compare the stdout from both builds byte for byte and require both commands to exit successfully.

For the synthetic sample, create a Cargo workspace with N packages. Give package 0 path dependencies on all other packages. Give each package this source: `pub fn value() -> bool { true }`. Run `mutarust --dry-run crate0000/src/lib.rs`. This makes all N manifests relevant to the selected source. For example, this Python 3 script creates the workspace:

```python
from pathlib import Path
import sys

count = int(sys.argv[1])
root = Path(f"scale-{count}")
root.mkdir()
(root / "Cargo.toml").write_text('[workspace]\nmembers = ["crate*"]\nresolver = "2"\n')
for n in range(count):
    crate = root / f"crate{n:04}"
    (crate / "src").mkdir(parents=True)
    manifest = f'[package]\nname = "pkg{n:04}"\nversion = "0.1.0"\nedition = "2021"\n'
    if n == 0:
        manifest += "[dependencies]\n" + "".join(
            f'pkg{i:04} = {{ path = "../crate{i:04}" }}\n' for i in range(1, count)
        )
    (crate / "Cargo.toml").write_text(manifest)
    (crate / "src/lib.rs").write_text("pub fn value() -> bool { true }\n")
```

Run each release binary under `valgrind --tool=callgrind --callgrind-out-file=profile.out`, then use `callgrind_annotate --inclusive=yes --threshold=99 profile.out`. Each count below is one profile, not a timing sample. The output matched between builds.

| Packages and selected manifests | Prefix function, before | Prefix function, after |
| --- | ---: | ---: |
| 100 | 5,902,921 instructions | 1,565,572 instructions |
| 800 | 338,352,915 instructions | 16,917,340 instructions |

The old scan grew about 57 times when N grew eight times. The indexed version grew about 11 times. The profile includes name and prefix collection as well as membership checks, so these are not comparison counts.

## Real workspace check

Use Tokio commit `6470b5e4b8e943ad014383a5bf6a93ba415678a3`, with a generated lockfile. Its metadata had 202 packages and 10 workspace members. Run `mutarust --dry-run tokio/src/lib.rs tokio-util/src/lib.rs tokio-stream/src/lib.rs tokio-macros/src/lib.rs`. This is a smaller selected-manifest set than the synthetic sample. All runs returned four mutants and identical stdout (SHA-256 `33e2fef366f1a9e8885bdea19564c7a7dc772a968fea78ebdd42dbce61853981`).

Seven alternating runs per build gave a before median of 181 ms (range 173–214 ms) and an after median of 180 ms (range 172–214 ms). The ranges overlap. This is no measurable end-to-end difference, not a speedup claim. The before Callgrind profile attributed 8.23 million of 87.25 million parent-process instructions to `target_artifact_prefixes`; parsing and other workspace setup also consumed instructions. The profile excludes Cargo subprocesses, which are included in wall time.
