# Configuration Guide

Mutarust reads a YAML configuration file only when the command has a
`--config FILE` option. It does not find a configuration file automatically.
The command reads a relative file name from its current directory.

```text
mutarust --config mutarust.yml [TARGET]...
```

Start with [mutarust.yml.example](../mutarust.yml.example). The published JSON
Schema is [schema/mutarust.schema.json](../schema/mutarust.schema.json).

## Policy Fields

All fields are optional. A Boolean field defaults to `false`. An omitted score
has no score gate. An omitted list is empty. The current command uses silent
mode, mutator selection, source selection, total-score policy, and
covered-score policy. The command does not silently change a user setting to a
different value.

| Field | Type | Purpose |
| --- | --- | --- |
| `skip_without_test` | Boolean | Set the skip-without-test policy. |
| `skip_with_cfg` | Boolean | Set the conditional-compilation policy. |
| `json_output` | Boolean | Write the full report to `report.json`. |
| `html_output` | Boolean | Write the HTML report to `mutarust-report.html`. |
| `silent_mode` | Boolean | Hide status output for individual mutants. |
| `min_msi` | Integer, 0 to 100 | Set the total-score policy. |
| `min_covered_msi` | Integer, 0 to 100 | Set the covered-score policy. |
| `exclude_dirs` | List of paths | Set source directory prefixes to exclude. |
| `disable_mutators` | List of names or group patterns | Disable selected mutators. |
| `enable_mutators` | List of names or group patterns | Select an initial mutator allowlist. |
| `ignore_source_lines` | List of regular expressions | Set source lines to ignore. |

A mutator pattern is an exact name, such as `conditional/bool-literal`, a
group pattern with a final `*`, such as `conditional/*`, or `*` for all
mutators.

`skip_with_cfg` is the Rust adaptation of Mutago `skip_with_build_tags`.

## Command Priority

The command checks the YAML file before it starts Cargo. Unknown fields, wrong
types, invalid scores, invalid mutator patterns, unknown mutator patterns, and
invalid regular expressions fail with a diagnostic that names the configuration
file.

When supplied, `--silent`, `--no-silent`, `--min-msi`, `--min-covered-msi`,
and `--enable` take priority over their configuration fields. `--disable` adds
to `disable_mutators`; it does not replace that list. Do not use `--silent`
and `--no-silent` together.

Mutarust checks the total-score policy after a normal mutation run. A result
below `min_msi` returns exit value 4. With `--coverage`, Mutarust also checks
the covered-score policy after the total-score policy. A positive
`min_covered_msi` without `--coverage` returns exit value 4.

When `json_output` is true, Mutarust writes `report.json` after a completed
run. When `html_output` is true, Mutarust writes `mutarust-report.html`. Use
`--logger-summary-json` for the compact `mutarust-summary.json` file and
`--logger-agentic-json` for `mutarust-agentic.json`. Use `--logger-github` for
GitHub Actions warnings and `--logger-gitlab` for `mutarust-gitlab.json`. See
the [report schemas](json-outputs.md).

## Source Selection

`exclude_dirs` removes source candidates below each path prefix. A relative
prefix is relative to the Cargo workspace root. If the source is outside that
root, the prefix is relative to the common parent directory. For example,
`checked/src/generated` removes candidates in that directory and its child
directories. This setting limits mutation runs. It does not change
`--list-files` output.

Each `ignore_source_lines` expression is checked against complete source
lines. Mutarust does not create a mutation when its changed range touches a
matching line.

Use `--match REGEXP` to mutate only functions whose names match the regular
expression. The expression is not anchored unless it has `^` and `$`.

## Source Annotations

The following file-local line comments disable mutations:

```rust
// mutator-disable-func
fn all_mutators_disabled() -> bool { true }

// mutator-disable-func conditional/bool-literal
fn one_mutator_disabled() -> bool { true }

// mutator-disable-next-line conditional/bool-literal
fn next_line_disabled() -> bool { true }

// mutator-disable-regexp generated conditional/bool-literal
fn generated_value() -> bool { true }
```

An empty mutator list, or `*`, means all mutators. Mutarust checks annotation
mutator names against the full built-in list. A function annotation must be on
the line before its function. A regular-expression annotation uses the first
space to separate its expression from its optional mutator list. Use an
expression without spaces. Invalid annotations return exit value 3 with the
source path and line.

Mutarust uses the same configuration field set as Mutago v2.7.7, with the
Rust conditional-compilation field name described above.
