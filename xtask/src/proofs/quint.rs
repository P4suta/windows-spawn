//! The Quint layer: Apalache proves the lifecycle model's invariant inductively, and the Windows tests replay the model's traces.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use super::{fresh_directory, observe, succeeded};
use crate::tasks::{fail, Result};

/// The Apalache release `quint verify` runs.
const APALACHE: &str = "0.62.4";
/// The seed, count, and length of the traces the simulator generates; its TypeScript backend is deterministic for a seed.
const SEED: &str = "0x5eed";
const TRACES: &str = "400";
const STEPS: &str = "8";
/// The replay keeps the first traces, and the first few that reach each rare outcome.
const FIRST: usize = 40;
const RARE: usize = 5;
/// The steps the bounded check explores beyond what the inductive invariant covers.
const VERIFY_STEPS: &str = "10";
/// What the traces the repository holds must reach, so the replay exercises every outcome.
const COVERAGE: [&str; 7] = [
    "\tInvalidData\t",
    "\tDenied\t",
    "\tStatus ",
    "\tKilled",
    "\texternalKill\t",
    "\tresume\t-\tInvalidData\tExited 9",
    "\tdropSuspended\t-\tDone\tExited 9",
];
/// The replay table, beside the model; `tests/lifecycle` replays it on Windows.
const TABLE: &str = "proofs/quint/traces/lifecycle.tsv";
/// The model, and the npm package that pins Quint.
const MODEL: &str = "proofs/quint/lifecycle.qnt";
const PACKAGE: [&str; 2] = ["package.json", "package-lock.json"];

/// Installs the pinned Quint under `target/proofs/quint`, where Quint and Apalache also write what they produce.
fn workspace(root: &Path) -> Result<PathBuf> {
    let directory = root.join("target/proofs/quint");
    fs::create_dir_all(&directory)?;
    for file in PACKAGE {
        fs::copy(root.join("proofs/quint").join(file), directory.join(file))?;
    }
    let mut install = Command::new("npm");
    install
        .current_dir(&directory)
        .args(["ci", "--ignore-scripts", "--no-audit", "--no-fund"]);
    succeeded("npm ci", &observe(&mut install)?)?;
    Ok(directory)
}

/// Runs Quint on the model with `arguments`.
fn quint_on_model(root: &Path, workspace: &Path, command: &str, arguments: &[&str]) -> Command {
    let mut quint = Command::new("npx");
    quint
        .current_dir(workspace)
        .args(["--no-install", "quint", command])
        .arg(root.join(MODEL))
        .args(arguments);
    quint
}

/// Checks the lifecycle model and requires the repository's traces to be the ones it generates.
pub(crate) fn quint(root: &Path) -> Result<()> {
    let workspace = workspace(root)?;
    succeeded(
        "quint typecheck",
        &observe(&mut quint_on_model(root, &workspace, "typecheck", &[]))?,
    )?;

    let bounded = observe(&mut quint_on_model(
        root,
        &workspace,
        "verify",
        &[
            "--invariant",
            "invariant",
            "--max-steps",
            VERIFY_STEPS,
            "--apalache-version",
            APALACHE,
        ],
    ))?;
    holds("the bounded check", &bounded)?;
    let inductive = observe(&mut quint_on_model(
        root,
        &workspace,
        "verify",
        &[
            "--inductive-invariant",
            "inductive",
            "--apalache-version",
            APALACHE,
        ],
    ))?;
    holds("the inductive check", &inductive)?;
    let refuted = observe(&mut quint_on_model(
        root,
        &workspace,
        "verify",
        &[
            "--invariant",
            "killNeverFails",
            "--max-steps",
            VERIFY_STEPS,
            "--apalache-version",
            APALACHE,
        ],
    ))?;
    let report = String::from_utf8_lossy(&refuted.stdout);
    if refuted.status.success() || !report.contains("[violation]") {
        return fail(format!(
            "Apalache did not refute the negative control killNeverFails:\n{report}"
        ));
    }

    let table = generate(root, &workspace)?;
    if let Some(missing) = COVERAGE.iter().find(|outcome| !table.contains(*outcome)) {
        return fail(format!(
            "the traces never reach {missing:?}; raise the trace count or length"
        ));
    }
    let held = fs::read_to_string(root.join(TABLE))?;
    if held != table {
        return fail(format!(
            "{TABLE} differs from the traces the model generates; regenerate it with `cargo xtask proof-traces`"
        ));
    }
    println!("Quint: the invariant is inductive, the negative control is refuted, and the replay table holds the model's traces");
    Ok(())
}

/// Writes the traces the model generates into the replay table.
pub(crate) fn write_traces(root: &Path) -> Result<()> {
    let workspace = workspace(root)?;
    let table = generate(root, &workspace)?;
    fs::write(root.join(TABLE), table)?;
    Ok(())
}

/// Generates the seeded traces and returns their replay rows.
fn generate(root: &Path, workspace: &Path) -> Result<String> {
    let traces = fresh_directory(&root.join("target/proofs"), "quint-traces")?;
    let pattern = traces.join("trace_{seq}.itf.json");
    let pattern = pattern
        .to_str()
        .ok_or_else(|| crate::tasks::TaskError::from("the trace path is not UTF-8".to_owned()))?;
    let generated = observe(&mut quint_on_model(
        root,
        workspace,
        "run",
        &[
            "--backend",
            "typescript",
            "--mbt",
            "--n-traces",
            TRACES,
            "--max-samples",
            TRACES,
            "--max-steps",
            STEPS,
            "--seed",
            SEED,
            "--invariant",
            "invariant",
            "--out-itf",
            pattern,
        ],
    ))?;
    succeeded("quint run", &generated)?;
    let count: usize = TRACES.parse().map_err(|error| format!("{error}"))?;
    let mut all = Vec::with_capacity(count);
    for index in 0..count {
        let trace: Value =
            serde_json::from_slice(&fs::read(traces.join(format!("trace_{index}.itf.json")))?)?;
        all.push(rows(index, &trace)?);
    }
    Ok(select(&all).concat())
}

/// Keeps the first traces and the first few that reach each rare outcome, in their original order.
fn select(traces: &[String]) -> Vec<String> {
    let mut chosen: Vec<usize> = (0..traces.len().min(FIRST)).collect();
    for outcome in COVERAGE {
        chosen.extend(
            traces
                .iter()
                .enumerate()
                .filter(|(_, rows)| rows.contains(outcome))
                .map(|(index, _)| index)
                .take(RARE),
        );
    }
    chosen.sort_unstable();
    chosen.dedup();
    chosen
        .into_iter()
        .filter_map(|index| traces.get(index).cloned())
        .collect()
}

/// Requires Apalache to have found no violation.
fn holds(what: &str, output: &std::process::Output) -> Result<()> {
    succeeded(what, output)?;
    let report = String::from_utf8_lossy(&output.stdout);
    if report.contains("[ok] No violation found") {
        Ok(())
    } else {
        fail(format!("{what} reported no clean result:\n{report}"))
    }
}

/// A sum-type value of an ITF trace, as its tag followed by its integer payload when it has one.
fn variant(value: &Value) -> std::result::Result<String, String> {
    let tag = value
        .pointer("/tag")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{value} is no variant"))?;
    Ok(value
        .pointer("/value/#bigint")
        .and_then(Value::as_str)
        .map_or_else(|| tag.to_owned(), |payload| format!("{tag} {payload}")))
}

/// The replay rows of one trace: its index, the step, the action and its argument, and the outcome and process state after it.
/// A paused process carries the suspend count of its primary thread.
fn rows(index: usize, trace: &Value) -> std::result::Result<String, String> {
    let states = trace
        .pointer("/states")
        .and_then(Value::as_array)
        .ok_or("the trace has no states")?;
    let mut rows = String::new();
    for (step, state) in states.iter().enumerate() {
        let action = state
            .pointer("/mbt::actionTaken")
            .and_then(Value::as_str)
            .ok_or("a state names no action")?;
        let picked = |name: &str| state.pointer(&format!("/mbt::nondetPicks/{name}/value"));
        let argument = match action {
            "init" => {
                let policy = picked("chosen")
                    .map(variant)
                    .transpose()?
                    .unwrap_or_default();
                let start = match picked("suspended").and_then(Value::as_bool) {
                    Some(true) => "suspended",
                    Some(false) => "running",
                    None => return Err("init picked no start".to_owned()),
                };
                format!("{policy} {start}")
            }
            "exits" => picked("chosen")
                .and_then(|code| code.pointer("/#bigint"))
                .and_then(Value::as_str)
                .ok_or("exits picked no code")?
                .to_owned(),
            _ => "-".to_owned(),
        };
        let outcome = variant(state.pointer("/outcome").ok_or("a state has no outcome")?)?;
        let mut process = variant(state.pointer("/process").ok_or("a state has no process")?)?;
        if process == "Paused" {
            let suspensions = state
                .pointer("/suspensions/#bigint")
                .and_then(Value::as_str)
                .ok_or("a paused state has no suspend count")?;
            process = format!("Paused {suspensions}");
        }
        writeln!(
            rows,
            "{index}\t{step}\t{action}\t{argument}\t{outcome}\t{process}"
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn traces_become_replay_rows() {
        let trace = json!({"states": [
            {"mbt::actionTaken": "init",
             "mbt::nondetPicks": {"chosen": {"tag": "Some", "value": {"tag": "KillTree", "value": {"#tup": []}}},
                                  "suspended": {"tag": "Some", "value": true}},
             "outcome": {"tag": "Started", "value": {"#tup": []}},
             "process": {"tag": "Paused", "value": {"#tup": []}},
             "suspensions": {"#bigint": "1"}},
            {"mbt::actionTaken": "resume",
             "mbt::nondetPicks": {},
             "outcome": {"tag": "Resumed", "value": {"#tup": []}},
             "process": {"tag": "Running", "value": {"#tup": []}},
             "suspensions": {"#bigint": "0"}},
            {"mbt::actionTaken": "exits",
             "mbt::nondetPicks": {"chosen": {"tag": "Some", "value": {"#bigint": "77"}},
                                  "suspended": {"tag": "None", "value": {"#tup": []}}},
             "outcome": {"tag": "Done", "value": {"#tup": []}},
             "process": {"tag": "Exited", "value": {"#bigint": "77"}}},
            {"mbt::actionTaken": "wait",
             "mbt::nondetPicks": {},
             "outcome": {"tag": "Status", "value": {"#bigint": "77"}},
             "process": {"tag": "Exited", "value": {"#bigint": "77"}}}
        ]});
        assert_eq!(
            rows(3, &trace).unwrap(),
            "3\t0\tinit\tKillTree suspended\tStarted\tPaused 1\n3\t1\tresume\t-\tResumed\tRunning\n3\t2\texits\t77\tDone\tExited 77\n3\t3\twait\t-\tStatus 77\tExited 77\n"
        );
        assert!(rows(0, &json!({})).is_err());

        let mut traces = vec!["0\t0\tinit\t-\tStarted\tRunning\n".to_owned(); FIRST + 2];
        traces.push("42\t3\tdropChild\t-\tDone\tKilled\n".to_owned());
        let selected = select(&traces);
        assert_eq!(selected.len(), FIRST + 1);
        assert!(selected.last().unwrap().contains("Killed"));
        assert!(rows(0, &json!({"states": [{"mbt::actionTaken": "exits", "mbt::nondetPicks": {}, "outcome": {}, "process": {}}]})).is_err());
    }
}
