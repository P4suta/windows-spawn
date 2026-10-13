//! Every Win32 failure point returns its own error and releases what the operation acquired.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, OwnedHandle};

use crate::error::{Phase, Result, SpawnError};
use crate::sys::fault::{self, Call};
use crate::sys::test_support::{
    current_process, isolated, process_handle_count, ProcessExitGuard, TestConsole,
};
use crate::{
    Command, ConsoleMode, Job, Mitigation, MitigationPolicy, ParentProcess, SpawnOptions, Stdio,
    SuspendedChild,
};

/// The exit code the Job of `JobAssignAndTerminate` gives its process.
const JOB_EXIT_CODE: u32 = 7;

/// An operation whose every Win32 call is failed in turn.
#[derive(Clone, Copy, Debug)]
enum Operation {
    SpawnAndWait,
    SpawnSuspendedAndResume,
    OutputOfOwnedTree,
    Status,
    PipeThroughChild,
    KillRunningChild,
    JobAssignAndTerminate,
    OpenParentProcess,
    ConfigureHandleInputs,
    RenderCommandLine,
    TryWaitExitedChild,
    SpawnOnPseudoConsole,
    KillOwnedTree,
    WaitTree,
}

const OPERATIONS: [Operation; 14] = [
    Operation::SpawnAndWait,
    Operation::SpawnSuspendedAndResume,
    Operation::OutputOfOwnedTree,
    Operation::Status,
    Operation::PipeThroughChild,
    Operation::KillRunningChild,
    Operation::JobAssignAndTerminate,
    Operation::OpenParentProcess,
    Operation::ConfigureHandleInputs,
    Operation::RenderCommandLine,
    Operation::TryWaitExitedChild,
    Operation::SpawnOnPseudoConsole,
    Operation::KillOwnedTree,
    Operation::WaitTree,
];

/// Types the outcome of a pipe read or write, as the crate types its own calls.
fn pipe<T>(operation: crate::Operation, outcome: io::Result<T>) -> Result<T> {
    outcome.map_err(|error| SpawnError::windows(Phase::Runtime, operation, error))
}

/// A private duplicate, typed as the crate types its own duplicates.
fn duplicate(handle: &impl AsHandle) -> Result<OwnedHandle> {
    crate::sys::duplicate_local(handle.as_handle(), false).map_err(|error| {
        SpawnError::windows(
            Phase::Preparation,
            crate::Operation::DuplicateLocalHandle,
            error,
        )
    })
}

/// Returns true if `error` holds the injected failure, as the failure itself or as one that a release met.
fn injected(error: &SpawnError) -> bool {
    match error {
        SpawnError::Windows(error) => fault::is_injected(error.io_error()),
        SpawnError::Cleanup(cleanup) => {
            cleanup.primary().is_some_and(injected)
                || cleanup
                    .failures()
                    .any(|release| fault::is_injected(release.io_error()))
        }
        SpawnError::Validation(_) => false,
    }
}

/// Capabilities the operations borrow, created before any call is recorded.
struct Fixture {
    nul: File,
    job: Job,
    parent: ParentProcess,
    host: SuspendedChild,
    _host_guard: ProcessExitGuard,
    console: TestConsole,
}

impl Fixture {
    fn new() -> io::Result<Self> {
        let mut host_command = Command::new("cmd.exe");
        host_command
            .args(["/D", "/C", "exit /b 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let host = host_command.spawn_suspended()?;
        let host_guard = ProcessExitGuard::watch(host.as_handle());
        Ok(Self {
            nul: File::options().read(true).write(true).open("NUL")?,
            job: Job::create()?,
            parent: ParentProcess::open(host.id())?,
            host,
            _host_guard: host_guard,
            console: TestConsole::create(),
        })
    }

    /// A command that exits 0 whatever follows it and touches every configurable input.
    fn rich_command(&self) -> Result<Command> {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/C", "rem"])
            .arg_handle(&self.nul)?
            .env_handle("WINDOWS_SPAWN_FAULT_HANDLE", &self.nul)?
            .env("WINDOWS_SPAWN_FAULT_TEXT", "value")
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::inherit())
            .stdout(Stdio::from_borrowed(&self.nul)?)
            .stderr(Stdio::piped());
        Ok(command)
    }

    fn run(&self, operation: Operation) -> Result<()> {
        match operation {
            Operation::SpawnAndWait => self.spawn_and_wait(),
            Operation::SpawnSuspendedAndResume => self.spawn_suspended_and_resume(),
            Operation::OutputOfOwnedTree => Self::output_of_owned_tree(),
            Operation::Status => Self::status(),
            Operation::PipeThroughChild => Self::pipe_through_child(),
            Operation::KillRunningChild => Self::kill_running_child(),
            Operation::JobAssignAndTerminate => Self::job_assign_and_terminate(),
            Operation::OpenParentProcess => self.open_parent_process(),
            Operation::ConfigureHandleInputs => self.configure_handle_inputs(),
            Operation::RenderCommandLine => Self::render_command_line(),
            Operation::TryWaitExitedChild => Self::try_wait_exited_child(),
            Operation::SpawnOnPseudoConsole => self.spawn_on_pseudoconsole(),
            Operation::KillOwnedTree => Self::kill_owned_tree(),
            Operation::WaitTree => Self::wait_tree(),
        }
    }

    fn spawn_and_wait(&self) -> Result<()> {
        let mut command = self.rich_command()?;
        command
            .mitigation(MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn))
            .own_process_tree();
        let options = SpawnOptions::new()
            .job(&self.job)
            .parent_process(&self.parent);
        let mut child = command.spawn_with(options)?;
        child.wait()?;
        child.try_wait()?;
        Ok(())
    }

    fn spawn_suspended_and_resume(&self) -> Result<()> {
        let mut command = self.rich_command()?;
        command.mitigation(MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn));
        let suspended = command.spawn_suspended_with(SpawnOptions::new().job(&self.job))?;
        suspended.resume()?.wait()?;
        Ok(())
    }

    fn output_of_owned_tree() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "echo out"]).own_process_tree();
        command.output()?;
        Ok(())
    }

    fn status() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command.status()?;
        Ok(())
    }

    /// Writes and reads once each, so the call sequence does not depend on how the pipe splits output.
    fn pipe_through_child() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/C", "more"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let written = stdin.write(b"line\r\n");
        drop(stdin);
        let mut output = [0_u8; 1];
        let read = child.stdout.take().expect("piped stdout").read(&mut output);
        child.wait()?;
        pipe(crate::Operation::WritePipe, written)?;
        pipe(crate::Operation::ReadPipe, read)?;
        Ok(())
    }

    fn kill_running_child() -> Result<()> {
        let mut child = gated_command().spawn()?;
        let running = child.try_wait();
        let killed = child.kill();
        child.wait()?;
        assert!(running?.is_none());
        killed
    }

    fn job_assign_and_terminate() -> Result<()> {
        let job = Job::create()?;
        job.set_kill_on_close(true)?;
        let adopted = Job::from_handle(duplicate(&job)?)?;
        let duplicate = adopted.duplicate()?;
        let mut child = gated_command().spawn()?;
        let terminated = duplicate
            .assign(&child)
            .and_then(|()| duplicate.terminate(JOB_EXIT_CODE));
        if terminated.is_err() {
            child.kill()?;
        }
        child.wait()?;
        terminated
    }

    fn open_parent_process(&self) -> Result<()> {
        ParentProcess::open(std::process::id())?;
        ParentProcess::from_handle(duplicate(&self.host)?)?;
        Ok(())
    }

    fn configure_handle_inputs(&self) -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command
            .arg_handle(&self.nul)?
            .env_handle("WINDOWS_SPAWN_FAULT_HANDLE", &self.nul)?;
        Stdio::from_borrowed(&self.nul)?;
        Ok(())
    }

    fn try_wait_exited_child() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/C", "exit /b 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        ProcessExitGuard::watch(child.as_handle()).exit_code();
        assert!(child.try_wait()?.is_some());
        Ok(())
    }

    /// Reuses one pseudoconsole: closing one per run can block on some Windows Server 2022 builds and hold handles.
    fn spawn_on_pseudoconsole(&self) -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command
            .spawn_with(SpawnOptions::new().pseudo_console(&self.console))?
            .wait()?;
        Ok(())
    }

    fn render_command_line() -> Result<()> {
        let mut on_path = Command::new("cmd.exe");
        on_path
            .arg("two words")
            .env("WINDOWS_SPAWN_FAULT_TEXT", "value");
        on_path.to_command_line()?;
        let mut in_windows_directory = Command::new("explorer");
        in_windows_directory.env_clear();
        in_windows_directory.to_command_line()?;
        Ok(())
    }

    /// Kills a child that owns its tree, which terminates the private Job.
    fn kill_owned_tree() -> Result<()> {
        let mut child = gated_command().own_process_tree().spawn()?;
        let killed = child.kill();
        child.wait()?;
        killed
    }

    /// Waits for an owned tree whose only process still runs when the wait lists the tree.
    ///
    /// The child has no console, so no console host joins the tree and the call sequence is fixed.
    fn wait_tree() -> Result<()> {
        let mut command = gated_command();
        command.console(ConsoleMode::NoConsole).own_process_tree();
        command.spawn()?.wait_tree()?;
        Ok(())
    }

    /// Records `operation`'s calls, then fails each in turn and requires exactly that error back.
    fn assert_every_failure_propagates(&self, operation: Operation) -> Vec<Call> {
        let calls = {
            let plan = fault::record();
            self.run(operation)
                .unwrap_or_else(|error| panic!("{operation:?} fails without injection: {error}"));
            plan.calls()
        };
        assert!(!calls.is_empty(), "{operation:?} made no Win32 call");
        for (index, call) in calls.iter().enumerate() {
            let result = {
                let _plan = fault::fail_at(index);
                self.run(operation)
            };
            match result {
                Err(error) if injected(&error) => {}
                Err(error) => panic!(
                    "{operation:?}: failing call {index} ({call:?}) returned another error: {error}"
                ),
                Ok(()) => panic!("{operation:?}: failing call {index} ({call:?}) was ignored"),
            }
        }
        calls
    }
}

/// A command that blocks on its piped standard input until killed or given EOF.
fn gated_command() -> Command {
    let mut command = Command::new("cmd.exe");
    command
        .args(["/D", "/C", "set /p x="])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
fn every_win32_failure_propagates_as_the_same_error() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let mut reached = Vec::new();
    for operation in OPERATIONS {
        reached.extend(fixture.assert_every_failure_propagates(operation));
    }
    for call in [
        Call::DuplicateLocal,
        Call::DuplicateRemote,
        Call::ReclaimRemote,
        Call::StandardHandle,
        Call::NullHandle,
        Call::CreatePipe,
        Call::OpenProcess,
        Call::ValidateProcess,
        Call::CreateJob,
        Call::QueryJob,
        Call::SetJob,
        Call::AssignJob,
        Call::TerminateJob,
        Call::ListJob,
        Call::OpenJobProcess,
        Call::AttributeList,
        Call::AllocateAttributes,
        Call::UpdateAttribute,
        Call::CreateProcess,
        Call::WaitProcess,
        Call::TryWaitProcess,
        Call::ExitStatus,
        Call::TerminateProcess,
        Call::ResumeThread,
        Call::ReadHandle,
        Call::WriteHandle,
        Call::EnvironmentStrings,
        Call::MaximumPath,
    ] {
        assert!(reached.contains(&call), "no operation reaches {call:?}");
    }
    Ok(())
}

/// Compares this process's handle count, so it runs alone in a fresh test process.
#[test]
fn failed_operations_release_their_handles() -> io::Result<()> {
    if !isolated("failure_tests::failed_operations_release_their_handles") {
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let run_all = || {
        for operation in OPERATIONS {
            fixture.assert_every_failure_propagates(operation);
        }
    };
    run_all();
    let before = process_handle_count(current_process())?;
    run_all();
    let after = process_handle_count(current_process())?;
    assert_eq!(before, after, "failed operations leaked handles");
    Ok(())
}
