//! The Prolog layer: the documented executable search as relations, its tests, and the vectors the crate's Windows test reads.

use std::fs;
use std::path::Path;
use std::process::Command;

use super::{fresh_directory, observe, succeeded};
use crate::tasks::{fail, Result};

/// The vectors the oracle writes, which the repository holds.
const VECTORS: &str = "executable_search.tsv";

fn swipl(package: &Path) -> Command {
    let mut command = Command::new("swipl");
    command.current_dir(package);
    command
}

/// Runs the specification's tests, and requires the oracle to tell every mistake apart and to write exactly the held vectors.
pub(crate) fn prolog(root: &Path) -> Result<()> {
    let package = root.join("proofs/prolog");
    let tests =
        observe(swipl(&package).args(["-g", "run_tests", "-t", "halt", "executable_search.pl"]))?;
    succeeded("the Prolog tests", &tests)?;
    let report = String::from_utf8_lossy(&tests.stderr);
    if !report.contains("tests passed") {
        return fail(format!("the Prolog tests reported no pass:\n{report}"));
    }

    let written = fresh_directory(&root.join("target/proofs"), "prolog-vectors")?;
    let oracle = observe(swipl(&package).arg("oracle.pl").arg(&written))?;
    succeeded("the oracle", &oracle)?;
    let fresh = fs::read(written.join(VECTORS))?;
    let held = fs::read(package.join("vectors").join(VECTORS))?;
    if fresh != held {
        return fail(format!(
            "proofs/prolog/vectors/{VECTORS} differs from what the specification writes; run `swipl oracle.pl vectors` in proofs/prolog"
        ));
    }
    print!("{}", String::from_utf8_lossy(&oracle.stdout));
    println!("Prolog: the specification passes its tests, tells every mistake apart, and writes the held vectors");
    Ok(())
}
