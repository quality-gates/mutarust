use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use mutarust::{
    Baseline, CommandSettings, Configuration, CoverageControls, DisplayFilter, ExecutionControls,
    GitDiffControls, Registry, ReportContext, TestExecution, WorkerLimit, github_annotations,
    write_agentic_report, write_compact_summary, write_full_report, write_gitlab_report,
    write_html_report,
};

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();

    if env::var_os("GO_FLAGS_COMPLETION").is_some() {
        return print_bash_completion(&arguments).map_or(ExitCode::FAILURE, |()| ExitCode::from(2));
    }

    run(parse_command(&arguments)).unwrap_or(ExitCode::FAILURE)
}

fn run(command: Command) -> io::Result<ExitCode> {
    match command {
        Command::Help => print_help().map(|()| ExitCode::SUCCESS),
        Command::Version => print_version().map(|()| ExitCode::SUCCESS),
        Command::ListMutators => list_mutators(),
        Command::ListFiles(targets) => list_files(&targets),
        Command::PrintAst(targets) => print_ast(&targets),
        Command::Run(command) => run_mutation_tests(*command),
        Command::Invalid(message) => source_error(&message),
    }
}

fn parse_command(arguments: &[String]) -> Command {
    let Some(first) = arguments.first() else {
        return Command::Help;
    };

    match first.as_str() {
        "--help" | "-h" if arguments.len() == 1 => Command::Help,
        "--version" | "-V" if arguments.len() == 1 => Command::Version,
        "--list-mutators" => parse_list_mutators(arguments.len()),
        "--list-files" => parse_target_mode(&arguments[1..], "--list-files", Command::ListFiles),
        "--print-ast" => parse_target_mode(&arguments[1..], "--print-ast", Command::PrintAst),
        _ => parse_run(arguments),
    }
}

fn parse_list_mutators(argument_count: usize) -> Command {
    if argument_count == 1 {
        Command::ListMutators
    } else {
        Command::Invalid(
            "the --list-mutators command does not accept configuration or mutation options"
                .to_owned(),
        )
    }
}

fn parse_target_mode(
    arguments: &[String],
    name: &str,
    build: fn(Vec<String>) -> Command,
) -> Command {
    if let Some(option) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Command::Invalid(format!(
            "the {name} command does not accept the {option} option"
        ));
    }
    build(arguments.to_vec())
}

fn print_help() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Mutation testing for Rust\n\nUsage:\n  mutarust [OPTIONS] [TARGET]...\n  mutarust --list-files [TARGET]...\n  mutarust --print-ast [TARGET]...\n  mutarust --list-mutators"
    )?;
    writeln!(
        stdout,
        "\nOptions:\n  -h, --help           Print help\n  -V, --version        Print version\n      --config FILE     Read mutation policy from a YAML file\n      --list-files      List selected Rust production source files\n      --print-ast       Print the parsed Rust syntax for selected sources\n      --list-mutators   List available mutators\n      --exec COMMAND    Run a custom command for each mutant\n      --exec-timeout    Stop each test command after this many seconds\n      --timeout         Alias for --exec-timeout\n      --timeout-coefficient FACTOR  Set an adaptive Cargo timeout\n      --test-flags FLAGS  Add shell-quoted Cargo test flags\n      --test-recursive  Select all Cargo workspace packages\n      --workers COUNT   Run this many Cargo mutation jobs\n      --dry-run         List mutants without writing files or running tests\n      --no-exec         Write mutants without running tests\n      --do-not-remove-tmp-folder  Keep mutation workspaces\n      --match REGEXP    Mutate only functions with matching names\n      --verbose         Print mutation location and worker count\n      --debug           Print verbose details plus mutator and test command\n      --silent          Hide mutant status output\n      --no-silent       Print mutant status output\n      --quiet           Show only escaped mutants\n      --output-statuses LETTERS  Show only these states: k e s n x\n      --no-diffs        Hide escaped-mutant source diffs\n      --logger-summary-json  Write compact scores to mutarust-summary.json\n      --logger-agentic-json  Write escaped mutants to mutarust-agentic.json\n      --logger-github   Emit escaped mutants as GitHub Actions warnings\n      --logger-gitlab   Write mutarust-gitlab.json Code Quality findings\n      --blacklist FILE  Read accepted mutation checksums\n      --baseline FILE   Read escaped-mutant IDs; default mutarust-baseline.json\n      --update-baseline Write current escaped-mutant IDs and exit\n      --fail-on-escaped Fail only for escaped IDs outside the baseline\n      --run-mutant-id ID  Run one mutant without score gates\n      --min-msi         Set the minimum mutation score percentage\n      --min-covered-msi Set the minimum covered-code score percentage\n      --enable NAME     Select a mutator name or group pattern\n      --disable NAME    Disable a mutator name or group pattern"
    )?;
    writeln!(
        stdout,
        "      --coverage        Collect LLVM line coverage before mutation\n      --per-test        Run mapped tests for each covered mutant\n      --git-diff-lines  Mutate Git changed lines only\n      --git-diff-base REF  Set Git base; default origin/HEAD, then master"
    )
}

fn print_bash_completion(arguments: &[String]) -> io::Result<()> {
    let prefix = arguments.last().map(String::as_str).unwrap_or("");
    let mut stdout = io::stdout().lock();
    for candidate in bash_completion_candidates() {
        if candidate.starts_with(prefix) {
            writeln!(stdout, "{candidate}")?;
        }
    }
    Ok(())
}

fn bash_completion_candidates() -> &'static [&'static str] {
    &[
        "-h",
        "--help",
        "-V",
        "--version",
        "--config",
        "--list-files",
        "--print-ast",
        "--list-mutators",
        "--exec",
        "--exec-timeout",
        "--timeout",
        "--timeout-coefficient",
        "--test-flags",
        "--test-recursive",
        "--workers",
        "--dry-run",
        "--no-exec",
        "--do-not-remove-tmp-folder",
        "--match",
        "--verbose",
        "--debug",
        "--silent",
        "--no-silent",
        "--no-diffs",
        "--logger-summary-json",
        "--logger-agentic-json",
        "--logger-github",
        "--logger-gitlab",
        "--blacklist",
        "--baseline",
        "--update-baseline",
        "--fail-on-escaped",
        "--run-mutant-id",
        "--min-msi",
        "--min-covered-msi",
        "--enable",
        "--disable",
        "--coverage",
        "--per-test",
        "--git-diff-lines",
        "--git-diff-base",
        "[TARGET]...",
    ]
}

fn print_version() -> io::Result<()> {
    writeln!(io::stdout().lock(), "mutarust {}", mutarust::VERSION)
}

fn list_mutators() -> io::Result<ExitCode> {
    let mut stdout = io::stdout().lock();
    for name in Registry::builtins().names() {
        writeln!(stdout, "{name}")?;
    }
    Ok(ExitCode::SUCCESS)
}

fn list_files(targets: &[String]) -> io::Result<ExitCode> {
    with_selected_sources(targets, print_files)
}

fn print_ast(targets: &[String]) -> io::Result<ExitCode> {
    with_selected_sources(targets, print_syntax_trees)
}

fn with_selected_sources(
    targets: &[String],
    then: impl FnOnce(&[PathBuf]) -> Result<(), String>,
) -> io::Result<ExitCode> {
    match mutarust::find_rust_sources(targets) {
        Ok(files) if files.is_empty() => {
            source_error("could not find any suitable Rust source files")
        }
        Ok(files) => match then(&files) {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(message) => source_error(&message),
        },
        Err(error) => source_error(&error.to_string()),
    }
}

fn print_syntax_trees(files: &[PathBuf]) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    for file in files {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("could not open file {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&text)
            .map_err(|error| format!("could not parse file {}: {error}", file.display()))?;
        writeln!(stdout, "{}", file.display()).map_err(|error| error.to_string())?;
        writeln!(stdout, "{syntax:#?}").map_err(|error| error.to_string())?;
        writeln!(stdout).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn parse_run(arguments: &[String]) -> Command {
    let mut command = RunCommand::default();
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if !argument.starts_with('-') {
            command.targets.push(argument.clone());
            index += 1;
            continue;
        }
        match parse_run_argument(&mut command, argument, arguments.get(index + 1)) {
            Ok(consumed) => index += consumed,
            Err(message) => return Command::Invalid(message),
        }
    }
    validate_execution_options(&command)
        .map_or_else(Command::Invalid, |_| Command::Run(Box::new(command)))
}

fn parse_run_argument(
    command: &mut RunCommand,
    argument: &str,
    next: Option<&String>,
) -> Result<usize, String> {
    if let Some(result) = parse_value_option(command, argument, next) {
        return result.map(|()| 2);
    }
    parse_switch_option(command, argument).map(|()| 1)
}

fn parse_value_option(
    command: &mut RunCommand,
    argument: &str,
    next: Option<&String>,
) -> Option<Result<(), String>> {
    let value = || required_value(next, argument);
    Some(match argument {
        "--timeout" | "--exec-timeout" => {
            value().and_then(|value| set_timeout(command, value, argument))
        }
        "--timeout-coefficient" => {
            value().and_then(|value| set_timeout_coefficient(command, value))
        }
        "--workers" => value().and_then(|value| set_workers(command, value)),
        "--test-flags" => value().and_then(|value| set_test_flags(command, value)),
        "--exec" => value().and_then(|value| set_custom_command(command, value)),
        "--git-diff-base" => value().and_then(|value| set_git_diff_base(command, value)),
        "--baseline" => value().and_then(|value| set_baseline_path(command, value)),
        "--blacklist" => value().map(|value| add_blacklist(command, value)),
        _ => {
            return parse_display_value_option(command, argument, next)
                .or_else(|| parse_policy_value_option(command, argument, next));
        }
    })
}

fn parse_display_value_option(
    command: &mut RunCommand,
    argument: &str,
    next: Option<&String>,
) -> Option<Result<(), String>> {
    let value = || required_value(next, argument);
    Some(match argument {
        "--output-statuses" => value().and_then(|value| set_output_statuses(command, value)),
        _ => return None,
    })
}

fn parse_policy_value_option(
    command: &mut RunCommand,
    argument: &str,
    next: Option<&String>,
) -> Option<Result<(), String>> {
    let value = || required_value(next, argument);
    Some(match argument {
        "--match" => value().and_then(|value| set_function_match(command, value)),
        "--config" => value().and_then(|value| set_config(command, value)),
        "--min-msi" => {
            value().and_then(|value| set_score(&mut command.settings.min_msi, value, argument))
        }
        "--min-covered-msi" => value()
            .and_then(|value| set_score(&mut command.settings.min_covered_msi, value, argument)),
        "--run-mutant-id" => value().and_then(|value| set_run_mutant_id(command, value)),
        "--enable" => value().map(|value| add_enabled(command, value)),
        "--disable" => value().map(|value| command.settings.disable_mutators.push(value)),
        _ => return None,
    })
}

fn parse_switch_option(command: &mut RunCommand, argument: &str) -> Result<(), String> {
    if let Some(result) = parse_execution_switch(command, argument) {
        return result;
    }
    if let Some(result) = parse_baseline_switch(command, argument) {
        return result;
    }
    if let Some(result) = parse_display_switch(command, argument) {
        return result;
    }
    match argument {
        "--test-recursive" => {
            command.recursive_tests = true;
            Ok(())
        }
        "--verbose" => {
            command.output.verbose = true;
            Ok(())
        }
        "--debug" => {
            command.output.debug = true;
            Ok(())
        }
        "--help" | "-h" | "--version" | "-V" | "--list-files" | "--print-ast"
        | "--list-mutators" => Err(format!("cannot use {argument} with mutation options")),
        _ => Err(format!("unknown argument: {argument}")),
    }
}

fn parse_display_switch(command: &mut RunCommand, argument: &str) -> Option<Result<(), String>> {
    Some(match argument {
        "--silent" => set_silent(command, true),
        "--no-silent" => set_silent(command, false),
        "--no-diffs" => {
            command.output.no_diffs = true;
            Ok(())
        }
        "--logger-summary-json" => set_logger_summary_json(command),
        "--logger-agentic-json" => set_logger_agentic_json(command),
        "--logger-github" => set_logger_github(command),
        "--logger-gitlab" => set_logger_gitlab(command),
        "--quiet" => {
            command.output.quiet = true;
            Ok(())
        }
        _ => return None,
    })
}

fn set_logger_summary_json(command: &mut RunCommand) -> Result<(), String> {
    if command.logger_summary_json {
        Err("--logger-summary-json can be supplied only once".to_owned())
    } else {
        command.logger_summary_json = true;
        Ok(())
    }
}

fn set_logger_agentic_json(command: &mut RunCommand) -> Result<(), String> {
    if command.logger_agentic_json {
        Err("--logger-agentic-json can be supplied only once".to_owned())
    } else {
        command.logger_agentic_json = true;
        Ok(())
    }
}

fn set_logger_github(command: &mut RunCommand) -> Result<(), String> {
    if command.logger_github {
        Err("--logger-github can be supplied only once".to_owned())
    } else {
        command.logger_github = true;
        Ok(())
    }
}

fn set_logger_gitlab(command: &mut RunCommand) -> Result<(), String> {
    if command.logger_gitlab {
        Err("--logger-gitlab can be supplied only once".to_owned())
    } else {
        command.logger_gitlab = true;
        Ok(())
    }
}

fn parse_baseline_switch(command: &mut RunCommand, argument: &str) -> Option<Result<(), String>> {
    Some(match argument {
        "--update-baseline" => set_update_baseline(command),
        "--fail-on-escaped" => set_fail_on_escaped(command),
        _ => return None,
    })
}

fn parse_execution_switch(command: &mut RunCommand, argument: &str) -> Option<Result<(), String>> {
    Some(match argument {
        "--dry-run" => set_dry_run(command),
        "--no-exec" => set_no_exec(command),
        "--do-not-remove-tmp-folder" => set_keep_temporary(command),
        "--coverage" => set_coverage(command),
        "--per-test" => set_per_test_coverage(command),
        "--git-diff-lines" => set_git_diff_lines(command),
        _ => return None,
    })
}

fn set_dry_run(command: &mut RunCommand) -> Result<(), String> {
    command.execution.dry_run = true;
    Ok(())
}

fn set_git_diff_lines(command: &mut RunCommand) -> Result<(), String> {
    if command.execution.git_diff_lines {
        Err("--git-diff-lines can only be used once".to_owned())
    } else {
        command.execution.git_diff_lines = true;
        Ok(())
    }
}

fn set_git_diff_base(command: &mut RunCommand, base: String) -> Result<(), String> {
    if command.execution.git_diff_base.is_some() {
        Err("--git-diff-base can only be used once".to_owned())
    } else {
        command.execution.git_diff_base = Some(base);
        Ok(())
    }
}

fn set_no_exec(command: &mut RunCommand) -> Result<(), String> {
    command.execution.no_exec = true;
    Ok(())
}

fn set_keep_temporary(command: &mut RunCommand) -> Result<(), String> {
    command.execution.keep_temporary = true;
    Ok(())
}

fn set_coverage(command: &mut RunCommand) -> Result<(), String> {
    if command.execution.coverage {
        Err("--coverage can be supplied only once".to_owned())
    } else {
        command.execution.coverage = true;
        Ok(())
    }
}

fn set_per_test_coverage(command: &mut RunCommand) -> Result<(), String> {
    if command.execution.per_test_coverage {
        Err("--per-test can be supplied only once".to_owned())
    } else {
        command.execution.per_test_coverage = true;
        Ok(())
    }
}

fn set_run_mutant_id(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.run_mutant_id.replace(value).is_some() {
        Err("--run-mutant-id can be supplied only once".to_owned())
    } else {
        Ok(())
    }
}

fn set_baseline_path(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command
        .baseline
        .path
        .replace(PathBuf::from(value))
        .is_some()
    {
        Err("--baseline can be supplied only once".to_owned())
    } else {
        Ok(())
    }
}

fn add_blacklist(command: &mut RunCommand, value: String) {
    command.execution.blacklist_files.push(PathBuf::from(value));
}

fn set_update_baseline(command: &mut RunCommand) -> Result<(), String> {
    if command.baseline.update {
        Err("--update-baseline can be supplied only once".to_owned())
    } else {
        command.baseline.update = true;
        Ok(())
    }
}

fn set_fail_on_escaped(command: &mut RunCommand) -> Result<(), String> {
    if command.baseline.fail_on_escaped {
        Err("--fail-on-escaped can be supplied only once".to_owned())
    } else {
        command.baseline.fail_on_escaped = true;
        Ok(())
    }
}

fn set_function_match(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.function_match.replace(value).is_some() {
        Err("--match can be supplied only once".to_owned())
    } else {
        Ok(())
    }
}

fn required_value(value: Option<&String>, option: &str) -> Result<String, String> {
    value
        .cloned()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn set_timeout(command: &mut RunCommand, value: String, option: &str) -> Result<(), String> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| format!("{option} requires a positive whole number"))?;
    if seconds == 0 {
        return Err(format!("{option} requires a positive whole number"));
    }
    command.timeout = Duration::from_secs(seconds);
    command.execution.fixed_timeout = true;
    Ok(())
}

fn set_timeout_coefficient(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.execution.timeout_coefficient.is_some() {
        return Err("--timeout-coefficient can be supplied only once".to_owned());
    }
    let coefficient = value
        .parse::<f64>()
        .map_err(|_| "--timeout-coefficient requires a positive number".to_owned())?;
    if !coefficient.is_finite() || coefficient <= 0.0 {
        return Err("--timeout-coefficient requires a positive number".to_owned());
    }
    command.execution.timeout_coefficient = Some(coefficient);
    Ok(())
}

fn set_workers(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.execution.workers.is_some() {
        return Err("--workers can be supplied only once".to_owned());
    }
    let workers = value
        .parse::<usize>()
        .ok()
        .and_then(WorkerLimit::new)
        .ok_or_else(|| "--workers requires a positive whole number".to_owned())?;
    command.execution.workers = Some(workers);
    Ok(())
}

fn set_test_flags(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.execution.cargo_flags.is_some() {
        return Err("--test-flags can be supplied only once".to_owned());
    }
    let flags = shell_words::split(&value)
        .map_err(|error| format!("could not parse --test-flags: {error}"))?;
    if flags.is_empty() {
        return Err("--test-flags requires at least one Cargo argument".to_owned());
    }
    command.execution.cargo_flags = Some(flags);
    Ok(())
}

fn set_custom_command(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.custom_command.replace(value).is_some() {
        Err("--exec can be supplied only once".to_owned())
    } else {
        Ok(())
    }
}

fn set_config(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.configuration.is_some() {
        return Err("--config can be supplied only once".to_owned());
    }
    command.configuration = Some(PathBuf::from(value));
    Ok(())
}

fn set_silent(command: &mut RunCommand, value: bool) -> Result<(), String> {
    if command.settings.silent_mode.replace(value).is_some() {
        return Err("use only one of --silent and --no-silent".to_owned());
    }
    Ok(())
}

fn set_output_statuses(command: &mut RunCommand, value: String) -> Result<(), String> {
    if command.output.output_statuses.is_some() {
        return Err("--output-statuses can be supplied only once".to_owned());
    }
    if value.is_empty() || !value.bytes().all(|letter| b"kesnx".contains(&letter)) {
        return Err(
            "--output-statuses requires only these letters: k (killed), e (escaped), \
             s (skipped), n (not covered), x (errored)"
                .to_owned(),
        );
    }
    command.output.output_statuses = Some(value);
    Ok(())
}

fn set_score(score: &mut Option<u8>, value: String, option: &str) -> Result<(), String> {
    let value = value
        .parse::<u8>()
        .map_err(|_| format!("{option} requires a whole percentage from 0 to 100"))?;
    if value > 100 {
        return Err(format!(
            "{option} requires a whole percentage from 0 to 100"
        ));
    }
    *score = Some(value);
    Ok(())
}

fn add_enabled(command: &mut RunCommand, value: String) {
    command
        .settings
        .enable_mutators
        .get_or_insert_default()
        .push(value);
}

fn run_mutation_tests(command: RunCommand) -> io::Result<ExitCode> {
    let (configuration, baseline, run) = match start_mutation_run(&command) {
        Ok(values) => values,
        Err(error) => return source_error(&error),
    };
    finish_mutation_run(&command, &configuration, &baseline, &run)
}

fn start_mutation_run(
    command: &RunCommand,
) -> Result<(Configuration, Baseline, mutarust::MutationRun), String> {
    let baseline = Baseline::load(command.baseline.path())?;
    let configuration = effective_configuration(command).map_err(|error| error.to_string())?;
    let (registry, filters) = configured_registry(command, &configuration)?;
    let execution = test_execution(command)?;
    let run = mutarust::run_mutation_tests_with_controls(
        &command.targets,
        &registry,
        command.timeout,
        command.run_mutant_id.as_deref(),
        &filters,
        &execution,
        &execution_controls(command, configuration.silent_mode),
    )
    .map_err(|error| error.to_string())?;
    Ok((configuration, baseline, run))
}

fn test_execution(command: &RunCommand) -> Result<TestExecution, String> {
    match &command.custom_command {
        Some(custom) => TestExecution::custom(
            custom,
            command.recursive_tests,
            command.output.verbose,
            command.output.debug,
        )
        .map_err(|error| error.to_string()),
        None => Ok(TestExecution::cargo_with_options(
            command.recursive_tests,
            command.execution.cargo_flags.clone().unwrap_or_default(),
            command.output.verbose,
            command.output.debug,
        )),
    }
}

fn execution_controls(command: &RunCommand, silent_mode: bool) -> ExecutionControls {
    ExecutionControls {
        dry_run: command.execution.dry_run,
        no_exec: command.execution.no_exec,
        keep_temporary: command.execution.keep_temporary,
        timeout_coefficient: command.execution.timeout_coefficient,
        workers: command.execution.workers.unwrap_or_default(),
        coverage: CoverageControls {
            enabled: command.execution.coverage,
            per_test: command.execution.per_test_coverage,
        },
        git_diff: GitDiffControls {
            enabled: command.execution.git_diff_lines,
            base: command.execution.git_diff_base.clone(),
        },
        blacklist_files: command.execution.blacklist_files.clone(),
        progress: progress_enabled(command, silent_mode),
        filter: display_filter(command, silent_mode),
    }
}

fn display_filter(command: &RunCommand, silent_mode: bool) -> DisplayFilter {
    DisplayFilter {
        silent: silent_mode,
        quiet: command.output.quiet,
        output_statuses: command.output.output_statuses.clone(),
    }
}

/// Decides whether to show a live progress line on standard error.
///
/// The progress line needs a real terminal, and it never runs alongside
/// verbose or debug diagnostics (which share standard output, not standard
/// error, but redraw at a similar cadence) or modes that print no results.
fn progress_enabled(command: &RunCommand, silent_mode: bool) -> bool {
    io::stderr().is_terminal() && progress_allowed(command, silent_mode)
}

/// The non-terminal conditions for [`progress_enabled`].
fn progress_allowed(command: &RunCommand, silent_mode: bool) -> bool {
    !command.output.verbose
        && !command.output.debug
        && !silent_mode
        && !command.execution.no_exec
        && !command.execution.dry_run
}

fn configured_registry(
    command: &RunCommand,
    configuration: &Configuration,
) -> Result<(Registry, mutarust::SourceFilters), String> {
    let mut registry = Registry::builtins();
    let names = registry.names().map(str::to_owned).collect::<Vec<_>>();
    let filters = mutarust::SourceFilters::new(
        &configuration.exclude_dirs,
        &configuration.ignore_source_lines,
        command.function_match.as_deref(),
        &names,
    )?;
    let selected = configuration
        .select_mutators(&names)
        .map_err(|error| selection_error(command.configuration.as_deref(), &error))?;
    registry.retain(|name| selected.iter().any(|selected_name| selected_name == name));
    Ok((registry, filters))
}

fn finish_mutation_run(
    command: &RunCommand,
    configuration: &Configuration,
    baseline: &Baseline,
    run: &mutarust::MutationRun,
) -> io::Result<ExitCode> {
    if command.baseline.update {
        return write_baseline(command.baseline.path(), run);
    }
    if command.execution.dry_run {
        print_dry_run(run)?;
        return Ok(write_reports_or_error(command, configuration, run).unwrap_or(ExitCode::SUCCESS));
    }
    if command.execution.no_exec {
        print_generated_mutants(run)?;
        return Ok(write_reports_or_error(command, configuration, run).unwrap_or(ExitCode::SUCCESS));
    }
    let one_mutant = command.run_mutant_id.is_some();
    print_mutation_results(
        run,
        display_filter(command, configuration.silent_mode),
        command.output.no_diffs,
        one_mutant,
    )?;
    if let Some(code) = write_reports_or_error(command, configuration, run) {
        return Ok(code);
    }
    Ok(if one_mutant {
        ExitCode::SUCCESS
    } else {
        score_gates(
            run,
            configuration,
            baseline,
            command.baseline.fail_on_escaped,
        )
    })
}

fn write_reports_or_error(
    command: &RunCommand,
    configuration: &Configuration,
    run: &mutarust::MutationRun,
) -> Option<ExitCode> {
    let context = ReportContext {
        one_mutant: command.run_mutant_id.is_some(),
    };
    write_report_if(configuration.json_output, || write_full_report(run, &context))
        .or_else(|| write_report_if(configuration.html_output, || write_html_report(run)))
        .or_else(|| write_report_if(command.logger_summary_json, || write_compact_summary(run)))
        .or_else(|| {
            write_report_if(command.logger_agentic_json, || {
                write_agentic_report(run, std::path::Path::new("."))
            })
        })
        .or_else(|| write_github_annotations_if(command.logger_github, run))
        .or_else(|| write_report_if(command.logger_gitlab, || write_gitlab_report(run)))
}

fn write_report_if(
    enabled: bool,
    write: impl FnOnce() -> Result<(), String>,
) -> Option<ExitCode> {
    if !enabled {
        return None;
    }
    match write() {
        Ok(()) => None,
        Err(error) => {
            write_error(&error);
            Some(ExitCode::from(3))
        }
    }
}

fn write_github_annotations_if(enabled: bool, run: &mutarust::MutationRun) -> Option<ExitCode> {
    if !enabled {
        return None;
    }
    let annotations = github_annotations(run);
    if annotations.is_empty() {
        return None;
    }
    match write!(io::stdout().lock(), "{annotations}") {
        Ok(()) => None,
        Err(error) => {
            write_error(&error.to_string());
            Some(ExitCode::from(3))
        }
    }
}

fn write_baseline(path: &std::path::Path, run: &mutarust::MutationRun) -> io::Result<ExitCode> {
    match Baseline::write(path, run) {
        Ok(count) => {
            writeln!(
                io::stdout().lock(),
                "Baseline written to \"{}\" ({} escaped mutant(s))",
                path.display(),
                count
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => source_error(&error),
    }
}

fn print_dry_run(run: &mutarust::MutationRun) -> io::Result<ExitCode> {
    writeln!(
        io::stdout().lock(),
        "Total: {} mutation(s) would be generated. No files written, no tests run.",
        run.total()
    )?;
    Ok(ExitCode::SUCCESS)
}

fn print_generated_mutants(run: &mutarust::MutationRun) -> io::Result<ExitCode> {
    let mut stdout = io::stdout().lock();
    for result in run.results() {
        print_result_details(&mut stdout, result)?;
    }
    writeln!(stdout, "Generated: {}", run.total())?;
    writeln!(
        stdout,
        "No tests run. Generated mutations are in the mutation areas above."
    )?;
    Ok(ExitCode::SUCCESS)
}

fn selection_error(path: Option<&std::path::Path>, error: &mutarust::ConfigurationError) -> String {
    path.map_or_else(
        || error.to_string(),
        |path| format!("configuration {}: {error}", path.display()),
    )
}

fn effective_configuration(
    command: &RunCommand,
) -> Result<Configuration, mutarust::ConfigurationError> {
    let mut configuration = match &command.configuration {
        Some(path) => Configuration::read(path)?,
        None => Configuration::default(),
    };
    configuration.apply(&command.settings)?;
    Ok(configuration)
}

fn print_mutation_results(
    run: &mutarust::MutationRun,
    filter: DisplayFilter,
    no_diffs: bool,
    one_mutant: bool,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for result in run.results() {
        if !filter.shows(result.state) {
            continue;
        }
        print_result_details(&mut stdout, result)?;
        if result.state == mutarust::MutationState::Escaped && !no_diffs {
            write!(stdout, "{}", result.diff)?;
        }
    }
    if one_mutant {
        return Ok(());
    }
    writeln!(stdout, "Killed: {}", run.killed())?;
    writeln!(stdout, "Escaped: {}", run.escaped())?;
    writeln!(stdout, "Errored: {}", run.errored())?;
    writeln!(stdout, "Not covered: {}", run.not_covered())?;
    writeln!(stdout, "Skipped: {}", run.skipped())?;
    writeln!(stdout, "Total: {}", run.total())?;
    writeln!(
        stdout,
        "Mutation score: {:.2}%",
        run.mutation_score() * 100.0
    )?;
    if run.has_coverage() {
        writeln!(
            stdout,
            "Covered-code mutation score: {:.2}%",
            run.covered_mutation_score() * 100.0
        )?;
    }
    writeln!(stdout, "Per-mutator results:")?;
    writeln!(stdout, "Mutator | Killed | Escaped | Skipped | Total")?;
    for summary in run.mutator_summaries() {
        writeln!(
            stdout,
            "{} | {} | {} | {} | {}",
            summary.mutator, summary.killed, summary.escaped, summary.skipped, summary.total
        )?;
    }
    Ok(())
}

fn print_result_details(
    output: &mut impl Write,
    result: &mutarust::MutationResult,
) -> io::Result<()> {
    writeln!(
        output,
        "{} {} {}",
        result.state,
        result.source.display(),
        result.mutator
    )?;
    writeln!(output, "  ID: {}", result.stable_id)?;
    if let Some(detail) = &result.error {
        writeln!(output, "  {detail}")?;
    }
    Ok(())
}

fn total_score_gate(run: &mutarust::MutationRun, minimum: Option<u8>) -> ExitCode {
    let Some(minimum) = minimum else {
        return ExitCode::SUCCESS;
    };
    let score = run.mutation_score() * 100.0;
    if score < f64::from(minimum) {
        write_error(&format!(
            "mutation score {score:.2}% is below the required {}%",
            minimum
        ));
        ExitCode::from(4)
    } else {
        ExitCode::SUCCESS
    }
}

fn score_gates(
    run: &mutarust::MutationRun,
    configuration: &Configuration,
    baseline: &Baseline,
    fail_on_escaped: bool,
) -> ExitCode {
    let total = total_score_gate(run, configuration.min_msi);
    if total != ExitCode::SUCCESS {
        return total;
    }
    let covered = covered_score_gate(run, configuration.min_covered_msi);
    if covered != ExitCode::SUCCESS {
        return covered;
    }
    escaped_mutant_gate(run, baseline, fail_on_escaped)
}

fn escaped_mutant_gate(
    run: &mutarust::MutationRun,
    baseline: &Baseline,
    fail_on_escaped: bool,
) -> ExitCode {
    if !fail_on_escaped {
        return ExitCode::SUCCESS;
    }
    let count = baseline.new_escaped_count(run);
    if count == 0 {
        ExitCode::SUCCESS
    } else {
        write_error(&format!(
            "{count} new mutant(s) escaped — kill them or run --update-baseline to accept"
        ));
        ExitCode::from(4)
    }
}

fn covered_score_gate(run: &mutarust::MutationRun, minimum: Option<u8>) -> ExitCode {
    let Some(minimum) = minimum else {
        return ExitCode::SUCCESS;
    };
    if minimum == 0 {
        return ExitCode::SUCCESS;
    }
    if !run.has_coverage() {
        write_error("covered-code mutation score requires --coverage");
        return ExitCode::from(4);
    }
    let score = run.covered_mutation_score() * 100.0;
    if score < f64::from(minimum) {
        write_error(&format!(
            "covered-code mutation score {score:.2}% is below the required {}%",
            minimum
        ));
        ExitCode::from(4)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_files(files: &[std::path::PathBuf]) -> Result<(), String> {
    let mut stdout = io::stdout().lock();

    for file in files {
        writeln!(stdout, "{}", file.display()).map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn source_error(message: &str) -> io::Result<ExitCode> {
    write_error(message);
    Ok(ExitCode::from(3))
}

fn write_error(message: &str) {
    let _ = writeln!(io::stderr().lock(), "mutarust: {message}");
}

enum Command {
    Help,
    Version,
    ListMutators,
    ListFiles(Vec<String>),
    PrintAst(Vec<String>),
    Run(Box<RunCommand>),
    Invalid(String),
}

struct RunCommand {
    targets: Vec<String>,
    timeout: Duration,
    custom_command: Option<String>,
    recursive_tests: bool,
    execution: ExecutionOptions,
    output: OutputOptions,
    function_match: Option<String>,
    configuration: Option<PathBuf>,
    logger_summary_json: bool,
    logger_agentic_json: bool,
    logger_github: bool,
    logger_gitlab: bool,
    run_mutant_id: Option<String>,
    baseline: BaselineOptions,
    settings: CommandSettings,
}

#[derive(Default)]
struct OutputOptions {
    verbose: bool,
    debug: bool,
    quiet: bool,
    no_diffs: bool,
    output_statuses: Option<String>,
}

#[derive(Default)]
struct BaselineOptions {
    path: Option<PathBuf>,
    update: bool,
    fail_on_escaped: bool,
}

impl BaselineOptions {
    fn path(&self) -> &std::path::Path {
        self.path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("mutarust-baseline.json"))
    }
}

#[derive(Default)]
struct ExecutionOptions {
    dry_run: bool,
    no_exec: bool,
    keep_temporary: bool,
    fixed_timeout: bool,
    timeout_coefficient: Option<f64>,
    workers: Option<WorkerLimit>,
    cargo_flags: Option<Vec<String>>,
    coverage: bool,
    per_test_coverage: bool,
    git_diff_lines: bool,
    git_diff_base: Option<String>,
    blacklist_files: Vec<PathBuf>,
}

impl Default for RunCommand {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            timeout: mutarust::DEFAULT_TEST_TIMEOUT,
            custom_command: None,
            recursive_tests: false,
            execution: ExecutionOptions::default(),
            output: OutputOptions::default(),
            function_match: None,
            configuration: None,
            logger_summary_json: false,
            logger_agentic_json: false,
            logger_github: false,
            logger_gitlab: false,
            run_mutant_id: None,
            baseline: BaselineOptions::default(),
            settings: CommandSettings::default(),
        }
    }
}

fn validate_execution_options(command: &RunCommand) -> Result<(), String> {
    validation_error(command).map_or(Ok(()), |message| Err(message.to_owned()))
}

fn validation_error(command: &RunCommand) -> Option<&'static str> {
    run_mode_error(command)
        .or_else(|| dry_run_control_error(command))
        .or_else(|| cargo_control_error(command))
        .or_else(|| coverage_control_error(command))
        .or_else(|| git_diff_control_error(command))
        .or_else(|| baseline_control_error(command))
}

fn dry_run_control_error(command: &RunCommand) -> Option<&'static str> {
    let execution = &command.execution;
    [
        (
            execution.dry_run && execution.keep_temporary,
            "--dry-run cannot be used with --do-not-remove-tmp-folder",
        ),
        (
            execution.dry_run && execution.fixed_timeout,
            "--dry-run cannot be used with --timeout",
        ),
        (
            execution.dry_run && execution.timeout_coefficient.is_some(),
            "--dry-run cannot be used with --timeout-coefficient",
        ),
        (
            execution.dry_run && execution.cargo_flags.is_some(),
            "--dry-run cannot be used with --test-flags",
        ),
        (
            execution.dry_run && execution.workers.is_some(),
            "--dry-run cannot be used with --workers",
        ),
        (
            execution.dry_run && command.recursive_tests,
            "--dry-run cannot be used with --test-recursive",
        ),
        (
            execution.dry_run && execution.coverage,
            "--dry-run cannot be used with --coverage",
        ),
        (
            execution.dry_run && execution.per_test_coverage,
            "--dry-run cannot be used with --per-test",
        ),
    ]
    .into_iter()
    .find_map(|(invalid, message)| invalid.then_some(message))
}

fn run_mode_error(command: &RunCommand) -> Option<&'static str> {
    let execution = &command.execution;
    [
        (
            execution.dry_run && execution.no_exec,
            "--dry-run and --no-exec cannot be used together",
        ),
        (
            execution.dry_run && command.custom_command.is_some(),
            "--dry-run cannot be used with --exec",
        ),
        (
            execution.no_exec && command.custom_command.is_some(),
            "--no-exec cannot be used with --exec",
        ),
    ]
    .into_iter()
    .find_map(|(invalid, message)| invalid.then_some(message))
}

fn cargo_control_error(command: &RunCommand) -> Option<&'static str> {
    let execution = &command.execution;
    [
        (
            execution.timeout_coefficient.is_some() && execution.fixed_timeout,
            "--timeout-coefficient cannot be used with --timeout",
        ),
        (
            execution.timeout_coefficient.is_some() && command.custom_command.is_some(),
            "--timeout-coefficient requires the Cargo test command",
        ),
        (
            execution.timeout_coefficient.is_some() && execution.no_exec,
            "--timeout-coefficient cannot be used with --no-exec",
        ),
        (
            execution.no_exec && execution.fixed_timeout,
            "--no-exec cannot be used with --timeout",
        ),
        (
            execution.cargo_flags.is_some() && command.custom_command.is_some(),
            "--test-flags cannot be used with --exec",
        ),
        (
            execution.cargo_flags.is_some() && execution.no_exec,
            "--test-flags cannot be used with --no-exec",
        ),
        (
            command.recursive_tests && execution.no_exec,
            "--test-recursive cannot be used with --no-exec",
        ),
    ]
    .into_iter()
    .find_map(|(invalid, message)| invalid.then_some(message))
}

fn coverage_control_error(command: &RunCommand) -> Option<&'static str> {
    let execution = &command.execution;
    [
        (
            execution.coverage && command.custom_command.is_some(),
            "--coverage requires the Cargo test command",
        ),
        (
            execution.per_test_coverage && command.custom_command.is_some(),
            "--per-test requires the Cargo test command",
        ),
        (
            execution.coverage && execution.no_exec,
            "--coverage cannot be used with --no-exec",
        ),
        (
            execution.per_test_coverage && execution.no_exec,
            "--per-test cannot be used with --no-exec",
        ),
    ]
    .into_iter()
    .find_map(|(invalid, message)| invalid.then_some(message))
}

fn git_diff_control_error(command: &RunCommand) -> Option<&'static str> {
    (!command.execution.git_diff_lines && command.execution.git_diff_base.is_some())
        .then_some("--git-diff-base requires --git-diff-lines")
}

fn baseline_control_error(command: &RunCommand) -> Option<&'static str> {
    [
        (
            command.baseline.update && command.execution.dry_run,
            "--update-baseline cannot be used with --dry-run",
        ),
        (
            command.baseline.update && command.execution.no_exec,
            "--update-baseline cannot be used with --no-exec",
        ),
        (
            command.baseline.update && command.run_mutant_id.is_some(),
            "--update-baseline cannot be used with --run-mutant-id",
        ),
    ]
    .into_iter()
    .find_map(|(invalid, message)| invalid.then_some(message))
}

#[cfg(test)]
mod tests {
    use super::{RunCommand, progress_allowed};

    #[test]
    fn progress_is_allowed_only_without_diagnostics_or_result_free_modes() {
        let command = RunCommand::default();
        assert!(progress_allowed(&command, false));
        assert!(!progress_allowed(&command, true), "silent must disable it");

        let mut verbose = RunCommand::default();
        verbose.output.verbose = true;
        assert!(
            !progress_allowed(&verbose, false),
            "verbose must disable it"
        );

        let mut debug = RunCommand::default();
        debug.output.debug = true;
        assert!(!progress_allowed(&debug, false), "debug must disable it");

        let mut no_exec = RunCommand::default();
        no_exec.execution.no_exec = true;
        assert!(
            !progress_allowed(&no_exec, false),
            "no-exec must disable it"
        );

        let mut dry_run = RunCommand::default();
        dry_run.execution.dry_run = true;
        assert!(
            !progress_allowed(&dry_run, false),
            "dry-run must disable it"
        );
    }
}
