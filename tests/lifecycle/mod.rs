//! The traces of the Quint lifecycle model, replayed against the crate.
//!
//! `proofs/quint/traces/lifecycle.tsv` holds traces of `proofs/quint/lifecycle.qnt`, one step per row:
//! the trace, the step, the action and its argument, what the call returned, and the process state after it.
//! Each trace drives one probe through the public API.
//! After every step the replay requires the call to have returned what the row says, and the process to be in the row's state.
//! The table stays outside the package, so the replay compiles only with `--cfg windows_spawn_vectors`.

use std::fmt::Display;
use std::io;
use std::os::windows::io::{AsRawHandle, OwnedHandle};

use windows_spawn::{Child, DropPolicy, SpawnOptions, SuspendedChild};
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{SuspendThread, TerminateProcess, WaitForSingleObject};

use crate::support::{
    local_duplicate, suspend_count, Gate, ProbeCommand, ProcessExitGuard, Report, Role,
    SELF_EXIT_CODES,
};

const TRACES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proofs/quint/traces/lifecycle.tsv"
));
/// The rows `cargo xtask proof-traces` writes.
const ROWS: usize = 435;
/// The exit code another process gives the probe, as the model's `EXTERNAL`.
const EXTERNAL: u32 = 9;

/// The wrapper that owns the process, as the model's `owner`.
enum Owner {
    Suspended(SuspendedChild),
    Owned(Child),
    Dropped,
}

/// One trace in flight: the probe's owner and pipes, and handles that observe its process apart from the crate.
struct Replay {
    owner: Owner,
    gate: Gate,
    report: Option<Report>,
    /// The probe reported that it runs.
    ready: bool,
    process: ProcessExitGuard,
    observer: OwnedHandle,
}

impl Replay {
    /// Spawns the probe as `init` chose: under a drop policy, suspended or running.
    ///
    /// The probe command drops before the replay reads the report, because it holds copies of the pipe ends and a read waits for every writer to close.
    fn start(argument: &str) -> io::Result<Self> {
        let (policy, start) = argument
            .split_once(' ')
            .ok_or_else(|| malformed(format!("init names no start: {argument}")))?;
        let policy = match policy {
            "Detach" => DropPolicy::Detach,
            "KillTree" => DropPolicy::KillTree,
            other => return Err(malformed(format!("unknown drop policy {other}"))),
        };
        let options = SpawnOptions::new().drop_policy(policy);
        let (mut probe, gate, report) = ProbeCommand::new(Role::Gate)?;
        let (owner, process, observer) = match start {
            "suspended" => {
                let suspended = probe.command_mut().spawn_suspended_with(options)?;
                let process = ProcessExitGuard::watch(&suspended)?;
                let observer = local_duplicate(&suspended, false)?;
                (Owner::Suspended(suspended), process, observer)
            }
            "running" => {
                let child = probe.command_mut().spawn_with(options)?;
                let process = ProcessExitGuard::watch(&child)?;
                let observer = local_duplicate(&child, false)?;
                (Owner::Owned(child), process, observer)
            }
            other => return Err(malformed(format!("unknown start {other}"))),
        };
        drop(probe);
        Ok(Self {
            owner,
            gate,
            report: Some(report),
            ready: false,
            process,
            observer,
        })
    }

    /// Performs one action and returns what the call returned, in the model's terms.
    fn step(&mut self, action: &str, argument: &str) -> io::Result<String> {
        let owner = std::mem::replace(&mut self.owner, Owner::Dropped);
        let (owner, outcome) = match (action, owner) {
            ("suspendAgain", Owner::Suspended(suspended)) => {
                // SAFETY: the primary thread handle is valid while `suspended` lives.
                let previous =
                    unsafe { SuspendThread(suspended.primary_thread_handle().as_raw_handle()) };
                if previous == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                (Owner::Suspended(suspended), "Done".to_owned())
            }
            ("resume", Owner::Suspended(suspended)) => match suspended.resume() {
                Ok(child) => (Owner::Owned(child), "Resumed".to_owned()),
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    (Owner::Dropped, "InvalidData".to_owned())
                }
                Err(error) => return Err(error),
            },
            ("dropSuspended", Owner::Suspended(suspended)) => {
                drop(suspended);
                (Owner::Dropped, "Done".to_owned())
            }
            ("exits", owner) => {
                let code = argument
                    .parse()
                    .map_err(|error| malformed(format!("exit code {argument}: {error}")))?;
                self.gate.open_with(code)?;
                (owner, "Done".to_owned())
            }
            ("wait", Owner::Owned(mut child)) => {
                let status = child.wait()?;
                (Owner::Owned(child), status_of(status.code()))
            }
            ("tryWait", Owner::Owned(mut child)) => {
                let outcome = child
                    .try_wait()?
                    .map_or_else(|| "NoStatus".to_owned(), |status| status_of(status.code()));
                (Owner::Owned(child), outcome)
            }
            ("kill", Owner::Owned(mut child)) => {
                let outcome = match child.kill() {
                    Ok(()) => "Done".to_owned(),
                    Err(error)
                        if error.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED).ok() =>
                    {
                        "Denied".to_owned()
                    }
                    Err(error) => return Err(error),
                };
                (Owner::Owned(child), outcome)
            }
            ("dropChild", Owner::Owned(child)) => {
                drop(child);
                (Owner::Dropped, "Done".to_owned())
            }
            ("externalKill", owner) => {
                // SAFETY: the observer handle is valid while `self` lives, and it has the full access of the crate's handle.
                if unsafe { TerminateProcess(self.observer.as_raw_handle(), EXTERNAL) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                (owner, "Done".to_owned())
            }
            (action, _) => {
                return Err(malformed(format!(
                    "{action} does not apply to the probe's owner"
                )))
            }
        };
        self.owner = owner;
        Ok(outcome)
    }

    /// Requires the process to be in `state`, observed through handles the crate does not own.
    fn check(&mut self, state: &str) -> io::Result<()> {
        let (name, value) = state.split_once(' ').unwrap_or((state, ""));
        match name {
            "Paused" => {
                if self.exited()? {
                    return Err(differs("the process", state, "an exited process"));
                }
                let Owner::Suspended(suspended) = &self.owner else {
                    return Err(malformed(
                        "a paused process belongs to a SuspendedChild".to_owned(),
                    ));
                };
                let expected: u32 = value
                    .parse()
                    .map_err(|error| malformed(format!("suspend count {value}: {error}")))?;
                let actual = suspend_count(suspended.primary_thread_handle())?;
                if actual != expected {
                    return Err(differs("the suspend count", expected, actual));
                }
            }
            "Running" => {
                if !self.ready {
                    self.report()?.expect(b"r")?;
                    self.ready = true;
                }
                if self.exited()? {
                    return Err(differs("the process", state, "an exited process"));
                }
            }
            "Exited" => {
                let expected: u32 = value
                    .parse()
                    .map_err(|error| malformed(format!("exit code {value}: {error}")))?;
                let actual = self.process.exit_code()?;
                if actual != expected {
                    return Err(differs("the exit code", expected, actual));
                }
            }
            "Killed" => {
                if let Some(report) = self.report.take() {
                    self.gate.release();
                    if !report.rest()?.is_empty() {
                        return Err(differs(
                            "the process",
                            state,
                            "a process that saw its gate close",
                        ));
                    }
                }
                let actual = self.process.exit_code()?;
                if SELF_EXIT_CODES.contains(&actual) {
                    return Err(differs("the exit code", "a termination", actual));
                }
            }
            other => return Err(malformed(format!("unknown process state {other}"))),
        }
        Ok(())
    }

    fn report(&mut self) -> io::Result<&mut Report> {
        self.report
            .as_mut()
            .ok_or_else(|| malformed("the report pipe closed before the trace ended".to_owned()))
    }

    /// Returns true if the process has exited, without waiting.
    fn exited(&self) -> io::Result<bool> {
        // SAFETY: the observer handle is valid while `self` lives.
        match unsafe { WaitForSingleObject(self.observer.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

/// A row the replay cannot read.
fn malformed(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Marks every error that reports the crate disagreeing with a row.
const DIFFERS: &str = "differs from the model";

fn differs(what: &str, expected: impl Display, actual: impl Display) -> io::Error {
    io::Error::other(format!(
        "{what} {DIFFERS}: the row says {expected}, the crate gives {actual}"
    ))
}

fn status_of(code: Option<i32>) -> String {
    format!("Status {}", code.expect("a probe exits with a code"))
}

/// Replays every trace of `table` and returns the first disagreement.
fn replay(table: &str) -> io::Result<()> {
    let mut replay: Option<Replay> = None;
    for row in table.lines() {
        let fields: Vec<&str> = row.split('\t').collect();
        let [trace, step, action, argument, outcome, state] = fields.as_slice() else {
            return Err(malformed(format!("a row has six fields: {row:?}")));
        };
        let at = format!("trace {trace} step {step}, {action} {argument}");
        let returned = if *action == "init" {
            drop(replay.take());
            replay = Some(Replay::start(argument)?);
            "Started".to_owned()
        } else {
            replay
                .as_mut()
                .ok_or_else(|| malformed(format!("{at}: a trace starts with init")))?
                .step(action, argument)?
        };
        if returned != *outcome {
            return Err(differs(&format!("{at}: the result"), outcome, returned));
        }
        replay
            .as_mut()
            .ok_or_else(|| malformed(format!("{at}: a trace starts with init")))?
            .check(state)
            .map_err(|error| io::Error::new(error.kind(), format!("{at}: {error}")))?;
    }
    Ok(())
}

#[test]
fn the_crate_follows_every_lifecycle_trace() -> io::Result<()> {
    assert_eq!(TRACES.lines().count(), ROWS, "the table has every row");
    replay(TRACES)?;
    let traces = TRACES
        .lines()
        .filter(|row| row.contains("\tinit\t"))
        .count();
    println!("{traces} lifecycle traces, {ROWS} steps: the crate follows the model");
    Ok(())
}

/// Traces that contradict the crate, one for each kind of check, each with the fact it contradicts,
/// so the replay shows it can fail.
const CONTRADICTIONS: [(&str, &str); 4] = [
    (
        "`kill` after an exit nobody observed returns the error of `TerminateProcess`",
        "0\t0\tinit\tDetach running\tStarted\tRunning\n0\t1\texits\t3\tDone\tExited 3\n0\t2\tkill\t-\tDone\tExited 3\n",
    ),
    (
        "the probe exits with the code its gate carries",
        "0\t0\tinit\tDetach running\tStarted\tRunning\n0\t1\texits\t3\tDone\tExited 77\n",
    ),
    (
        "a second suspension raises the suspend count to two",
        "0\t0\tinit\tDetach suspended\tStarted\tPaused 1\n0\t1\tsuspendAgain\t-\tDone\tPaused 1\n",
    ),
    (
        "dropping a detached child leaves its process running",
        "0\t0\tinit\tDetach running\tStarted\tRunning\n0\t1\tdropChild\t-\tDone\tKilled\n",
    ),
];

#[test]
fn the_replay_rejects_traces_that_contradict_the_crate() {
    for (fact, trace) in CONTRADICTIONS {
        let error = replay(trace).expect_err(fact);
        assert!(error.to_string().contains(DIFFERS), "{fact}: {error}");
    }
}
