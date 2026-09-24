//! Every Win32 failure point returns its own error and releases what the operation acquired.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::thread;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON};

use crate::sys::fault::{self, Call};
use crate::sys::test_support::{
    current_process, isolated, pipe, process_handle_count, ProcessExitGuard,
};
use crate::{
    AsPseudoConsole, Command, DropPolicy, Job, Mitigation, MitigationPolicy, ParentProcess,
    SpawnOptions, Stdio, SuspendedChild,
};

/// An operation whose every Win32 call is failed in turn.
#[derive(Clone, Copy, Debug)]
enum Operation {
    SpawnAndWait,
    SpawnSuspendedAndResume,
    OutputWithKillTree,
    Status,
    PipeThroughChild,
    KillRunningChild,
    JobAssignAndTerminate,
    OpenParentProcess,
    ConfigureHandleInputs,
    RenderCommandLine,
    TryWaitExitedChild,
    SpawnOnPseudoConsole,
}

const OPERATIONS: [Operation; 12] = [
    Operation::SpawnAndWait,
    Operation::SpawnSuspendedAndResume,
    Operation::OutputWithKillTree,
    Operation::Status,
    Operation::PipeThroughChild,
    Operation::KillRunningChild,
    Operation::JobAssignAndTerminate,
    Operation::OpenParentProcess,
    Operation::ConfigureHandleInputs,
    Operation::RenderCommandLine,
    Operation::TryWaitExitedChild,
    Operation::SpawnOnPseudoConsole,
];

/// Capabilities the operations borrow, created before any call is recorded.
struct Fixture {
    nul: File,
    job: Job,
    parent: ParentProcess,
    host: SuspendedChild,
    _host_guard: ProcessExitGuard,
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
        })
    }

    /// A command that exits 0 whatever follows it and touches every configurable input.
    fn rich_command(&self) -> io::Result<Command> {
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

    fn run(&self, operation: Operation) -> io::Result<()> {
        match operation {
            Operation::SpawnAndWait => self.spawn_and_wait(),
            Operation::SpawnSuspendedAndResume => self.spawn_suspended_and_resume(),
            Operation::OutputWithKillTree => Self::output_with_kill_tree(),
            Operation::Status => Self::status(),
            Operation::PipeThroughChild => Self::pipe_through_child(),
            Operation::KillRunningChild => Self::kill_running_child(),
            Operation::JobAssignAndTerminate => Self::job_assign_and_terminate(),
            Operation::OpenParentProcess => self.open_parent_process(),
            Operation::ConfigureHandleInputs => self.configure_handle_inputs(),
            Operation::RenderCommandLine => Self::render_command_line(),
            Operation::TryWaitExitedChild => Self::try_wait_exited_child(),
            Operation::SpawnOnPseudoConsole => Self::spawn_on_pseudoconsole(),
        }
    }

    fn spawn_and_wait(&self) -> io::Result<()> {
        let mitigation = MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn);
        let options = SpawnOptions::new()
            .job(&self.job)
            .parent_process(&self.parent)
            .mitigation(mitigation)
            .drop_policy(DropPolicy::KillTree);
        let mut child = self.rich_command()?.spawn_with(options)?;
        child.wait()?;
        child.try_wait()?;
        Ok(())
    }

    fn spawn_suspended_and_resume(&self) -> io::Result<()> {
        let mitigation = MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn);
        let options = SpawnOptions::new().job(&self.job).mitigation(mitigation);
        let suspended = self.rich_command()?.spawn_suspended_with(options)?;
        suspended.resume()?.wait()?;
        Ok(())
    }

    fn output_with_kill_tree() -> io::Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "echo out"]);
        command.output_with(SpawnOptions::new().drop_policy(DropPolicy::KillTree))?;
        Ok(())
    }

    fn status() -> io::Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command.status()?;
        Ok(())
    }

    fn pipe_through_child() -> io::Result<()> {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/C", "more"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let written = stdin.write_all(b"line\r\n");
        drop(stdin);
        let mut output = Vec::new();
        let read = child
            .stdout
            .take()
            .expect("piped stdout")
            .read_to_end(&mut output);
        child.wait()?;
        written?;
        read?;
        Ok(())
    }

    fn kill_running_child() -> io::Result<()> {
        let mut child = gated_command().spawn()?;
        let running = child.try_wait();
        let killed = child.kill();
        child.wait()?;
        assert!(running?.is_none());
        killed
    }

    fn job_assign_and_terminate() -> io::Result<()> {
        let job = Job::create()?;
        job.set_kill_on_close(true)?;
        let adopted = Job::from_handle(crate::sys::duplicate_local(job.as_handle(), false)?)?;
        let duplicate = adopted.duplicate()?;
        let mut child = gated_command().spawn()?;
        let terminated = duplicate
            .assign(&child)
            .and_then(|()| duplicate.terminate(7));
        if terminated.is_err() {
            child.kill()?;
        }
        child.wait()?;
        terminated
    }

    fn open_parent_process(&self) -> io::Result<()> {
        ParentProcess::open(std::process::id())?;
        ParentProcess::from_handle(crate::sys::duplicate_local(self.host.as_handle(), false)?)?;
        Ok(())
    }

    fn configure_handle_inputs(&self) -> io::Result<()> {
        let mut command = Command::new("cmd.exe");
        command
            .arg_handle(&self.nul)?
            .env_handle("WINDOWS_SPAWN_FAULT_HANDLE", &self.nul)?;
        Stdio::from_borrowed(&self.nul)?;
        Ok(())
    }

    fn try_wait_exited_child() -> io::Result<()> {
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

    fn spawn_on_pseudoconsole() -> io::Result<()> {
        let console = TestConsole::create();
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command
            .spawn_with(SpawnOptions::new().pseudoconsole(&console))?
            .wait()?;
        Ok(())
    }

    fn render_command_line() -> io::Result<()> {
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
                Err(error) if fault::is_injected(&error) => {}
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
        Call::AttributeList,
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

/// A pseudoconsole created with raw Win32 calls, outside any recorded plan.
struct TestConsole {
    value: HPCON,
    input: Option<OwnedHandle>,
}

impl TestConsole {
    fn create() -> Self {
        let (input_reader, input_writer) = pipe();
        let (output_reader, output_writer) = pipe();
        let mut value = 0;
        // SAFETY: both pipe ends are valid for the call and `value` is writable; this type owns the HPCON.
        let created = unsafe {
            CreatePseudoConsole(
                COORD { X: 80, Y: 25 },
                input_reader.as_raw_handle() as HANDLE,
                output_writer.as_raw_handle() as HANDLE,
                0,
                &mut value,
            )
        };
        assert!(created >= 0, "CreatePseudoConsole failed with {created:#x}");
        drop((input_reader, output_writer));
        let mut output = File::from(output_reader);
        let _ = thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while matches!(output.read(&mut buffer), Ok(read) if read > 0) {}
        });
        Self {
            value,
            input: Some(input_writer),
        }
    }
}

/// The close runs on a detached thread because some Windows Server 2022 builds block in it.
impl Drop for TestConsole {
    fn drop(&mut self) {
        drop(self.input.take());
        let value = self.value;
        let _ = thread::spawn(move || {
            // SAFETY: this type uniquely owns the HPCON.
            unsafe { ClosePseudoConsole(value) };
        });
    }
}

// SAFETY: TestConsole owns a live HPCON for every borrow and keeps ownership.
unsafe impl AsPseudoConsole for TestConsole {
    fn raw_pseudoconsole(&self) -> isize {
        self.value
    }
}
