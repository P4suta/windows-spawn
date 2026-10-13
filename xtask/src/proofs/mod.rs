//! Formal-verification gates (ADR 0014): each runs a pinned verifier and accepts only a complete, expected result.

pub(crate) mod alloy;
pub(crate) mod kani;
pub(crate) mod lean;
pub(crate) mod prolog;
pub(crate) mod quint;
pub(crate) mod tla;
pub(crate) mod tools;
pub(crate) mod verus;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::tasks::{fail, Result};

/// Creates `parent/stem-N` for the first free `N`; `create_dir` fails atomically on a taken name.
pub(crate) fn fresh_directory(parent: &Path, stem: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(parent)?;
    let mut index = 0_usize;
    loop {
        let path = parent.join(format!("{stem}-{}-{index}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                index = index.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
}

/// Runs `command` to completion and returns what it printed, whatever its status.
pub(crate) fn observe(command: &mut Command) -> Result<Output> {
    println!("+ {command:?}");
    Ok(command.output()?)
}

/// Requires `output` to report success, and shows what it printed when it did not.
pub(crate) fn succeeded(what: &str, output: &Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    fail(format!(
        "{what} failed with {}:\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}
