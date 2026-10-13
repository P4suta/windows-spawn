//! The Lean layer: theorems for every input, the axioms they rest on, two independent kernels, and vectors that tie the models to the Rust functions.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{fresh_directory, observe, succeeded};
use crate::tasks::{fail, Result};

/// The library's exported theorems, each with the private witness that applies it to an input its hypotheses hold of.
pub(crate) const THEOREMS: &[(&str, &str)] = &[
    (
        "WindowsSpawn.CommandLine.argument_encodeArg",
        "WindowsSpawn.CommandLine.argument_encodeArg_witness",
    ),
    (
        "WindowsSpawn.CommandLine.encodeArg_plain",
        "WindowsSpawn.CommandLine.encodeArg_plain_witness",
    ),
    (
        "WindowsSpawn.CommandLine.split_encodeLine",
        "WindowsSpawn.CommandLine.split_encodeLine_witness",
    ),
];

/// The axioms a proof may rest on.
const ALLOWED_AXIOMS: [&str; 3] = ["propext", "Classical.choice", "Quot.sound"];

/// What the canary's three proofs rest on, which the audit must name when it refuses them.
const CANARY_AXIOMS: [&str; 3] = ["sorryAx", "native_decide", "Canary.declared"];

/// The vectors the oracle writes, which the repository holds.
const VECTORS: [&str; 2] = ["command_line_encoding.tsv", "command_line_parsing.tsv"];

/// The Rust tests that read the vectors.
const VECTOR_TESTS: [&str; 2] = [
    "domain::command_line_rules::tests::the_encoder_writes_what_the_lean_model_writes",
    "domain::command_line_rules::tests::the_parser_models_agree_with_the_lean_models",
];

fn lake(package: &Path) -> Command {
    let mut command = Command::new("lake");
    command.current_dir(package);
    command
}

/// Builds the theorems, audits their axioms, replays them in two kernels, and ties their models to the Rust functions.
pub(crate) fn lean(root: &Path) -> Result<()> {
    let package = root.join("proofs/lean");
    let built = observe(lake(&package).arg("build"))?;
    succeeded("lake build", &built)?;

    let audit = observe(lake(&package).args(["exe", "audit"]))?;
    succeeded("the axiom audit", &audit)?;
    check_audit(&String::from_utf8_lossy(&audit.stdout)).map_err(crate::tasks::TaskError::from)?;

    let canary = observe(lake(&package).args(["build", "Canary"]))?;
    succeeded("lake build Canary", &canary)?;
    let refused = observe(lake(&package).args(["exe", "audit", "Canary"]))?;
    check_canary(
        refused.status.code(),
        &String::from_utf8_lossy(&refused.stderr),
    )
    .map_err(crate::tasks::TaskError::from)?;

    let replayed = observe(lake(&package).args(["env", "leanchecker", "WindowsSpawn"]))?;
    succeeded("leanchecker", &replayed)?;
    nanoda(&package)?;

    oracle(root, &package)?;
    vector_tests(root)?;
    println!(
        "Lean: {} theorems on three axioms, replayed by leanchecker and nanoda, tied to Rust by {} vector files",
        THEOREMS.len(),
        VECTORS.len()
    );
    Ok(())
}

/// Requires the exported theorems to be exactly the inventory, each with its witness, on the allowed axioms only.
fn check_audit(report: &str) -> std::result::Result<(), String> {
    let mut public = Vec::new();
    let mut private = Vec::new();
    for line in report.lines() {
        let mut columns = line.split('\t');
        let (Some(name), Some(visibility), Some(axioms)) =
            (columns.next(), columns.next(), columns.next())
        else {
            return Err(format!("unexpected audit line: {line}"));
        };
        if let Some(axiom) = axioms
            .split(',')
            .find(|axiom| !axiom.is_empty() && !ALLOWED_AXIOMS.contains(axiom))
        {
            return Err(format!("{name} rests on {axiom}"));
        }
        match visibility {
            "public" => public.push(name),
            "private" => private.push(name),
            _ => return Err(format!("unexpected visibility in: {line}")),
        }
    }
    public.sort_unstable();
    let mut expected: Vec<&str> = THEOREMS.iter().map(|(theorem, _)| *theorem).collect();
    expected.sort_unstable();
    if public != expected {
        return Err(format!(
            "the exported theorems {public:?} differ from the inventory in xtask/src/proofs/lean.rs {expected:?}"
        ));
    }
    if let Some((theorem, witness)) = THEOREMS
        .iter()
        .find(|(_, witness)| !private.contains(witness))
    {
        return Err(format!("{theorem} has no witness {witness}"));
    }
    Ok(())
}

/// Requires the audit to refuse the canary on each of its three axioms.
fn check_canary(code: Option<i32>, refusal: &str) -> std::result::Result<(), String> {
    if code != Some(1) {
        return Err(format!(
            "the audit accepted the canary or failed to run it: exit {code:?}"
        ));
    }
    CANARY_AXIOMS
        .iter()
        .find(|axiom| !refusal.contains(*axiom))
        .map_or(Ok(()), |axiom| {
            Err(format!("the audit did not name {axiom} in the canary"))
        })
}

/// Checks the exported library in nanoda, a type checker independent of Lean's kernel.
fn nanoda(package: &Path) -> Result<()> {
    let mut command = lake(package);
    command
        .args(["env", "leanexport", "WindowsSpawn"])
        .stdout(Stdio::piped());
    println!("+ {command:?}");
    let mut exporter = command.spawn()?;
    let stream = exporter
        .stdout
        .take()
        .ok_or_else(|| crate::tasks::TaskError::from("leanexport has no output".to_owned()))?;
    let mut check = lake(package);
    check
        .args(["env", "nanoda_bin", "nanoda.json"])
        .stdin(Stdio::from(stream));
    let checked = observe(&mut check)?;
    let status = exporter.wait()?;
    if !status.success() {
        return fail(format!("leanexport failed with {status}"));
    }
    succeeded("nanoda", &checked)?;
    let report = String::from_utf8_lossy(&checked.stdout);
    if report.contains("with no typechecker errors") {
        Ok(())
    } else {
        fail(format!("nanoda reported no clean check:\n{report}"))
    }
}

/// Requires the oracle to write exactly the vectors the repository holds.
fn oracle(root: &Path, package: &Path) -> Result<()> {
    let written = fresh_directory(&root.join("target/proofs"), "lean-vectors")?;
    let run = observe(lake(package).args(["exe", "oracle"]).arg(&written))?;
    succeeded("the oracle", &run)?;
    for name in VECTORS {
        let fresh = fs::read(written.join(name))?;
        let held = fs::read(package.join("vectors").join(name))?;
        if fresh != held {
            return fail(format!(
                "proofs/lean/vectors/{name} differs from what the proved models write; run `lake exe oracle vectors` in proofs/lean"
            ));
        }
    }
    Ok(())
}

/// Runs the Rust tests that read the vectors, and requires each of them to run and pass.
fn vector_tests(root: &Path) -> Result<()> {
    let mut command = crate::tasks::cargo(root);
    crate::tasks::with_vectors(&mut command)
        .args(["test", "--locked", "--lib", "--target-dir"])
        .arg(root.join("target/proofs/vectors"))
        .args(["--", "--exact"])
        .args(VECTOR_TESTS);
    let run = observe(&mut command)?;
    succeeded("the vector tests", &run)?;
    let report = String::from_utf8_lossy(&run.stdout);
    let summary = format!("test result: ok. {} passed", VECTOR_TESTS.len());
    if VECTOR_TESTS
        .iter()
        .all(|test| report.contains(&format!("test {test} ... ok")))
        && report.contains(&summary)
    {
        Ok(())
    } else {
        fail(format!("not every vector test ran:\n{report}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit(lines: &[&str]) -> String {
        lines.join("\n")
    }

    fn complete() -> Vec<String> {
        THEOREMS
            .iter()
            .flat_map(|(theorem, witness)| {
                [
                    format!("{theorem}\tpublic\tQuot.sound,propext"),
                    format!("{witness}\tprivate\tQuot.sound,propext"),
                ]
            })
            .collect()
    }

    #[test]
    fn the_audit_must_list_exactly_the_inventory_with_witnesses() {
        let lines = complete();
        let report: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(check_audit(&audit(&report)), Ok(()));
        assert!(check_audit(&audit(&report[1..])).is_err());
        assert!(check_audit(&audit(&report[..report.len() - 1])).is_err());

        let mut extra = report.clone();
        extra.push("WindowsSpawn.CommandLine.unlisted\tpublic\tpropext");
        assert!(check_audit(&audit(&extra)).is_err());

        let mut unfinished = report.clone();
        unfinished.push("WindowsSpawn.CommandLine.helper\tprivate\tsorryAx");
        assert!(check_audit(&audit(&unfinished)).is_err());
        assert!(check_audit("").is_err());
    }

    #[test]
    fn the_canary_must_be_refused_on_each_axiom() {
        let refusal = "Canary.assumed rests on [Canary.declared]\nCanary.unfinished rests on [sorryAx]\nCanary.compiled rests on [Canary.compiled._native.native_decide.ax_1_1]";
        assert_eq!(check_canary(Some(1), refusal), Ok(()));
        assert!(check_canary(Some(0), refusal).is_err());
        assert!(check_canary(Some(2), refusal).is_err());
        assert!(check_canary(Some(1), "Canary.unfinished rests on [sorryAx]").is_err());
    }
}
