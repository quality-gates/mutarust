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
has no score gate. An omitted list is empty. This configuration contract is
complete before each related run feature is complete. The current command uses
silent mode, mutator selection, and the total-score policy. Later feature
changes use the stored source selection, report, and covered-score settings.
The command does not silently change a user setting to a different value.

| Field | Type | Purpose |
| --- | --- | --- |
| `skip_without_test` | Boolean | Set the skip-without-test policy. |
| `skip_with_cfg` | Boolean | Set the conditional-compilation policy. |
| `json_output` | Boolean | Set the JSON report policy. |
| `html_output` | Boolean | Set the HTML report policy. |
| `silent_mode` | Boolean | Hide status output for individual mutants. |
| `min_msi` | Integer, 0 to 100 | Set the total-score policy. |
| `min_covered_msi` | Integer, 0 to 100 | Set the covered-score policy. |
| `exclude_dirs` | List of paths | Set source directory prefixes to exclude. |
| `disable_mutators` | List of names or group patterns | Disable selected mutators. |
| `enable_mutators` | List of names or group patterns | Select an initial mutator allowlist. |
| `ignore_source_lines` | List of regular expressions | Set source lines to ignore. |

A mutator pattern is an exact name, such as `conditional/bool-literal`, or a
group pattern with a final `*`, such as `conditional/*`.

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
below `min_msi` returns exit value 4. The covered-score policy takes effect
with the later coverage feature.

Mutarust uses the same configuration field set as Mutago v2.7.7, with the
Rust conditional-compilation field name described above.
