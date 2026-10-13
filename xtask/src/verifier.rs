//! The Windows tests under Application Verifier, which stops a process on handle misuse, heap corruption, lock misuse, or a leak.
//!
//! Each test of the library and of `tests/windows_spawn.rs` runs alone, so a stop names its test.
//! Three tests misuse a handle on purpose to prove a refusal, and each must stop with its own code,
//! which shows that the verifier watches the tests at all.
//! The tests without the libtest harness run whole, and a stop in any child they start fails them.

use std::path::Path;

use crate::tasks::Result;

/// The verifier layers every test runs under.
#[cfg(windows)]
const LAYERS: [&str; 8] = [
    "Handles",
    "Heaps",
    "Locks",
    "Memory",
    "SRWLock",
    "Threadpool",
    "TLS",
    "Leak",
];

/// The tests that misuse a handle on purpose, with the stop each must raise.
#[cfg(any(windows, test))]
const EXPECTED_STOPS: [(&str, &str); 3] = [
    (
        "handles::tests::owned_handle_adoption_validates_resource_kind",
        "0x305",
    ),
    (
        "sys::tests::environment_paths_comparison_and_error_helpers_work",
        "0x305",
    ),
    (
        "handle_list_excludes_an_unlisted_inheritable_handle",
        "0x300",
    ),
];

/// The test targets without the libtest harness, which run whole.
#[cfg(windows)]
const WHOLE: [&str; 2] = ["argv_roundtrip", "command_line_parsers"];

/// How many logs of one image the gate reads, newest first; each process of the image writes one.
#[cfg(windows)]
const MOST_LOGS: usize = 64;

/// What a test must do under the verifier.
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
enum Expectation {
    /// Pass with no verifier entry.
    Clean,
    /// Fail on a verifier stop with this code.
    Stop(&'static str),
}

#[cfg(any(windows, test))]
fn expectation(test: &str) -> Expectation {
    EXPECTED_STOPS
        .iter()
        .find(|(name, _)| *name == test)
        .map_or(Expectation::Clean, |(_, code)| Expectation::Stop(code))
}

/// The stop codes of every entry in a log Application Verifier exported.
#[cfg(any(windows, test))]
fn stop_codes(log: &str) -> Vec<String> {
    log.match_indices("StopCode=\"")
        .filter_map(|(start, marker)| {
            let rest = log.get(start.saturating_add(marker.len())..)?;
            rest.split('"').next().map(str::to_owned)
        })
        .collect()
}

/// Requires a test's result to meet its expectation.
#[cfg(any(windows, test))]
fn judge(test: &str, passed: bool, codes: &[String]) -> std::result::Result<(), String> {
    match expectation(test) {
        Expectation::Clean if passed && codes.is_empty() => Ok(()),
        Expectation::Clean => Err(format!(
            "{test} must pass with no verifier entry, but it {} with {codes:?}",
            if passed { "passed" } else { "failed" }
        )),
        Expectation::Stop(code) if !passed && codes.iter().any(|seen| seen == code) => Ok(()),
        Expectation::Stop(code) => Err(format!(
            "{test} misuses a handle on purpose and must stop with {code}, but it {} with {codes:?}; is the verifier active?",
            if passed { "passed" } else { "failed" }
        )),
    }
}

#[cfg(windows)]
pub(crate) fn verifier(root: &Path) -> Result<()> {
    windows::verifier(root)
}

#[cfg(not(windows))]
pub(crate) fn verifier(_root: &Path) -> Result<()> {
    crate::tasks::fail("Application Verifier runs only on Windows")
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use serde_json::Value;

    use super::{judge, stop_codes, EXPECTED_STOPS, LAYERS, MOST_LOGS, WHOLE};
    use crate::proofs::{fresh_directory, observe, succeeded};
    use crate::tasks::{cargo, fail, with_vectors, Result, TaskError};

    /// Verifier settings for one image, removed with its logs when dropped.
    struct Enabled<'a> {
        image: &'a str,
    }

    impl<'a> Enabled<'a> {
        fn new(image: &'a str) -> Result<Self> {
            let mut enable = Command::new("appverif");
            enable.arg("-enable").args(LAYERS).args(["-for", image]);
            succeeded(
                "appverif -enable, which needs an administrator",
                &observe(&mut enable)?,
            )?;
            Ok(Self { image })
        }

        fn clear_logs(&self) -> Result<()> {
            let mut delete = Command::new("appverif");
            delete.args(["-delete", "logs", "-for", self.image]);
            succeeded("appverif -delete logs", &observe(&mut delete)?)
        }

        /// The stop codes in every log the image's processes wrote since the last clear.
        fn stop_codes(&self, scratch: &Path) -> Result<Vec<String>> {
            let mut codes = Vec::new();
            for index in 0..MOST_LOGS {
                let log = scratch.join(format!("log-{index}.xml"));
                let mut export = Command::new("appverif");
                export
                    .args(["-export", "log", "-for", self.image, "-with"])
                    .arg(format!("To={}", log.display()))
                    .arg(format!("Log={index}"));
                observe(&mut export)?;
                let Ok(text) = fs::read_to_string(&log) else {
                    return Ok(codes);
                };
                fs::remove_file(&log)?;
                codes.extend(stop_codes(&text));
            }
            fail(format!("{} wrote more than {MOST_LOGS} logs", self.image))
        }
    }

    impl Drop for Enabled<'_> {
        fn drop(&mut self) {
            for arguments in [["-delete", "logs"], ["-disable", "*"]] {
                drop(
                    Command::new("appverif")
                        .args(arguments)
                        .args(["-for", self.image])
                        .output(),
                );
            }
        }
    }

    /// The test executables of the crate: (target name, path).
    fn executables(root: &Path) -> Result<Vec<(String, PathBuf)>> {
        let mut compile = cargo(root);
        with_vectors(&mut compile).args(["test", "--no-run", "--locked", "--message-format=json"]);
        let compiled = observe(&mut compile)?;
        succeeded("cargo test --no-run", &compiled)?;
        let mut executables = Vec::new();
        for line in String::from_utf8_lossy(&compiled.stdout).lines() {
            let Ok(message) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let kind = message
                .pointer("/target/kind/0")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if message.pointer("/reason").and_then(Value::as_str) != Some("compiler-artifact")
                || !matches!(kind, "lib" | "test")
            {
                continue;
            }
            if let (Some(name), Some(path)) = (
                message.pointer("/target/name").and_then(Value::as_str),
                message.pointer("/executable").and_then(Value::as_str),
            ) {
                executables.push((name.to_owned(), PathBuf::from(path)));
            }
        }
        Ok(executables)
    }

    fn listed_tests(executable: &Path) -> Result<Vec<String>> {
        let mut list = Command::new(executable);
        list.args(["--list", "--format", "terse"]);
        let listed = observe(&mut list)?;
        succeeded("the test list", &listed)?;
        Ok(String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter_map(|line| line.strip_suffix(": test"))
            .map(str::to_owned)
            .collect())
    }

    pub(super) fn verifier(root: &Path) -> Result<()> {
        let scratch = fresh_directory(&root.join("target"), "verifier")?;
        let mut failures = Vec::new();
        let mut stopped = Vec::new();
        let mut tests = 0_usize;
        for (name, executable) in executables(root)? {
            let image = executable
                .file_name()
                .and_then(|image| image.to_str())
                .ok_or_else(|| {
                    TaskError::from(format!("{} has no file name", executable.display()))
                })?
                .to_owned();
            let enabled = Enabled::new(&image)?;
            let runs: Vec<Option<String>> = if WHOLE.contains(&name.as_str()) {
                vec![None]
            } else {
                listed_tests(&executable)?.into_iter().map(Some).collect()
            };
            for run in runs {
                enabled.clear_logs()?;
                let mut command = Command::new(&executable);
                command.current_dir(root);
                if let Some(test) = &run {
                    command.args(["--exact", test, "--test-threads=1"]);
                }
                let passed = observe(&mut command)?.status.success();
                let codes = enabled.stop_codes(&scratch)?;
                let label = run.unwrap_or_else(|| name.clone());
                if !codes.is_empty() {
                    stopped.push(label.clone());
                }
                if let Err(failure) = judge(&label, passed, &codes) {
                    failures.push(failure);
                }
                tests = tests.saturating_add(1);
            }
        }
        if let Some((test, _)) = EXPECTED_STOPS
            .iter()
            .find(|(test, _)| !stopped.iter().any(|seen| seen == test))
        {
            failures.push(format!("{test} never ran under the verifier"));
        }
        if !failures.is_empty() {
            return fail(failures.join("\n"));
        }
        println!(
            "Application Verifier: {tests} runs, each clean but the {} that misuse a handle on purpose and stopped",
            EXPECTED_STOPS.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "<avrf:logEntry Time=\"t\" LayerName=\"Handles\" StopCode=\"0x305\" Severity=\"Error\"><avrf:message>Incorrect object type for handle.</avrf:message></avrf:logEntry>";

    #[test]
    fn stops_are_read_from_exported_logs() {
        assert_eq!(stop_codes(LOG), ["0x305"]);
        assert_eq!(stop_codes("<avrf:logSession/>"), Vec::<String>::new());
    }

    #[test]
    fn only_the_deliberate_misuse_may_stop() {
        let stop = vec!["0x305".to_owned()];
        assert!(judge("child::tests::reader_errors_reach_the_caller", true, &[]).is_ok());
        assert!(judge("child::tests::reader_errors_reach_the_caller", true, &stop).is_err());
        assert!(judge("child::tests::reader_errors_reach_the_caller", false, &[]).is_err());
        let deliberate = "handles::tests::owned_handle_adoption_validates_resource_kind";
        assert!(judge(deliberate, false, &stop).is_ok());
        assert!(judge(deliberate, true, &[]).is_err());
        assert!(judge(deliberate, false, &["0x300".to_owned()]).is_err());
    }
}
