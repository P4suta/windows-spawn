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
        .map(|count| count.0)
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
        .map(|count| count.0)
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
        .map(|count| count.0)
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
pub(crate) struct ResumeCompleted(());

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
        resume: fn(BorrowedHandle<'_>) -> io::Result<sys::PreviousSuspendCount>,
    ) -> Result<ResumeCompleted> {
        trace::io(
            Phase::Resume,
            Operation::ResumeThread,
            ResourceKind::Thread,
            || resume(self.thread_handle()),
        )
        .map_err(|error| Error::windows(Phase::Resume, Operation::ResumeThread, error))
        .and_then(|previous| {
            if previous.0 == 1 {
                self.state = ExecutionState::Running;
                Ok(ResumeCompleted(()))
            } else {
                Err(Error::Validation(ValidationError::UnexpectedSuspendCount {
                    actual: previous.0,
                }))
            }
        })
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
    Terminate(TerminatingProcessTree),
}

#[derive(Debug)]
enum JobCleanupState {
    Armed,
    Attempted,
}

#[derive(Debug)]
struct TerminatingProcessTree {
    job: Job,
    process: ProcessOwner,
    cleanup: JobCleanupState,
    #[cfg(test)]
    emergency_cleanup: fn(&Job, u32) -> Result<()>,
}

impl TerminatingProcessTree {
    fn new(job: Job, process: ProcessOwner) -> Self {
        Self {
            job,
            process,
            cleanup: JobCleanupState::Armed,
            #[cfg(test)]
            emergency_cleanup: Job::terminate,
        }
    }

    fn terminate_with(mut self, terminate: fn(&Job, u32) -> Result<()>) -> Result<CleanupOutcome> {
        self.cleanup = JobCleanupState::Attempted;
        terminate(&self.job, 1).map(|()| CleanupOutcome::Terminated)
    }
}

impl Drop for TerminatingProcessTree {
    fn drop(&mut self) {
        if matches!(self.cleanup, JobCleanupState::Armed) {
            self.cleanup = JobCleanupState::Attempted;
            #[cfg(test)]
            let terminate = self.emergency_cleanup;
            #[cfg(not(test))]
            let terminate = Job::terminate;
            let _emergency_cleanup = terminate(&self.job, 1);
        }
    }
}

/// The process-tree action completed by [`Child::cleanup`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupOutcome {
    /// No owned terminating Job was present.
    Preserved,
    /// The owned terminating Job was terminated.
    Terminated,
}

impl ProcessTree {
    fn new(process: ProcessOwner, job: JobOwnership) -> Self {
        match job {
            JobOwnership::Preserve => Self::Preserve(process),
            JobOwnership::Terminate(job) => {
                Self::Terminate(TerminatingProcessTree::new(job, process))
            }
        }
    }

    fn process(&self) -> &ProcessOwner {
        match self {
            Self::Preserve(process) => process,
            Self::Terminate(tree) => &tree.process,
        }
    }

    fn process_mut(&mut self) -> &mut ProcessOwner {
        match self {
            Self::Preserve(process) => process,
            Self::Terminate(tree) => &mut tree.process,
        }
    }

    fn terminate_descendants(self) -> Result<CleanupOutcome> {
        self.terminate_descendants_with(Job::terminate)
    }

    fn terminate_descendants_with(
        self,
        terminate: fn(&Job, u32) -> Result<()>,
    ) -> Result<CleanupOutcome> {
        match self {
            Self::Preserve(_) => Ok(CleanupOutcome::Preserved),
            Self::Terminate(tree) => tree.terminate_with(terminate),
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

    pub(crate) fn resume_initial(&mut self) -> Result<ResumeCompleted> {
        self.resume_initial_with(sys::resume_thread)
    }

    pub(crate) fn resume_initial_with(
        &mut self,
        resume: fn(BorrowedHandle<'_>) -> io::Result<sys::PreviousSuspendCount>,
    ) -> Result<ResumeCompleted> {
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
        self.kill_with(sys::terminate_process)
    }

    fn kill_with(
        &mut self,
        terminate: fn(BorrowedHandle<'_>, u32) -> io::Result<()>,
    ) -> Result<()> {
        if self.exit.is_some() {
            return Ok(());
        }
        trace::io(
            Phase::Runtime,
            Operation::TerminateProcess,
            ResourceKind::Process,
            || terminate(self.process_handle(), 1),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::TerminateProcess, error))
    }

    /// Waits for exit and caches the status.
    ///
    /// # Errors
    ///
    /// Returns a typed failure if waiting or retrieving the exit code fails.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        self.wait_with(sys::wait_process, sys::exit_status)
    }

    fn wait_with(
        &mut self,
        wait: fn(BorrowedHandle<'_>) -> io::Result<()>,
        status: fn(BorrowedHandle<'_>) -> io::Result<ExitStatus>,
    ) -> Result<ExitStatus> {
        if let Some(status) = self.exit {
            return Ok(status);
        }
        drop(self.stdin.take());
        trace::io(
            Phase::Runtime,
            Operation::WaitProcess,
            ResourceKind::Process,
            || wait(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::WaitProcess, error))
        .and_then(|()| {
            trace::io(
                Phase::Runtime,
                Operation::QueryExitCode,
                ResourceKind::Process,
                || status(self.process_handle()),
            )
            .map_err(|error| Error::windows(Phase::Runtime, Operation::QueryExitCode, error))
        })
        .map(|status| {
            self.exit = Some(status);
            status
        })
    }

    /// Checks for exit without blocking, returning the cached status thereafter.
    ///
    /// # Errors
    ///
    /// Returns a typed failure if querying the process or its exit code fails.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.try_wait_with(sys::try_wait_process, sys::exit_status)
    }

    fn try_wait_with(
        &mut self,
        wait: fn(BorrowedHandle<'_>) -> io::Result<bool>,
        status: fn(BorrowedHandle<'_>) -> io::Result<ExitStatus>,
    ) -> Result<Option<ExitStatus>> {
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        trace::io(
            Phase::Runtime,
            Operation::WaitProcess,
            ResourceKind::Process,
            || wait(self.process_handle()),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::WaitProcess, error))
        .and_then(|exited| {
            if exited {
                trace::io(
                    Phase::Runtime,
                    Operation::QueryExitCode,
                    ResourceKind::Process,
                    || status(self.process_handle()),
                )
                .map_err(|error| Error::windows(Phase::Runtime, Operation::QueryExitCode, error))
                .map(Some)
            } else {
                Ok(None)
            }
        })
        .map(|status| {
            self.exit = status;
            self.exit
        })
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
        output_result(status, termination, readers)
    }

    /// Performs policy-driven process-tree cleanup and consumes the child.
    ///
    /// # Errors
    ///
    /// Returns a typed cleanup failure if the Job cannot be terminated.
    pub fn cleanup(self) -> Result<CleanupOutcome> {
        self.tree.terminate_descendants()
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.process_handle()
    }
}

fn drain_output(handle: OwnedHandle<PipeKind, CurrentTable>) -> Result<Vec<u8>> {
    drain_output_with(handle, sys::read_handle)
}

fn drain_output_with(
    handle: OwnedHandle<PipeKind, CurrentTable>,
    read_handle: fn(BorrowedHandle<'_>, &mut [u8]) -> io::Result<sys::ReadCount>,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = trace::io(
            Phase::Runtime,
            Operation::ReadPipe,
            ResourceKind::Pipe,
            || read_handle(handle.as_handle(), &mut buffer),
        )
        .map_err(|error| Error::windows(Phase::Runtime, Operation::ReadPipe, error))?;
        let Some(read) = std::num::NonZeroUsize::new(read.0) else {
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

fn output_result(
    status: Result<ExitStatus>,
    termination: Result<CleanupOutcome>,
    readers: Result<(Vec<u8>, Vec<u8>)>,
) -> Result<Output> {
    let (stdout, stderr) = readers?;
    let _cleanup = termination?;
    Ok(Output {
        status: status?,
        stdout,
        stderr,
    })
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
    use std::os::windows::process::ExitStatusExt;

    use super::*;
    use crate::{Command, JobClosePolicy, SpawnOptions};

    #[test]
    fn cleanup_outcome_follows_job_ownership() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        assert_eq!(command.spawn()?.cleanup()?, CleanupOutcome::Preserved);

        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        assert_eq!(
            command
                .spawn_with(
                    SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses)
                )?
                .cleanup()?,
            CleanupOutcome::Terminated
        );
        Ok(())
    }

    #[test]
    fn emergency_cleanup_is_an_explicit_drop_transition() -> Result<()> {
        fn panic_on_cleanup(_: &Job, _: u32) -> Result<()> {
            panic!("emergency cleanup invoked");
        }

        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let mut child = command
            .spawn_with(SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses))?;
        match &mut child.tree {
            ProcessTree::Terminate(tree) => tree.emergency_cleanup = panic_on_cleanup,
            ProcessTree::Preserve(_) => {
                return Err(Error::Validation(ValidationError::SizeOverflow));
            }
        }
        let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(child)));
        assert!(dropped.is_err());
        Ok(())
    }

    #[test]
    fn failed_resume_is_reported_without_starting_the_process() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let mut suspended = command.spawn_suspended()?;
        let error = suspended
            .child
            .resume_initial_with(|_| Err(io::Error::from_raw_os_error(5)))
            .unwrap_err();
        let Error::Windows(error) = error else {
            return Err(Error::Validation(ValidationError::SizeOverflow));
        };
        assert_eq!(error.operation(), Operation::ResumeThread);
        assert!(matches!(
            suspended
                .child
                .resume_initial_with(|_| Ok(sys::PreviousSuspendCount(2))),
            Err(Error::Validation(ValidationError::UnexpectedSuspendCount {
                actual: 2
            }))
        ));
        Ok(())
    }

    #[test]
    fn child_stdout_read_returns_the_emitted_byte() -> Result<()> {
        let pipe = sys::create_pipe(sys::PipeDirection::ParentReads)
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreatePipe, error))?;
        let written = sys::write_handle(pipe.child.as_handle(), b"x")
            .map_err(|error| Error::windows(Phase::Runtime, Operation::WritePipe, error))?;
        assert_eq!(written.0, 1);
        drop(pipe.child);
        let mut stdout = ChildStdout {
            handle: OwnedHandle::from_system(pipe.parent),
        };
        let mut buffer = [0_u8; 4];
        let read = stdout
            .read(&mut buffer)
            .map_err(|error| Error::windows(Phase::Runtime, Operation::ReadPipe, error))?;
        assert_eq!(read, 1);
        assert_eq!(buffer.first(), Some(&b'x'));
        Ok(())
    }

    #[test]
    fn reader_panics_become_typed_join_failures() -> io::Result<()> {
        assert!(join_reader(None).map_err(io::Error::from)?.is_empty());
        let inner_failure = thread::spawn(|| -> Result<Vec<u8>> {
            Err(Error::Validation(ValidationError::SizeOverflow))
        });
        assert!(join_reader(Some(inner_failure)).is_err());

        let failed = thread::spawn(|| -> Result<Vec<u8>> { panic!("synthetic reader panic") });
        let completed = thread::spawn(|| Ok(vec![1_u8]));
        let error = join_readers(Some(failed), Some(completed)).unwrap_err();
        let Error::Windows(error) = error else {
            return Err(io::Error::other("Windows error expected"));
        };
        assert_eq!(error.operation(), Operation::JoinOutputReader);

        let completed = thread::spawn(|| Ok(vec![1_u8]));
        let failed = thread::spawn(|| -> Result<Vec<u8>> { panic!("synthetic reader panic") });
        assert!(join_readers(Some(completed), Some(failed)).is_err());
        Ok(())
    }

    #[test]
    fn runtime_failures_are_typed_before_os_resources_are_released() -> Result<()> {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let mut suspended = command.spawn_suspended_with(
            SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses),
        )?;
        let child = &mut suspended.child;
        assert!(child
            .kill_with(|_, _| Err(io::Error::from_raw_os_error(5)))
            .is_err());
        assert!(child
            .wait_with(
                |_| Err(io::Error::from_raw_os_error(5)),
                |_| Ok(ExitStatus::from_raw(0)),
            )
            .is_err());
        assert!(child
            .wait_with(|_| Ok(()), |_| Err(io::Error::from_raw_os_error(5)),)
            .is_err());
        assert!(child
            .try_wait_with(
                |_| Err(io::Error::from_raw_os_error(5)),
                |_| Ok(ExitStatus::from_raw(0)),
            )
            .is_err());
        assert_eq!(
            child.try_wait_with(|_| Ok(false), |_| Ok(ExitStatus::from_raw(0)))?,
            None
        );
        assert!(child
            .try_wait_with(|_| Ok(true), |_| Err(io::Error::from_raw_os_error(5)),)
            .is_err());
        assert_eq!(
            child
                .try_wait_with(|_| Ok(true), |_| Ok(ExitStatus::from_raw(7)))?
                .and_then(|status| status.code()),
            Some(7)
        );
        child.kill_with(|_, _| Err(io::Error::from_raw_os_error(5)))?;
        assert_eq!(
            child
                .wait_with(|_| Ok(()), |_| Ok(ExitStatus::from_raw(0)))?
                .code(),
            Some(7)
        );
        assert_eq!(
            child.try_wait_with(|_| Ok(false), |_| Ok(ExitStatus::from_raw(0)))?,
            child.exit
        );
        child.exit = None;
        let child = suspended.child;
        assert!(child
            .tree
            .terminate_descendants_with(|_, _| {
                Err(Error::Validation(ValidationError::SizeOverflow))
            })
            .is_err());
        Ok(())
    }

    #[test]
    fn drain_and_output_composition_cover_all_results() -> Result<()> {
        let successful_read = sys::create_pipe(sys::PipeDirection::ParentReads)
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreatePipe, error))?;
        sys::write_handle(successful_read.child.as_handle(), b"x")
            .map_err(|error| Error::windows(Phase::Runtime, Operation::WritePipe, error))?;
        drop(successful_read.child);
        let handle = OwnedHandle::from_system(successful_read.parent);
        assert_eq!(drain_output_with(handle, sys::read_handle)?, b"x");

        let failed_read = sys::create_pipe(sys::PipeDirection::ParentReads)
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreatePipe, error))?;
        let handle = OwnedHandle::from_system(failed_read.parent);
        assert!(
            drain_output_with(handle, |_, _| { Err(io::Error::from_raw_os_error(5)) }).is_err()
        );
        drop(failed_read.child);

        let oversized_read = sys::create_pipe(sys::PipeDirection::ParentReads)
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreatePipe, error))?;
        let handle = OwnedHandle::from_system(oversized_read.parent);
        assert!(drain_output_with(handle, |_, buffer| {
            Ok(sys::ReadCount(buffer.len().saturating_add(1)))
        })
        .is_err());
        drop(oversized_read.child);

        let status = || Ok(ExitStatus::from_raw(0));
        assert!(output_result(
            status(),
            Ok(CleanupOutcome::Preserved),
            Err(Error::Validation(ValidationError::SizeOverflow)),
        )
        .is_err());
        assert!(output_result(
            status(),
            Err(Error::Validation(ValidationError::SizeOverflow)),
            Ok((Vec::new(), Vec::new())),
        )
        .is_err());
        assert!(output_result(
            Err(Error::Validation(ValidationError::SizeOverflow)),
            Ok(CleanupOutcome::Preserved),
            Ok((Vec::new(), Vec::new()))
        )
        .is_err());
        let output = output_result(
            status(),
            Ok(CleanupOutcome::Preserved),
            Ok((vec![1], vec![2])),
        )?;
        assert_eq!(output.stdout, vec![1]);
        assert_eq!(output.stderr, vec![2]);
        Ok(())
    }
}
