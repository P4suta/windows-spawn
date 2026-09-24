//! Repository automation for windows-spawn.

mod cli;
mod gates;
mod tasks;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = env::args().skip(1);
    let result = match cli::parse(arguments) {
        Ok(task) => tasks::execute(task),
        Err(error) => Err(tasks::TaskError::from(error)),
    };
    match result {
        Ok(code) => u8::try_from(code).map_or(ExitCode::FAILURE, ExitCode::from),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
