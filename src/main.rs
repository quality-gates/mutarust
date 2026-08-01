use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();

    run(parse_command(&arguments)).unwrap_or(ExitCode::FAILURE)
}

fn run(command: Command) -> io::Result<ExitCode> {
    match command {
        Command::Help => print_help().map(|()| ExitCode::SUCCESS),
        Command::Version => print_version().map(|()| ExitCode::SUCCESS),
        Command::ListFiles(targets) => list_files(&targets),
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
        "--list-files" => Command::ListFiles(arguments[1..].to_vec()),
        _ => Command::Invalid(first.clone()),
    }
}

fn print_help() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Mutation testing for Rust\n\nUsage: mutarust --list-files [TARGET]..."
    )?;
    writeln!(
        stdout,
        "\nOptions:\n  -h, --help        Print help\n  -V, --version     Print version\n      --list-files  List selected Rust production source files"
    )
}

fn print_version() -> io::Result<()> {
    writeln!(io::stdout().lock(), "mutarust {}", mutarust::VERSION)
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
    ListFiles(Vec<String>),
    Invalid(String),
}
