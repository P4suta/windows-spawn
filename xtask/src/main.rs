//! Repository automation for windows-spawn.

mod cli;
mod tasks;

use std::env;
use std::process;

fn main() {
    let arguments = env::args().skip(1);
    let result = match cli::parse(arguments) {
        Ok(task) => tasks::execute(task),
        Err(error) => Err(tasks::TaskError::from(error)),
    };
    match result {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    }
}
