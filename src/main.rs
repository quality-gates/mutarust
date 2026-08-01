use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();

    run(parse_command(&arguments)).unwrap_or(ExitCode::FAILURE)
}

fn run(command: Command<'_>) -> io::Result<ExitCode> {
    match command {
        Command::Help => print_help().map(|()| ExitCode::SUCCESS),
        Command::Version => print_version().map(|()| ExitCode::SUCCESS),
        Command::Invalid(argument) => Ok(invalid_argument(argument)),
    }
}

fn parse_command(arguments: &[String]) -> Command<'_> {
    let Some(first) = arguments.first() else {
        return Command::Help;
    };

    if arguments.len() > 1 {
        return Command::Invalid(first);
    }

    match first.as_str() {
        "--help" | "-h" => Command::Help,
        "--version" | "-V" => Command::Version,
        _ => Command::Invalid(first),
    }
}

fn print_help() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Mutation testing for Rust\n\nUsage: mutarust [OPTIONS] <TARGET>..."
    )?;
    writeln!(
        stdout,
        "\nOptions:\n  -h, --help     Print help\n  -V, --version  Print version"
    )
}

fn print_version() -> io::Result<()> {
    writeln!(io::stdout().lock(), "mutarust {}", mutarust::VERSION)
}

fn invalid_argument(argument: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "mutarust: unknown argument: {argument}"
    );
    ExitCode::from(3)
}

enum Command<'a> {
    Help,
    Version,
    Invalid(&'a str),
}
