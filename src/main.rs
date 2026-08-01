use std::env;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

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
        Command::Run { targets, timeout } => run_mutation_tests(&targets, timeout),
        Command::Invalid(argument) => Ok(invalid_argument(argument)),
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
        "--list-files" => Command::ListFiles(arguments[1..].to_vec()),
        _ => parse_run(arguments),
    }
}

fn print_help() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Mutation testing for Rust\n\nUsage:\n  mutarust [--timeout SECONDS] [TARGET]...\n  mutarust --list-files [TARGET]...\n  mutarust --list-mutators"
    )?;
    writeln!(
        stdout,
        "\nOptions:\n  -h, --help          Print help\n  -V, --version       Print version\n      --list-files    List selected Rust production source files\n      --list-mutators List available mutators\n      --timeout        Stop each test run after this many seconds"
    )
}

fn print_version() -> io::Result<()> {
    writeln!(io::stdout().lock(), "mutarust {}", mutarust::VERSION)
}

fn list_mutators() -> io::Result<ExitCode> {
    let mut stdout = io::stdout().lock();
    for name in mutarust::Registry::builtins().names() {
        writeln!(stdout, "{name}")?;
    }
    Ok(ExitCode::SUCCESS)
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
    let mut targets = Vec::new();
    let mut timeout = mutarust::DEFAULT_TEST_TIMEOUT;
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        if argument == "--timeout" {
            let Some(seconds) = arguments.get(index + 1) else {
                return Command::Invalid("--timeout requires a positive whole number".to_owned());
            };
            let Ok(seconds) = seconds.parse::<u64>() else {
                return Command::Invalid("--timeout requires a positive whole number".to_owned());
            };
            if seconds == 0 {
                return Command::Invalid("--timeout requires a positive whole number".to_owned());
            }
            timeout = Duration::from_secs(seconds);
            index += 2;
        } else if argument.starts_with('-') {
            return Command::Invalid(argument.clone());
        } else {
            targets.push(argument.clone());
            index += 1;
        }
    }
    Command::Run { targets, timeout }
}

fn run_mutation_tests(targets: &[String], timeout: Duration) -> io::Result<ExitCode> {
    match mutarust::run_mutation_tests_with_timeout(
        targets,
        &mutarust::Registry::builtins(),
        timeout,
    ) {
        Ok(run) => print_mutation_results(&run).map(|()| ExitCode::SUCCESS),
        Err(error) => source_error(&error.to_string()),
    }
}

fn print_mutation_results(run: &mutarust::MutationRun) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for result in run.results() {
        writeln!(
            stdout,
            "{} {} {}",
            result.state,
            result.source.display(),
            result.mutator
        )?;
        if let Some(error) = &result.error {
            writeln!(stdout, "  {error}")?;
        }
    }
    writeln!(stdout, "Killed: {}", run.killed())?;
    writeln!(stdout, "Escaped: {}", run.escaped())?;
    writeln!(stdout, "Errored: {}", run.errored())?;
    writeln!(stdout, "Not covered: {}", run.not_covered())?;
    writeln!(stdout, "Skipped: {}", run.skipped())
}

fn print_files(files: &[std::path::PathBuf]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();

    for file in files {
        writeln!(stdout, "{}", file.display())?;
    }

    Ok(())
}

fn invalid_argument(argument: String) -> ExitCode {
    write_error(&format!("unknown argument: {argument}"));
    ExitCode::from(3)
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
    Run {
        targets: Vec<String>,
        timeout: Duration,
    },
    Invalid(String),
}
