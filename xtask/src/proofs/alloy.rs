//! The Alloy layer: the handle-transfer design checked in every small configuration, and each of its decisions shown necessary.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::tools::{fetch, ALLOY};
use super::{fresh_directory, observe, succeeded};
use crate::tasks::Result;

/// The model the layer checks.
const MODEL: &str = "proofs/alloy/handle_transfer.als";

/// Whether the SAT solver must find an instance of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expect {
    /// A `check` that holds within its scope: no counterexample exists.
    NoInstance,
    /// A `run` whose scenario exists, or a `check` that a negative control must refute.
    Instance,
}

/// Every command of the model, with the result it must have.
pub(crate) const COMMANDS: &[(&str, Expect)] = &[
    ("CorrectInheritsExactly", Expect::NoInstance),
    ("CorrectListsEachValueOnce", Expect::NoInstance),
    ("CorrectLeavesNoDuplicate", Expect::NoInstance),
    ("SpawnThroughABusyParent", Expect::Instance),
    ("RollBackAfterADuplicate", Expect::Instance),
    ("NegativeControlInheritsAll", Expect::Instance),
    ("NegativeControlLowersLocally", Expect::Instance),
    ("NegativeControlKeepsRemote", Expect::Instance),
];

/// Runs every command of the model and requires each one's result.
pub(crate) fn alloy(root: &Path) -> Result<()> {
    let jar = fetch(root, ALLOY)?;
    let output = fresh_directory(&root.join("target/proofs"), "alloy")?;
    let mut command = Command::new("java");
    command
        .current_dir(root)
        .args(["--enable-native-access=ALL-UNNAMED", "-jar"])
        .arg(&jar)
        .args(["exec", "--force", "--quiet", "--type", "json", "--output"])
        .arg(&output)
        .arg(MODEL);
    let ran = observe(&mut command)?;
    succeeded("alloy exec", &ran)?;
    let receipt: Value = serde_json::from_slice(&fs::read(output.join("receipt.json"))?)?;
    check_receipt(&receipt).map_err(crate::tasks::TaskError::from)?;
    println!(
        "Alloy: {} commands of {MODEL} reached their results",
        COMMANDS.len()
    );
    Ok(())
}

/// Requires the receipt to hold exactly the inventory, each command with an instance exactly when it must have one.
fn check_receipt(receipt: &Value) -> std::result::Result<(), String> {
    let commands = receipt
        .pointer("/commands")
        .and_then(Value::as_object)
        .ok_or("the receipt lists no commands")?;
    let mut ran: Vec<&str> = commands.keys().map(String::as_str).collect();
    ran.sort_unstable();
    let mut expected: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
    expected.sort_unstable();
    if ran != expected {
        return Err(format!(
            "the model's commands {ran:?} differ from the inventory in xtask/src/proofs/alloy.rs {expected:?}"
        ));
    }
    for (name, expect) in COMMANDS {
        let found = commands
            .get(*name)
            .and_then(|command| command.pointer("/solution/0/instances"))
            .and_then(Value::as_array)
            .is_some_and(|instances| !instances.is_empty());
        let wanted = *expect == Expect::Instance;
        if found != wanted {
            return Err(format!(
                "{name} must end with {expect:?}, but the solver {} an instance",
                if found { "found" } else { "found no" }
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn receipt(found: &[&str]) -> Value {
        let mut commands = serde_json::Map::new();
        for (name, _) in COMMANDS {
            let mut command = json!({"name": name});
            if found.contains(name) {
                command["solution"] = json!([{"instances": [{"values": {}}]}]);
            }
            commands.insert((*name).to_owned(), command);
        }
        json!({ "commands": commands })
    }

    #[test]
    fn every_command_must_reach_its_result() {
        let instances: Vec<&str> = COMMANDS
            .iter()
            .filter(|(_, expect)| *expect == Expect::Instance)
            .map(|(name, _)| *name)
            .collect();
        assert!(check_receipt(&receipt(&instances)).is_ok());
        assert!(check_receipt(&receipt(&instances[1..])).is_err());
        let mut refuted = instances.clone();
        refuted.push("CorrectInheritsExactly");
        assert!(check_receipt(&receipt(&refuted)).is_err());
        let mut extra = receipt(&instances);
        extra["commands"]["Unlisted"] = json!({"name": "Unlisted"});
        assert!(check_receipt(&extra).is_err());
        assert!(check_receipt(&json!({})).is_err());
    }
}
