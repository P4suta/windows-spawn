use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle as SystemOwnedHandle};
use std::process::{ExitStatus, Output};
use std::thread;

use crate::error::{Error, Operation, Phase, Result, ValidationError};
use crate::handles::Job;
use crate::resource::{CurrentTable, OwnedHandle, PipeKind, ProcessKind, ThreadKind};
use crate::sys;
use crate::trace::{self, ResourceKind};

/// The writable parent end of a child's standard-input pipe.
#[derive(Debug)]
pub struct ChildStdin {
    handle: OwnedHandle<PipeKind, CurrentTable>,
}

impl Write for ChildStdin {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        trace::io(
            Phase::Runtime,
            Operation::WritePipe,
            ResourceKind::Pipe,
            || sys::write_handle(self.handle.as_handle(), buffer),
        )
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsHandle for ChildStdin {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// The readable parent end of a child's standard-output pipe.
#[derive(Debug)]
pub struct ChildStdout {
    handle: OwnedHandle<PipeKind, CurrentTable>,
}

impl Read for ChildStdout {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        trace::io(
            Phase::Runtime,
            Operation::ReadPipe,
            ResourceKind::Pipe,
            || sys::read_handle(self.handle.as_handle(), buffer),
        )
    }
}

impl AsHandle for ChildStdout {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// The readable parent end of a child's standard-error pipe.
#[derive(Debug)]
pub struct ChildStderr {
    handle: OwnedHandle<PipeKind, CurrentTable>,
}

impl Read for ChildStderr {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        trace::io(
            Phase::Runtime,
            Operation::ReadPipe,
            ResourceKind::Pipe,
            || sys::read_handle(self.handle.as_handle(), buffer),
        )
    }
}

impl AsHandle for ChildStderr {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

#[derive(Debug)]
enum ExecutionState {
    InitialSuspended,
    Running,
}

#[derive(Debug)]
pub(crate) struct ProcessOwner {
    process: OwnedHandle<ProcessKind, CurrentTable>,
    thread: OwnedHandle<ThreadKind, CurrentTable>,
    pid: u32,
    state: ExecutionState,
}

impl ProcessOwner {
    pub(crate) fn new(created: sys::CreatedProcess) -> Self {
        Self {
            process: OwnedHandle::from_system(created.process),
            thread: OwnedHandle::from_system(created.thread),
            pid: created.pid,
            state: ExecutionState::InitialSuspended,
        }
    }

    pub(crate) fn process_handle(&self) -> BorrowedHandle<'_> {
        self.process.as_handle()
    }

    fn thread_handle(&self) -> BorrowedHandle<'_> {
        self.thread.as_handle()
    }

    fn resume_with(
        &mut self,
        resume: impl FnOnce(BorrowedHandle<'_>) -> io::Result<u32>,
    ) -> Result<()> {
        let previous = trace::io(
            Phase::Resume,
            Operation::ResumeThread,
            ResourceKind::Thread,
            || resume(self.thread_handle()),
        )
        .map_err(|error| Error::windows(Phase::Resume, Operation::ResumeThread, error))?;
        if previous != 1 {
            return Err(Error::Validation(ValidationError::UnexpectedSuspendCount {
                actual: previous,
            }));
        }
        self.state = ExecutionState::Running;
        Ok(())
    }
}

impl Drop for ProcessOwner {
    fn drop(&mut self) {
        if matches!(self.state, ExecutionState::InitialSuspended) {
            let _ = trace::io(
                Phase::Cleanup,
                Operation::TerminateProcess,
                ResourceKind::Process,
                || sys::terminate_process(self.process_handle(), 1),
            );
        }
    }
}

#[derive(Debug)]
pub(crate) enum JobOwnership {
    Preserve,
    Terminate(Job),
}

#[derive(Debug)]
enum ProcessTree {
    Preserve(ProcessOwner),
    Terminate {
        job: Job,
        process: ProcessOwner,
        cleanup: CleanupState,
    },
}

#[derive(Debug)]
enum CleanupState {
    Pending,
    Complete,
}

impl ProcessTree {
    fn new(process: ProcessOwner, job: JobOwnership) -> Self {
        match job {
            JobOwnership::Preserve => Self::Preserve(process),
            JobOwnership::Terminate(job) => Self::Terminate {
                job,
                process,
                cleanup: CleanupState::Pending,
            },
        }
    }

    fn process(&self) -> &ProcessOwner {
        match self {
            Self::Preserve(process) | Self::Terminate { process, .. } => process,
        }
    }

    fn process_mut(&mut self) -> &mut ProcessOwner {
        match self {
            Self::Preserve(process) | Self::Terminate { process, .. } => process,
        }
    }

    fn terminate_descendants(&mut self) -> Result<()> {
        match self {
            Self::Preserve(_) => Ok(()),
            Self::Terminate { job, cleanup, .. } => match cleanup {
                CleanupState::Complete => Ok(()),
                CleanupState::Pending => {
                    job.terminate(1)?;
                    *cleanup = CleanupState::Complete;
                    Ok(())
                }
            },
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Self::Terminate {
            job,
            cleanup: CleanupState::Pending,
            ..
        } = self
        {
            let _ = job.terminate(1);
        }
    }
}

/// A running or exited process whose resources are owned exactly once.
#[derive(Debug)]
pub struct Child {
    tree: ProcessTree,
    /// A pipe connected to the child's standard input, when requested.
    pub stdin: Option<ChildStdin>,
    /// A pipe connected to the child's standard output, when requested.
    pub stdout: Option<ChildStdout>,
    /// A pipe connected to the child's standard error, when requested.
    pub stderr: Option<ChildStderr>,
    exit: Option<ExitStatus>,
}

impl Child {
    pub(crate) fn new(
        process: ProcessOwner,
        job: JobOwnership,
        stdin: Option<SystemOwnedHandle>,
        stdout: Option<SystemOwnedHandle>,
        stderr: Option<SystemOwnedHandle>,
    ) -> Self {
        Self {
            tree: ProcessTree::new(process, job),
            stdin: stdin.map(|handle| ChildStdin {
                handle: OwnedHandle::from_system(handle),
            }),
            stdout: stdout.map(|handle| ChildStdout {
                handle: OwnedHandle::from_system(handle),
            }),
            stderr: stderr.map(|handle| ChildStderr {
                handle: OwnedHandle::from_system(handle),
            }),
            exit: None,
        }
    }

    pub(crate) fn process_handle(&self) -> BorrowedHandle<'_> {
        self.tree.process().process_handle()
    }

    fn primary_thread_handle(&self) -> BorrowedHandle<'_> {
        self.tree.process().thread_handle()
    }

    pub(crate) fn resume_initial(&mut self) -> Result<()> {
        self.resume_initial_with(sys::resume_thread)
    }

    pub(crate) fn resume_initial_with(
        &mut self,
        resume: impl FnOnce(BorrowedHandle<'_>) -> io::Result<u32>,
    ) -> Result<()> {
        self.tree.process_mut().resume_with(resume)
    }

    /// Returns the process identifier captured at creation.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.tree.process().pid
    }

    /// Terminates the root process.
    ///
    /// # Errors
    ///
    /// Returns a typed Windows failure from `TerminateProcess`.
    pub fn kill(&mut self) -> Result<()> {
        if self.exit.is_some() {
            return Ok(());
        }
        trace::io(
            Phase::Runtime,
            Operation::TerminateProcess,
            ResourceKind::Process,
            || sys::terminate_process(self.process_handle(), 1),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::TerminateProcess, error))
    }

    /// Waits for exit and caches the status.
    ///
    /// # Errors
    ///
    /// Returns a typed failure if waiting or retrieving the exit code fails.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        if let Some(status) = self.exit {
            return Ok(status);
        }
        drop(self.stdin.take());
        trace::io(
            Phase::Runtime,
            Operation::WaitProcess,
            ResourceKind::Process,
            || sys::wait_process(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::WaitProcess, error))?;
        let status = trace::io(
            Phase::Runtime,
            Operation::QueryExitCode,
            ResourceKind::Process,
            || sys::exit_status(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::QueryExitCode, error))?;
        self.exit = Some(status);
        Ok(status)
    }

    /// Checks for exit without blocking, returning the cached status thereafter.
    ///
    /// # Errors
    ///
    /// Returns a typed failure if querying the process or its exit code fails.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        if !trace::io(
            Phase::Runtime,
            Operation::WaitProcess,
            ResourceKind::Process,
            || sys::try_wait_process(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::WaitProcess, error))?
        {
            return Ok(None);
        }
        let status = trace::io(
            Phase::Runtime,
            Operation::QueryExitCode,
            ResourceKind::Process,
            || sys::exit_status(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::QueryExitCode, error))?;
        self.exit = Some(status);
        Ok(self.exit)
    }

    /// Waits while draining both output pipes concurrently.
    ///
    /// # Errors
    ///
    /// Returns a typed failure from waiting, reading, joining, or Job cleanup.
    pub fn wait_with_output(mut self) -> Result<Output> {
        drop(self.stdin.take());
        let stdout_reader = self
            .stdout
            .take()
            .map(|stream| thread::spawn(move || drain_output(stream.handle)));
        let stderr_reader = self
            .stderr
            .take()
            .map(|stream| thread::spawn(move || drain_output(stream.handle)));
        let status = self.wait();
        let termination = self.tree.terminate_descendants();
        let readers = join_readers(stdout_reader, stderr_reader);
        let (stdout, stderr) = readers?;
        termination?;
        Ok(Output {
            status: status?,
            stdout,
            stderr,
        })
    }

    /// Performs policy-driven process-tree cleanup and consumes the child.
    ///
    /// # Errors
    ///
    /// Returns a typed cleanup failure if the Job cannot be terminated.
    pub fn cleanup(mut self) -> Result<()> {
        self.tree.terminate_descendants()
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.process_handle()
    }
}

fn drain_output(handle: OwnedHandle<PipeKind, CurrentTable>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = trace::io(
            Phase::Runtime,
            Operation::ReadPipe,
            ResourceKind::Pipe,
            || sys::read_handle(handle.as_handle(), &mut buffer),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::ReadPipe, error))?;
        let Some(read) = std::num::NonZeroUsize::new(read) else {
            drop(handle);
            return Ok(bytes);
        };
        let chunk = buffer
            .get(..read.get())
            .ok_or(Error::Validation(ValidationError::SizeOverflow))?;
        bytes.extend_from_slice(chunk);
    }
}

type Reader = thread::JoinHandle<Result<Vec<u8>>>;

fn join_reader(reader: Option<Reader>) -> Result<Vec<u8>> {
    match reader {
        Some(reader) => reader.join().map_err(|_| {
            Error::windows(
                Phase::Runtime,
                Operation::JoinOutputReader,
                io::Error::other("output reader thread panicked"),
            )
        })?,
        None => Ok(Vec::new()),
    }
}

fn join_readers(stdout: Option<Reader>, stderr: Option<Reader>) -> Result<(Vec<u8>, Vec<u8>)> {
    let stdout = join_reader(stdout);
    let stderr = join_reader(stderr);
    Ok((stdout?, stderr?))
}

/// A process whose primary thread has not yet been resumed.
#[derive(Debug)]
#[must_use = "dropping a suspended child terminates it"]
pub struct SuspendedChild {
    child: Child,
}

impl SuspendedChild {
    pub(crate) fn new(child: Child) -> Self {
        Self { child }
    }

    /// Returns the process identifier captured at creation.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Borrows the suspended process's primary thread handle.
    #[must_use]
    pub fn primary_thread_handle(&self) -> BorrowedHandle<'_> {
        self.child.primary_thread_handle()
    }

    /// Resumes the primary thread and transitions to an ordinary [`Child`].
    ///
    /// # Errors
    ///
    /// Returns a typed resume failure. Any failed transition terminates the
    /// still-owned process during emergency cleanup.
    pub fn resume(mut self) -> Result<Child> {
        self.child.resume_initial()?;
        Ok(self.child)
    }
}

impl AsHandle for SuspendedChild {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.child.as_handle()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::io;

    use super::*;
    use crate::{Command, JobClosePolicy, SpawnOptions};

    #[test]
    fn completed_job_cleanup_is_idempotent() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let mut child = command
            .spawn_with(SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses))?;
        child.tree.terminate_descendants()?;
        child.tree.terminate_descendants()?;
        let _status = child.wait()?;
        Ok(())
    }

    #[test]
    fn reader_panics_become_typed_join_failures() -> io::Result<()> {
        let failed = thread::spawn(|| -> Result<Vec<u8>> { panic!("synthetic reader panic") });
        let completed = thread::spawn(|| Ok(vec![1_u8]));
        let error = join_readers(Some(failed), Some(completed)).unwrap_err();
        let Error::Windows(error) = error else {
            return Err(io::Error::other("Windows error expected"));
        };
        assert_eq!(error.operation(), Operation::JoinOutputReader);
        Ok(())
    }
}
