//! The Kani layer: bounded and complete proofs of the platform-independent decisions, run on the production source.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::fresh_directory;
use crate::tasks::{fail, Result};

/// The Kani release the proofs pin.
const KANI_VERSION: &str = "0.68.0";

/// Parallel harness jobs, bounded so a run leaves the machine usable.
const KANI_JOBS: &str = "4";

/// A harness that must verify, with the number of cover properties it must satisfy.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Proof {
    pub(crate) name: &'static str,
    pub(crate) covers: u64,
}

/// A harness whose deliberately false claim must fail on the assertion that carries `marker`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Control {
    pub(crate) name: &'static str,
    pub(crate) marker: &'static str,
}

/// Every Kani harness that must verify.
pub(crate) const KANI_PROOFS: &[Proof] = &[
    Proof {
        name: "domain::attribute_rules::proofs::storage_covers_the_probed_size_within_one_allocation",
        covers: 3,
    },
    Proof {
        name: "domain::attribute_rules::proofs::only_a_sized_failure_yields_storage",
        covers: 1,
    },
    Proof {
        name: "domain::attribute_rules::proofs::count_matches_the_requested_attributes",
        covers: 2,
    },
    Proof {
        name: "domain::command_line_rules::proofs::one_argument_decodes_to_itself",
        covers: 4,
    },
    Proof {
        name: "domain::command_line_rules::proofs::adjacent_arguments_stay_apart",
        covers: 2,
    },
    Proof {
        name: "domain::command_line_rules::proofs::plain_arguments_stay_verbatim",
        covers: 2,
    },
    Proof {
        name: "domain::creation_flag_rules::proofs::the_word_holds_the_policy_one_console_one_priority_and_the_internal_flags",
        covers: 2,
    },
    Proof {
        name: "domain::environment_rules::proofs::built_blocks_lay_out_each_entry_before_the_terminator",
        covers: 2,
    },
    Proof {
        name: "domain::environment_rules::proofs::entries_read_each_text_between_nuls",
        covers: 2,
    },
    Proof {
        name: "domain::environment_rules::proofs::parsed_back_blocks_of_one_entry",
        covers: 1,
    },
    Proof {
        name: "domain::environment_rules::proofs::entries_split_at_the_first_separator_after_the_key_start",
        covers: 1,
    },
    Proof {
        name: "domain::plan_rules::proofs::plans_follow_the_documented_rules",
        covers: 5,
    },
    Proof {
        name: "mitigation::proofs::every_setter_writes_exactly_its_field",
        covers: 3,
    },
    Proof {
        name: "mitigation::proofs::a_new_policy_defers_every_field",
        covers: 0,
    },
];

/// Every Kani harness that must fail, one for each module with proofs.
pub(crate) const KANI_CONTROLS: &[Control] = &[
    Control {
        name: "domain::attribute_rules::proofs::negative_control_unbounded_storage",
        marker: "NEGATIVE_ATTRIBUTE_CONTROL",
    },
    Control {
        name: "domain::command_line_rules::proofs::negative_control_unescaped_closing_quote",
        marker: "NEGATIVE_COMMAND_LINE_CONTROL",
    },
    Control {
        name: "domain::creation_flag_rules::proofs::negative_control_a_spawn_that_runs_at_once",
        marker: "NEGATIVE_CREATION_FLAG_CONTROL",
    },
    Control {
        name: "domain::environment_rules::proofs::negative_control_first_separator_anywhere",
        marker: "NEGATIVE_ENVIRONMENT_CONTROL",
    },
    Control {
        name: "domain::plan_rules::proofs::negative_control_parent_with_inherited_stdio",
        marker: "NEGATIVE_PLAN_CONTROL",
    },
    Control {
        name: "mitigation::proofs::negative_control_write_without_clearing",
        marker: "NEGATIVE_MITIGATION_CONTROL",
    },
];

/// Runs every Kani harness once and accepts only the expected report: each proof verified, each negative control refused.
pub(crate) fn kani(root: &Path) -> Result<()> {
    if cfg!(windows) {
        return fail("Kani has no Windows build; run `cargo xtask proofs kani` on Linux or macOS");
    }
    let reports = fresh_directory(&root.join("target/proofs"), "kani")?;
    let path = reports.join("kani.json");
    let status = kani_command(root, &path).status()?;
    let report = read_report(&path)?;
    check_inventory(&harness_names(&report)?, KANI_PROOFS, KANI_CONTROLS)?;
    check_run(&report, KANI_PROOFS.len(), KANI_CONTROLS.len())?;
    check_proofs(&report, KANI_PROOFS)?;
    check_controls(&report, KANI_CONTROLS)?;
    if status.success() {
        return fail("cargo kani passed, so a negative control verified a false claim");
    }
    println!(
        "Kani {KANI_VERSION}: {} proofs verified and {} negative controls refused",
        KANI_PROOFS.len(),
        KANI_CONTROLS.len()
    );
    Ok(())
}

fn kani_command(root: &Path, report: &Path) -> Command {
    let mut command = crate::tasks::cargo(root);
    command
        .args(["kani", "--lib", "--target-dir"])
        .arg(root.join("target/proofs/kani-build"))
        .args(["--output-format=terse", "--jobs", KANI_JOBS])
        .args(["-Z", "unstable-options", "--export-json"])
        .arg(report);
    println!("+ {command:?}");
    command
}

/// Reads the name of every harness the crate declares, which the report lists whether or not it verified.
fn harness_names(report: &Value) -> std::result::Result<Vec<String>, String> {
    let harnesses = report
        .pointer("/harness_metadata")
        .and_then(Value::as_array)
        .ok_or("the Kani report lists no harnesses")?;
    let mut names = Vec::new();
    for harness in harnesses {
        names.push(
            harness
                .pointer("/pretty_name")
                .and_then(Value::as_str)
                .ok_or("a Kani harness has no name")?
                .to_owned(),
        );
        if harness.pointer("/attributes/kind").and_then(Value::as_str) != Some("Proof") {
            return Err(format!("{harness} is no plain proof harness"));
        }
    }
    names.sort();
    Ok(names)
}

/// Requires the crate's harnesses to be exactly the proofs and controls the gate expects.
fn check_inventory(
    listed: &[String],
    proofs: &[Proof],
    controls: &[Control],
) -> std::result::Result<(), String> {
    let mut expected: Vec<&str> = proofs
        .iter()
        .map(|proof| proof.name)
        .chain(controls.iter().map(|control| control.name))
        .collect();
    expected.sort_unstable();
    let mut listed: Vec<&str> = listed.iter().map(String::as_str).collect();
    listed.sort_unstable();
    if listed == expected {
        return Ok(());
    }
    let missing: Vec<&&str> = expected
        .iter()
        .filter(|name| !listed.contains(name))
        .collect();
    let extra: Vec<&&str> = listed
        .iter()
        .filter(|name| !expected.contains(name))
        .collect();
    Err(format!(
        "the Kani harnesses differ from the inventory in xtask/src/proofs.rs: missing {missing:?}, unlisted {extra:?}"
    ))
}

fn read_report(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn count(value: &Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(Value::as_u64)
}

fn results<'a>(report: &'a Value, harness: &str) -> Vec<&'a Value> {
    report
        .pointer("/verification_results/results")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter(|result| text(result, "/harness_id") == harness)
                .collect()
        })
        .unwrap_or_default()
}

fn properties<'a>(report: &'a Value, harness: &str) -> Vec<&'a Value> {
    report
        .pointer("/property_details")
        .and_then(Value::as_array)
        .map(|details| {
            details
                .iter()
                .filter(|detail| text(detail, "/harness_id") == harness)
                .collect()
        })
        .unwrap_or_default()
}

fn checks(result: &Value) -> &[Value] {
    result
        .pointer("/checks")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Requires the pinned Kani to have run every harness to completion, verifying `successful` and refuting `failed`.
fn check_run(report: &Value, successful: usize, failed: usize) -> std::result::Result<(), String> {
    let version = text(report, "/metadata/kani_version");
    if version != KANI_VERSION {
        return Err(format!(
            "the report comes from Kani {version:?}, not the pinned {KANI_VERSION}"
        ));
    }
    let as_count = |value: usize| u64::try_from(value).map_err(|error| error.to_string());
    let total = as_count(successful.saturating_add(failed))?;
    let summary = |name: &str| count(report, &format!("/verification_results/summary/{name}"));
    let completed = text(report, "/verification_results/summary/status") == "completed"
        && summary("total_harnesses") == Some(total)
        && summary("executed") == Some(total)
        && summary("successful") == Some(as_count(successful)?)
        && summary("failed") == Some(as_count(failed)?);
    if completed {
        Ok(())
    } else {
        Err(format!(
            "Kani did not run {total} harnesses to {successful} proofs and {failed} refutations: {}",
            report
                .pointer("/verification_results/summary")
                .unwrap_or(&Value::Null)
        ))
    }
}

/// Requires every proof to verify with its covers satisfied and every assertion of the crate reachable.
fn check_proofs(report: &Value, proofs: &[Proof]) -> std::result::Result<(), String> {
    for proof in proofs {
        let found = (results(report, proof.name), properties(report, proof.name));
        let ([result], [properties]) = (found.0.as_slice(), found.1.as_slice()) else {
            return Err(format!(
                "the report holds no single result for {}",
                proof.name
            ));
        };
        let details = |name: &str| count(properties, &format!("/property_details/{name}"));
        let complete = text(result, "/status") == "Success"
            && !checks(result).is_empty()
            && details("failed") == Some(0)
            && details("undetermined") == Some(0)
            && details("solver_error") == Some(0)
            && details("unsatisfiable") == Some(0)
            && details("satisfied") == Some(proof.covers);
        if !complete {
            return Err(format!(
                "{} did not verify with {} satisfied covers: {}",
                proof.name, proof.covers, properties
            ));
        }
        for check in checks(result) {
            let own = text(check, "/location/file").starts_with("src/");
            let assertion = text(check, "/category") == "assertion";
            if own && assertion && text(check, "/status") != "Success" {
                return Err(format!(
                    "{} has an assertion that is not proved reachable: {check}",
                    proof.name
                ));
            }
        }
    }
    Ok(())
}

/// Requires every control to fail on its marked assertion and on nothing else of the crate.
fn check_controls(report: &Value, controls: &[Control]) -> std::result::Result<(), String> {
    for control in controls {
        let found = results(report, control.name);
        let [result] = found.as_slice() else {
            return Err(format!(
                "the report holds no single result for {}",
                control.name
            ));
        };
        let failures: Vec<&Value> = checks(result)
            .iter()
            .filter(|check| text(check, "/status") == "Failure")
            .collect();
        let refused = text(result, "/status") == "Failure"
            && !failures.is_empty()
            && failures.iter().all(|check| {
                text(check, "/category") == "assertion"
                    && text(check, "/description").contains(control.marker)
            });
        if !refused {
            return Err(format!(
                "{} must fail only on the assertion marked {}, not on a tool error: {failures:?}",
                control.name, control.marker
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PROOF: Proof = Proof {
        name: "domain::proofs::holds",
        covers: 1,
    };
    const CONTROL: Control = Control {
        name: "domain::proofs::negative_control_false",
        marker: "NEGATIVE_TEST_CONTROL",
    };

    /// A report of one proof and one control, with the control refused on `description`.
    fn report(description: &str, category: &str) -> Value {
        json!({
            "metadata": {"kani_version": KANI_VERSION},
            "harness_metadata": [
                {"pretty_name": PROOF.name, "attributes": {"kind": "Proof"}},
                {"pretty_name": CONTROL.name, "attributes": {"kind": "Proof"}}
            ],
            "verification_results": {
                "summary": {"status": "completed", "total_harnesses": 2, "executed": 2, "successful": 1, "failed": 1},
                "results": [
                    {"harness_id": PROOF.name, "status": "Success", "checks": [
                        {"status": "Success", "category": "assertion", "description": "x", "location": {"file": "src/a.rs"}},
                        {"status": "Satisfied", "category": "cover", "description": "c", "location": {"file": "src/a.rs"}}
                    ]},
                    {"harness_id": CONTROL.name, "status": "Failure", "checks": [
                        {"status": "Failure", "category": category, "description": description, "location": {"file": "src/a.rs"}}
                    ]}
                ]
            },
            "property_details": [
                {"harness_id": PROOF.name, "property_details": {"failed": 0, "undetermined": 0, "solver_error": 0, "unsatisfiable": 0, "satisfied": 1}},
                {"harness_id": CONTROL.name, "property_details": {"failed": 1}}
            ]
        })
    }

    fn passing() -> Value {
        report("\"NEGATIVE_TEST_CONTROL\"", "assertion")
    }

    fn accepts(report: &Value) -> std::result::Result<(), String> {
        check_inventory(&harness_names(report)?, &[PROOF], &[CONTROL])?;
        check_run(report, 1, 1)?;
        check_proofs(report, &[PROOF])?;
        check_controls(report, &[CONTROL])
    }

    #[test]
    fn a_complete_report_passes() {
        assert_eq!(accepts(&passing()), Ok(()));
    }

    #[test]
    fn empty_stale_and_partial_reports_fail() {
        let empty = json!({
            "metadata": {"kani_version": KANI_VERSION},
            "harness_metadata": [],
            "verification_results": {"summary": {"status": "completed", "total_harnesses": 0, "executed": 0, "successful": 0, "failed": 0}, "results": []},
            "property_details": []
        });
        assert!(accepts(&empty).is_err());

        let mut stale = passing();
        stale["metadata"]["kani_version"] = json!("0.67.0");
        assert!(accepts(&stale).is_err());

        let mut unfinished = passing();
        unfinished["verification_results"]["summary"]["status"] = json!("partial");
        assert!(accepts(&unfinished).is_err());

        let mut short = passing();
        short["verification_results"]["summary"]["executed"] = json!(1);
        assert!(accepts(&short).is_err());
    }

    #[test]
    fn failing_vacuous_and_undetermined_proofs_fail() {
        let mut failed = passing();
        failed["verification_results"]["results"][0]["status"] = json!("Failure");
        assert!(accepts(&failed).is_err());

        let mut uncovered = passing();
        uncovered["property_details"][0]["property_details"]["satisfied"] = json!(0);
        assert!(accepts(&uncovered).is_err());

        let mut undetermined = passing();
        undetermined["property_details"][0]["property_details"]["undetermined"] = json!(1);
        assert!(accepts(&undetermined).is_err());

        let mut unreachable = passing();
        unreachable["verification_results"]["results"][0]["checks"][0]["status"] =
            json!("Unreachable");
        assert!(accepts(&unreachable).is_err());
    }

    #[test]
    fn controls_must_fail_on_their_own_assertion() {
        assert!(accepts(&report("\"OTHER\"", "assertion")).is_err());
        assert!(accepts(&report("unwinding assertion loop 0", "unwind")).is_err());
        let mut verified = passing();
        verified["verification_results"]["results"][1]["status"] = json!("Success");
        assert!(accepts(&verified).is_err());
    }

    #[test]
    fn the_inventory_matches_the_report_both_ways() {
        let listed = vec![PROOF.name.to_owned(), CONTROL.name.to_owned()];
        assert_eq!(check_inventory(&listed, &[PROOF], &[CONTROL]), Ok(()));
        assert!(check_inventory(&listed[..1], &[PROOF], &[CONTROL]).is_err());
        assert!(check_inventory(&listed, &[PROOF], &[]).is_err());

        let mut contract = passing();
        contract["harness_metadata"][0]["attributes"]["kind"] = json!({"ProofForContract": {}});
        assert!(harness_names(&contract).is_err());
    }

    #[test]
    fn the_real_inventory_names_each_harness_once() {
        let mut names: Vec<&str> = KANI_PROOFS
            .iter()
            .map(|proof| proof.name)
            .chain(KANI_CONTROLS.iter().map(|control| control.name))
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(KANI_CONTROLS
            .iter()
            .all(|control| control.name.contains("negative_control")));
        assert!(KANI_PROOFS
            .iter()
            .all(|proof| !proof.name.contains("negative_control")));
    }
}
