//! The TLA+ layer: TLC checks every behavior of the output-draining protocol in its finite instances.

use std::path::Path;
use std::process::Command;

use super::tools::{fetch, TLA2TOOLS};
use super::{fresh_directory, observe};
use crate::tasks::{fail, Result};

/// What TLC must conclude for one configuration of a specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Every behavior satisfies the invariants and properties.
    Holds,
    /// A behavior reaches a state where nothing can happen.
    Deadlock,
    /// A behavior violates a temporal property.
    Liveness,
}

/// The exit codes of `tlc2.output.EC.ExitStatus`.
const TLC_SUCCESS: i32 = 0;
const TLC_DEADLOCK: i32 = 11;
const TLC_LIVENESS: i32 = 13;

impl Verdict {
    /// The exit code TLC reports for the verdict.
    const fn exit_code(self) -> i32 {
        match self {
            Self::Holds => TLC_SUCCESS,
            Self::Deadlock => TLC_DEADLOCK,
            Self::Liveness => TLC_LIVENESS,
        }
    }

    /// The line TLC prints for the verdict.
    const fn message(self) -> &'static str {
        match self {
            Self::Holds => "Model checking completed. No error has been found.",
            Self::Deadlock => "Error: Deadlock reached.",
            Self::Liveness => "Error: Temporal properties were violated.",
        }
    }
}

/// A specification, a configuration, and the verdict the configuration must reach.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Check {
    pub(crate) spec: &'static str,
    pub(crate) config: &'static str,
    pub(crate) verdict: Verdict,
}

/// Every TLC run of `proofs/tla`; a configuration that is not a negative control must hold.
pub(crate) const CHECKS: &[Check] = &[
    Check {
        spec: "OutputDrain",
        config: "OutputDrain",
        verdict: Verdict::Holds,
    },
    Check {
        spec: "OutputDrain",
        config: "OutputDrainWaitFirst",
        verdict: Verdict::Deadlock,
    },
    Check {
        spec: "OutputDrain",
        config: "OutputDrainUnowned",
        verdict: Verdict::Liveness,
    },
];

/// Runs TLC on every configuration and requires each one's verdict.
pub(crate) fn tla(root: &Path) -> Result<()> {
    let jar = fetch(root, TLA2TOOLS)?;
    let states = fresh_directory(&root.join("target/proofs"), "tla-states")?;
    for check in CHECKS {
        let mut command = Command::new("java");
        command
            .current_dir(root.join("proofs/tla"))
            .args(["-XX:+UseParallelGC", "-cp"])
            .arg(&jar)
            .args(["tlc2.TLC", "-workers", "1", "-cleanup", "-config"])
            .arg(format!("{}.cfg", check.config))
            .arg("-metadir")
            .arg(states.join(check.config))
            .arg(format!("{}.tla", check.spec));
        let output = observe(&mut command)?;
        let report = String::from_utf8_lossy(&output.stdout);
        concluded(check, output.status.code(), &report)?;
    }
    println!(
        "TLA+: {} configurations reached their verdicts",
        CHECKS.len()
    );
    Ok(())
}

/// Requires TLC to have reached exactly the expected verdict, which a crash or a parse error never does.
fn concluded(check: &Check, code: Option<i32>, report: &str) -> Result<()> {
    if code == Some(check.verdict.exit_code()) && report.contains(check.verdict.message()) {
        Ok(())
    } else {
        fail(format!(
            "{}.cfg must conclude {:?} but TLC exited with {code:?}:\n{report}",
            check.config, check.verdict
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_expected_verdict_passes() {
        let check = CHECKS[1];
        assert!(concluded(&check, Some(11), "Error: Deadlock reached.").is_ok());
        assert!(concluded(&check, Some(0), Verdict::Holds.message()).is_err());
        assert!(concluded(
            &check,
            Some(11),
            "Error: Parsing or semantic analysis failed."
        )
        .is_err());
        assert!(concluded(&check, Some(150), "Error: Deadlock reached.").is_err());
        assert!(concluded(&CHECKS[0], Some(0), Verdict::Holds.message()).is_ok());
    }

    #[test]
    fn every_specification_has_a_holding_configuration() {
        for check in CHECKS {
            assert!(CHECKS
                .iter()
                .any(|other| other.spec == check.spec && other.verdict == Verdict::Holds));
        }
    }
}
