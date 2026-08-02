# Report Schemas

Mutarust can write JSON and HTML files after a completed mutation run. A report
is written only when a command option or configuration field enables it. A
baseline update does not write these reports.

| File | Enable with |
| --- | --- |
| `report.json` | `json_output: true` |
| `mutarust-summary.json` | `--logger-summary-json` |
| `mutarust-report.html` | `html_output: true` |
| `mutarust-agentic.json` | `--logger-agentic-json` |

See the [command guide](cli.md) and the [configuration guide](config.md) for
how to enable each report.

## Full report: `report.json`

Enable with `json_output: true` in a configuration file:

```text
mutarust --config mutarust.yml [TARGET]...
```

```json
{
  "metadata": {
    "version": "0.1.0",
    "hasCoverage": false,
    "oneMutant": false
  },
  "stats": {
    "totalMutantsCount": 2,
    "killedCount": 1,
    "notCoveredCount": 0,
    "escapedCount": 1,
    "errorCount": 0,
    "skippedCount": 0,
    "msi": 0.5,
    "coveredCodeMsi": 0.0
  },
  "mutatorStats": [
    {
      "name": "conditional/bool-literal",
      "killed": 1,
      "escaped": 1,
      "skipped": 0,
      "total": 2
    }
  ],
  "escaped": [
    {
      "id": "4582b234c128077507b7558eb62c337e",
      "mutator": {
        "mutatorName": "conditional/bool-literal",
        "originalFilePath": "checked/src/lib.rs",
        "originalStartLine": 2
      },
      "diff": "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -2,1 +2,1 @@\n-pub fn unchecked() -> bool { let value = true; value }\n+pub fn unchecked() -> bool { let value = false; value }\n"
    }
  ],
  "killed": [
    {
      "id": "c2b28e81b2cc0af0ff4a6a1225106223",
      "mutator": {
        "mutatorName": "conditional/bool-literal",
        "originalFilePath": "checked/src/lib.rs",
        "originalStartLine": 1
      },
      "diff": "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -1,1 +1,1 @@\n-pub fn checked() -> bool { let value = true; value }\n+pub fn checked() -> bool { let value = false; value }\n"
    }
  ],
  "errored": []
}
```

| Field | Type | Description |
| --- | --- | --- |
| `metadata.version` | string | Mutarust package version |
| `metadata.hasCoverage` | boolean | True when normal coverage shaped the run |
| `metadata.oneMutant` | boolean | True when `--run-mutant-id` selected one mutant |
| `stats` | object | Same fields as the compact summary |
| `mutatorStats` | array | Per-mutator counts for tested mutants; omitted when empty. The `killed` field counts killed and errored mutants, as in Mutago. |
| `escaped` | array | Escaped mutants |
| `killed` | array | Killed mutants |
| `skipped` | array | Skipped mutants; omitted when empty |
| `errored` | array | Errored mutants |
| `notCovered` | array | Not-covered mutants; omitted when empty |
| `generated` | array | Generated mutants from dry-run or no-exec; omitted when empty |
| `*.id` | string | Stable mutant ID |
| `*.mutator.mutatorName` | string | Mutator name |
| `*.mutator.originalFilePath` | string | Repository-relative source path |
| `*.mutator.originalStartLine` | integer | One-based source line of the mutation |
| `*.diff` | string | Unified source diff |
| `*.processOutput` | string | Optional error detail; omitted when absent |

Score fields use a ratio from zero to one. Source paths use `/` separators.

### Documented report forms

| Form | Shape |
| --- | --- |
| Empty run | All counts are zero. `escaped`, `killed`, and `errored` are empty arrays. Optional arrays are omitted. |
| Coverage run | `metadata.hasCoverage` is true. `notCovered` lists uncovered mutants when any exist. `coveredCodeMsi` uses covered mutants only. |
| One-mutant run | `metadata.oneMutant` is true. Arrays hold only the selected mutant. |
| Dry-run or no-exec | Results appear in `generated`. Tested-state arrays stay empty. |
| Baseline update | No JSON report is written. |

Published JSON Schema: [schema/report.schema.json](../schema/report.schema.json).

## Compact summary: `mutarust-summary.json`

Enable with `--logger-summary-json`:

```text
mutarust --logger-summary-json [TARGET]...
```

```json
{
  "totalMutantsCount": 2,
  "killedCount": 1,
  "notCoveredCount": 0,
  "escapedCount": 1,
  "errorCount": 0,
  "skippedCount": 0,
  "msi": 0.5,
  "coveredCodeMsi": 0.0
}
```

| Field | Type | Description |
| --- | --- | --- |
| `totalMutantsCount` | integer | Total mutants in the run |
| `killedCount` | integer | Killed mutants |
| `notCoveredCount` | integer | Not-covered mutants |
| `escapedCount` | integer | Escaped mutants |
| `errorCount` | integer | Errored mutants |
| `skippedCount` | integer | Skipped mutants |
| `msi` | number | Mutation score from zero to one |
| `coveredCodeMsi` | number | Covered-code mutation score from zero to one |

Use this file for badges and dashboards. `msi` and `coveredCodeMsi` are ratios
from zero to one, not percentages from zero to 100.

Published JSON Schema: [schema/summary.schema.json](../schema/summary.schema.json).

## HTML report: `mutarust-report.html`

Enable with `html_output: true` in a configuration file:

```text
mutarust --config mutarust.yml [TARGET]...
```

The file is self-contained. It uses only inlined CSS and JavaScript. It does
not load assets from an external repository or network host.

The report shows:

- Run counts for total, killed, escaped, errored, not-covered, and skipped mutants
- Total mutation score and covered-code mutation score as percentages
- A per-mutator result table
- Escaped-mutant evidence grouped by source file, with unified diffs

An empty run still writes a valid HTML file with zero counts and the text
`No escaped mutants.`

## Agent-ready report: `mutarust-agentic.json`

Enable with `--logger-agentic-json`:

```text
mutarust --logger-agentic-json [TARGET]...
```

```json
{
  "generated_at": "2026-08-02T22:00:00Z",
  "msi": 0.5,
  "escaped_count": 1,
  "reminder": "A mutant is an example of how this code could be wrong — it's not a script for the test. Don't assert on the mutant directly. Instead ask: if this code were buggy, what would a caller of the public API observe go wrong? Write a test for that.",
  "mutants": [
    {
      "id": "4582b234c128077507b7558eb62c337e",
      "file": "checked/src/lib.rs",
      "line": 2,
      "mutator": "conditional/bool-literal",
      "description": "Changes: `pub fn unchecked() -> bool { let value = true; value }` → `pub fn unchecked() -> bool { let value = false; value }`",
      "kill_hint": "Think about what a caller would observe if this flag were wrong. Write a test that drives the code via its public API with both values and asserts the different outcomes that a correct caller should see",
      "diff": "--- checked/src/lib.rs\n+++ checked/src/lib.rs\n@@ -2,1 +2,1 @@\n-pub fn unchecked() -> bool { let value = true; value }\n+pub fn unchecked() -> bool { let value = false; value }\n",
      "context_start_line": 1,
      "context_lines": [
        "pub fn checked() -> bool { let value = true; value }",
        "pub fn unchecked() -> bool { let value = true; value }"
      ],
      "test_files": [
        "checked/tests/mutation.rs"
      ]
    }
  ]
}
```

| Field | Type | Description |
| --- | --- | --- |
| `generated_at` | string | RFC 3339 generation time |
| `msi` | number | Mutation score from zero to one |
| `escaped_count` | integer | Escaped mutant count |
| `reminder` | string | Interpretation reminder for agents |
| `mutants[].id` | string | Stable mutant ID |
| `mutants[].file` | string | Repository-relative source path |
| `mutants[].line` | integer | One-based source line of the mutation |
| `mutants[].mutator` | string | Mutator name |
| `mutants[].diff` | string | Unified source diff |
| `mutants[].context_start_line` | integer | One-based line of `context_lines[0]` |
| `mutants[].context_lines` | array | Nearby source lines |
| `mutants[].test_files` | array | Nearby test file paths |
| `mutants[].description` | string | Plain-language description of the change |
| `mutants[].kill_hint` | string | Hint for a test that can kill the mutant |

An empty run writes `escaped_count` zero and an empty `mutants` array. Optional
fields such as `description`, `kill_hint`, `context_start_line`,
`context_lines`, and `test_files` are omitted when empty.

Published JSON Schema: [schema/agentic.schema.json](../schema/agentic.schema.json).
