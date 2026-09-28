use std::fs;

use mutarust::{
    CommandFlag, CommandSettings, Configuration, Registry, RegistryConfigurationError,
    configured_registry, validate_command_flags,
};

struct TempConfig(std::path::PathBuf);

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn exact_assign_invert_selectors_work_in_command_settings_and_yaml() {
    let names = Registry::builtins()
        .names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut command_configuration = Configuration::default();
    command_configuration
        .apply(&CommandSettings {
            enable_mutators: Some(vec!["arithmetic/assign_invert".to_owned()]),
            ..CommandSettings::default()
        })
        .expect("the exact command selector must be valid");
    assert_eq!(
        command_configuration
            .select_mutators(&names)
            .expect("the exact command selector must match"),
        vec!["arithmetic/assign_invert"]
    );

    let path = TempConfig(std::env::temp_dir().join(format!(
        "mutarust-underscore-selector-{}.yml",
        std::process::id()
    )));
    fs::write(&path.0, "disable_mutators:\n  - arithmetic/assign_invert\n")
        .expect("the selector fixture must be written");
    let yaml_configuration =
        Configuration::read(&path.0).expect("the exact YAML selector must be valid");
    assert!(
        !yaml_configuration
            .select_mutators(&names)
            .expect("the exact YAML selector must match")
            .contains(&"arithmetic/assign_invert".to_owned())
    );
}

#[test]
fn command_flag_conflicts_are_validated_by_the_library() {
    use CommandFlag as Flag;

    let cases: &[(&[Flag], &str)] = &[
        (
            &[Flag::DryRun, Flag::NoExec],
            "--dry-run and --no-exec cannot be used together",
        ),
        (
            &[Flag::DryRun, Flag::CustomCommand],
            "--dry-run cannot be used with --exec",
        ),
        (
            &[Flag::NoExec, Flag::CustomCommand],
            "--no-exec cannot be used with --exec",
        ),
        (
            &[Flag::DryRun, Flag::KeepTemporary],
            "--dry-run cannot be used with --do-not-remove-tmp-folder",
        ),
        (
            &[Flag::DryRun, Flag::FixedTimeout],
            "--dry-run cannot be used with --timeout",
        ),
        (
            &[Flag::DryRun, Flag::TimeoutCoefficient],
            "--dry-run cannot be used with --timeout-coefficient",
        ),
        (
            &[Flag::DryRun, Flag::TestFlags],
            "--dry-run cannot be used with --test-flags",
        ),
        (
            &[Flag::DryRun, Flag::Workers],
            "--dry-run cannot be used with --workers",
        ),
        (
            &[Flag::DryRun, Flag::RecursiveTests],
            "--dry-run cannot be used with --test-recursive",
        ),
        (
            &[Flag::DryRun, Flag::Coverage],
            "--dry-run cannot be used with --coverage",
        ),
        (
            &[Flag::DryRun, Flag::PerTestCoverage],
            "--dry-run cannot be used with --per-test",
        ),
        (
            &[Flag::TimeoutCoefficient, Flag::FixedTimeout],
            "--timeout-coefficient cannot be used with --timeout",
        ),
        (
            &[Flag::TimeoutCoefficient, Flag::CustomCommand],
            "--timeout-coefficient requires the Cargo test command",
        ),
        (
            &[Flag::TimeoutCoefficient, Flag::NoExec],
            "--timeout-coefficient cannot be used with --no-exec",
        ),
        (
            &[Flag::NoExec, Flag::FixedTimeout],
            "--no-exec cannot be used with --timeout",
        ),
        (
            &[Flag::TestFlags, Flag::CustomCommand],
            "--test-flags cannot be used with --exec",
        ),
        (
            &[Flag::TestFlags, Flag::NoExec],
            "--test-flags cannot be used with --no-exec",
        ),
        (
            &[Flag::RecursiveTests, Flag::NoExec],
            "--test-recursive cannot be used with --no-exec",
        ),
        (
            &[Flag::Coverage, Flag::CustomCommand],
            "--coverage requires the Cargo test command",
        ),
        (
            &[Flag::PerTestCoverage, Flag::CustomCommand],
            "--per-test requires the Cargo test command",
        ),
        (
            &[Flag::Coverage, Flag::NoExec],
            "--coverage cannot be used with --no-exec",
        ),
        (
            &[Flag::PerTestCoverage, Flag::NoExec],
            "--per-test cannot be used with --no-exec",
        ),
        (
            &[Flag::GitDiffBase],
            "--git-diff-base requires --git-diff-lines",
        ),
        (
            &[Flag::UpdateBaseline, Flag::DryRun],
            "--update-baseline cannot be used with --dry-run",
        ),
        (
            &[Flag::UpdateBaseline, Flag::NoExec],
            "--update-baseline cannot be used with --no-exec",
        ),
        (
            &[Flag::UpdateBaseline, Flag::RunMutantId],
            "--update-baseline cannot be used with --run-mutant-id",
        ),
    ];

    for (flags, expected) in cases {
        let error = validate_command_flags(flags).expect_err("incompatible flags must fail");
        assert_eq!(error.to_string(), *expected);
    }
}

#[test]
fn configured_registry_selects_mutators_and_builds_filters() {
    let mut configuration = Configuration::default();
    configuration
        .apply(&CommandSettings {
            enable_mutators: Some(vec!["conditional/bool-literal".to_owned()]),
            ..CommandSettings::default()
        })
        .expect("the selected mutator must be valid");

    let (registry, _filters) = configured_registry(&configuration, Some("sample"))
        .expect("the registry and filters must be configured");
    assert_eq!(
        registry.names().collect::<Vec<_>>(),
        vec!["conditional/bool-literal"]
    );
}

#[test]
fn configured_registry_reports_invalid_mutator_selection() {
    let mut configuration = Configuration::default();
    configuration
        .apply(&CommandSettings {
            enable_mutators: Some(vec!["missing/mutator".to_owned()]),
            ..CommandSettings::default()
        })
        .expect("the selector pattern must be valid");

    assert!(matches!(
        configured_registry(&configuration, None),
        Err(RegistryConfigurationError::MutatorSelection(_))
    ));
}

#[test]
fn configured_registry_reports_invalid_function_filter() {
    assert!(matches!(
        configured_registry(&Configuration::default(), Some("(")),
        Err(RegistryConfigurationError::SourceFilters(_))
    ));
}
