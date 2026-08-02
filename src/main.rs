use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use mutarust::{
    CommandSettings, Configuration, CoverageControls, ExecutionControls, Registry, TestExecution,
    WorkerLimit,
};

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();

    run(parse_command(&arguments)).unwrap_or(ExitCode::FAILURE)
}

fn run(command: Command) -> io::Result<ExitCode> {
    match command {
        Command::Help => print_help().map(|()| ExitCode::SUCCESS),
        Command::Version => print_version().map(|()| ExitCode::SUCCESS),
        Command::ListMutators => list_mutators(),
        Command::ListFiles(targets) => list_files(&targets),
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
        "--list-mutators" if arguments.len() == 1 => Command::ListMutators,
        "--list-mutators" => Command::Invalid(
            "the --list-mutators command does not accept configuration or mutation options"
                .to_owned(),
        ),
        "--list-files" => parse_list_files(&arguments[1..]),
        _ => parse_run(arguments),
    }
}

fn print_help() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Mutation testing for Rust\n\nUsage:\n  mutarust [OPTIONS] [TARGET]...\n  mutarust --list-files [TARGET]...\n  mutarust --list-mutators"
    )?;
    writeln!(
        stdout,
        "\nOptions:\n  -h, --help           Print help\n  -V, --version        Print version\n      --config FILE     Read mutation policy from a YAML file\n      --list-files      List selected Rust production source files\n      --list-mutators   List available mutators\n      --exec COMMAND    Run a custom command for each mutant\n      --exec-timeout    Stop each test command after this many seconds\n      --timeout         Alias for --exec-timeout\n      --timeout-coefficient FACTOR  Set an adaptive Cargo timeout\n      --test-flags FLAGS  Add shell-quoted Cargo test flags\n      --test-recursive  Select all Cargo workspace packages\n      --workers COUNT   Run this many Cargo mutation jobs\n      --dry-run         List mutants without writing files or running tests\n      --no-exec         Write mutants without running tests\n      --do-not-remove-tmp-folder  Keep mutation workspaces\n      --match REGEXP    Mutate only functions with matching names\n      --verbose         Tell a custom command to produce verbose output\n      --debug           Tell a custom command to produce debug output\n      --silent          Hide mutant status output\n      --no-silent       Print mutant status output\n      --no-diffs        Hide escaped-mutant source diffs\n      --run-mutant-id ID  Run one mutant without score gates\n      --min-msi         Set the minimum mutation score percentage\n      --min-covered-msi Set the minimum covered-code score percentage\n      --enable NAME     Select a mutator name or group pattern\n      --disable NAME    Disable a mutator name or group pattern"
    )?;
    writeln!(
        stdout,
        "      --coverage        Collect LLVM line coverage before mutation\n      --per-test        Run mapped tests for each covered mutant"
    )
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

fn parse_list_files(arguments: &[String]) -> Command {
    if let Some(option) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Command::Invalid(format!(
            "the --list-files command does not accept the {option} option"
        ));
    }
    Command::ListFiles(arguments.to_vec())
}

fn list_files(targets: &[String]) -> io::Result<ExitCode> {
    match mutarust::find_rust_sources(targets) {
        Ok(files) if files.is_empty() => {
            source_error("could not find any suitable Rust source files")
        }
        Ok(files) => print_files(&files).map(|()| ExitCode::SUCCESS),
        Err(error) => source_error(&error.to_string()),
    }
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
        _ => return parse_policy_value_option(command, argument, next),
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
    match argument {
        "--silent" => set_silent(command, true),
        "--no-silent" => set_silent(command, false),
        "--no-diffs" => {
            command.no_diffs = true;
            Ok(())
        }
        "--test-recursive" => {
            command.recursive_tests = true;
            Ok(())
        }
        "--verbose" => {
            command.verbose = true;
            Ok(())
        }
        "--debug" => {
            command.debug = true;
            Ok(())
        }
        "--help" | "-h" | "--version" | "-V" | "--list-files" | "--list-mutators" => {
            Err(format!("cannot use {argument} with mutation options"))
        }
        _ => Err(format!("unknown argument: {argument}")),
    }
}

fn parse_execution_switch(command: &mut RunCommand, argument: &str) -> Option<Result<(), String>> {
    Some(match argument {
        "--dry-run" => set_dry_run(command),
        "--no-exec" => set_no_exec(command),
        "--do-not-remove-tmp-folder" => set_keep_temporary(command),
        "--coverage" => set_coverage(command),
        "--per-test" => set_per_test_coverage(command),
        _ => return None,
    })
}

fn set_dry_run(command: &mut RunCommand) -> Result<(), String> {
    command.execution.dry_run = true;
    Ok(())
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
    let configuration = match effective_configuration(&command) {
        Ok(configuration) => configuration,
        Err(error) => return source_error(&error.to_string()),
    };
    let (registry, filters) = match configured_registry(&command, &configuration) {
        Ok(values) => values,
        Err(error) => return source_error(&error),
    };
    let execution = match test_execution(&command) {
        Ok(execution) => execution,
        Err(error) => return source_error(&error),
    };
    let run = match mutarust::run_mutation_tests_with_controls(
        &command.targets,
        &registry,
        command.timeout,
        command.run_mutant_id.as_deref(),
        &filters,
        &execution,
        &execution_controls(&command),
    ) {
        Ok(run) => run,
        Err(error) => return source_error(&error.to_string()),
    };
    finish_mutation_run(&command, &configuration, &run)
}

fn test_execution(command: &RunCommand) -> Result<TestExecution, String> {
    match &command.custom_command {
        Some(custom) => TestExecution::custom(
            custom,
            command.recursive_tests,
            command.verbose,
            command.debug,
        )
        .map_err(|error| error.to_string()),
        None => Ok(TestExecution::cargo_with_options(
            command.recursive_tests,
            command.execution.cargo_flags.clone().unwrap_or_default(),
        )),
    }
}

fn execution_controls(command: &RunCommand) -> ExecutionControls {
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
    }
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
    run: &mutarust::MutationRun,
) -> io::Result<ExitCode> {
    if command.execution.dry_run {
        return print_dry_run(run);
    }
    if command.execution.no_exec {
        return print_generated_mutants(run);
    }
    let one_mutant = command.run_mutant_id.is_some();
    print_mutation_results(run, configuration.silent_mode, command.no_diffs, one_mutant)?;
    Ok(if one_mutant {
        ExitCode::SUCCESS
    } else {
        score_gates(run, configuration)
    })
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
    silent: bool,
    no_diffs: bool,
    one_mutant: bool,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    if !silent {
        for result in run.results() {
            print_result_details(&mut stdout, result)?;
            if result.state == mutarust::MutationState::Escaped && !no_diffs {
                write!(stdout, "{}", result.diff)?;
            }
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

fn score_gates(run: &mutarust::MutationRun, configuration: &Configuration) -> ExitCode {
    let total = total_score_gate(run, configuration.min_msi);
    if total != ExitCode::SUCCESS {
        return total;
    }
    covered_score_gate(run, configuration.min_covered_msi)
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

fn print_files(files: &[std::path::PathBuf]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();

    for file in files {
        writeln!(stdout, "{}", file.display())?;
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
    Run(Box<RunCommand>),
    Invalid(String),
}

struct RunCommand {
    targets: Vec<String>,
    timeout: Duration,
    custom_command: Option<String>,
    recursive_tests: bool,
    verbose: bool,
    debug: bool,
    execution: ExecutionOptions,
    function_match: Option<String>,
    configuration: Option<PathBuf>,
    no_diffs: bool,
    run_mutant_id: Option<String>,
    settings: CommandSettings,
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
}

impl Default for RunCommand {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            timeout: mutarust::DEFAULT_TEST_TIMEOUT,
            custom_command: None,
            recursive_tests: false,
            verbose: false,
            debug: false,
            execution: ExecutionOptions::default(),
            function_match: None,
            configuration: None,
            no_diffs: false,
            run_mutant_id: None,
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
