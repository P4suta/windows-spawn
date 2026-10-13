//! Owned child process and suspended type state.

use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};
use std::process::{ExitStatus, Output};
use std::thread;

use crate::error::{Operation, Phase, Result, SpawnError};
use crate::handles::{windows, Job, Stdio};
use crate::resource::{Borrowed, Owned, Pipe, Process, Thread};
use crate::sys;

/// The writable parent end of a child's standard-input pipe.
///
/// Like the output ends, it converts into [`Stdio`] and into [`OwnedHandle`].
#[derive(Debug)]
pub struct ChildStdin {
    handle: Owned<Pipe>,
}

impl Write for ChildStdin {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        sys::write_handle(self.handle.borrow(), buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &ChildStdin {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        sys::write_handle(self.handle.borrow(), buffer)
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

impl AsRawHandle for ChildStdin {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

/// The readable parent end of a child's standard-output pipe.
///
/// It converts into [`Stdio`], so one child's output can feed another's input:
///
/// ```
/// use windows_spawn::{Command, Stdio};
///
/// let mut producer = Command::new("cmd.exe");
/// producer.args(["/D", "/C", "echo piped"]).stdout(Stdio::piped());
/// let mut first = producer.spawn()?;
///
/// let mut consumer = Command::new("findstr.exe");
/// consumer.arg("piped").stdin(first.stdout.take().expect("stdout is piped"));
/// let output = consumer.output()?;
/// first.wait()?;
/// assert!(String::from_utf8_lossy(&output.stdout).contains("piped"));
/// # Ok::<(), windows_spawn::SpawnError>(())
/// ```
#[derive(Debug)]
pub struct ChildStdout {
    handle: Owned<Pipe>,
}

impl Read for ChildStdout {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        sys::read_handle(self.handle.borrow(), buffer)
    }
}

impl AsHandle for ChildStdout {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl AsRawHandle for ChildStdout {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

/// The readable parent end of a child's standard-error pipe.
#[derive(Debug)]
pub struct ChildStderr {
    handle: Owned<Pipe>,
}

impl Read for ChildStderr {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        sys::read_handle(self.handle.borrow(), buffer)
    }
}

impl AsHandle for ChildStderr {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl AsRawHandle for ChildStderr {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl From<ChildStdin> for OwnedHandle {
    fn from(stream: ChildStdin) -> Self {
        stream.handle.into_inner()
    }
}

impl From<ChildStdout> for OwnedHandle {
    fn from(stream: ChildStdout) -> Self {
        stream.handle.into_inner()
    }
}

impl From<ChildStderr> for OwnedHandle {
    fn from(stream: ChildStderr) -> Self {
        stream.handle.into_inner()
    }
}

impl From<ChildStdin> for Stdio {
    fn from(stream: ChildStdin) -> Self {
        Self::from(OwnedHandle::from(stream))
    }
}

impl From<ChildStdout> for Stdio {
    fn from(stream: ChildStdout) -> Self {
        Self::from(OwnedHandle::from(stream))
    }
}

impl From<ChildStderr> for Stdio {
    fn from(stream: ChildStderr) -> Self {
        Self::from(OwnedHandle::from(stream))
    }
}

/// The crate's handle to a process it created.
///
/// Until the main thread has run, dropping the handle terminates the process, so a spawn that fails, or a suspended child that is dropped, leaves nothing running.
#[derive(Debug)]
pub(crate) struct ProcessHandle {
    handle: Owned<Process>,
    unstarted: bool,
}

impl ProcessHandle {
    /// Owns a process whose main thread hasn't run.
    pub(crate) const fn unstarted(handle: Owned<Process>) -> Self {
        Self {
            handle,
            unstarted: true,
        }
    }

    /// Records that the main thread runs, so dropping the handle leaves the process running.
    pub(crate) fn mark_started(&mut self) {
        self.unstarted = false;
    }

    pub(crate) fn borrow(&self) -> Borrowed<'_, Process> {
        self.handle.borrow()
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if self.unstarted {
            drop(sys::terminate_process(self.handle.borrow(), 1));
        }
    }
}

impl AsHandle for ProcessHandle {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// A running or exited process, with the API of [`std::process::Child`].
///
/// The handle from [`AsHandle`] has full access, including `SYNCHRONIZE`, and stays valid while this value lives,
/// so it can be waited on together with other objects.
/// Dropping the value leaves the process running, unless [`crate::Command::own_process_tree`] made it the owner of the tree.
#[derive(Debug)]
pub struct Child {
    /// Declared first, so a process that never ran ends with the exit code of a termination before its Job closes.
    process: ProcessHandle,
    /// Declared before the pipes, so closing the Job ends the tree while the pipes still hold their ends.
    kill_job: Option<Job>,
    /// A pipe connected to the child's standard input, when requested.
    pub stdin: Option<ChildStdin>,
    /// A pipe connected to the child's standard output, when requested.
    pub stdout: Option<ChildStdout>,
    /// A pipe connected to the child's standard error, when requested.
    pub stderr: Option<ChildStderr>,
    pid: u32,
    exit: Option<ExitStatus>,
}

impl Child {
    pub(crate) fn new(
        process: ProcessHandle,
        pid: u32,
        kill_job: Option<Job>,
        stdin: Option<Owned<Pipe>>,
        stdout: Option<Owned<Pipe>>,
        stderr: Option<Owned<Pipe>>,
    ) -> Self {
        Self {
            process,
            kill_job,
            stdin: stdin.map(|handle| ChildStdin { handle }),
            stdout: stdout.map(|handle| ChildStdout { handle }),
            stderr: stderr.map(|handle| ChildStderr { handle }),
            pid,
            exit: None,
        }
    }

    /// Returns the process identifier captured at creation.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.pid
    }

    /// Forces the child to exit with code 1, and returns `Ok` if it has already exited, as [`std::process::Child::kill`] does.
    ///
    /// When the child owns its tree ([`crate::Command::own_process_tree`]), this terminates every process in the tree: the child and its descendants.
    /// Otherwise it terminates the child alone.
    /// Termination completes asynchronously; [`Self::wait`] or [`Self::wait_tree`] waits for it.
    ///
    /// # Errors
    ///
    /// Returns the Windows error from `TerminateJobObject`, or from `TerminateProcess` while the process still runs.
    pub fn kill(&mut self) -> Result<()> {
        if let Some(job) = &self.kill_job {
            return job.terminate(1);
        }
        if self.exit.is_some() {
            return Ok(());
        }
        let Err(error) = sys::terminate_process(self.process.borrow(), 1) else {
            return Ok(());
        };
        if matches!(sys::try_wait_process(self.process.borrow()), Ok(true)) {
            return Ok(());
        }
        Err(SpawnError::windows(
            Phase::Runtime,
            Operation::TerminateProcess,
            error,
        ))
    }

    /// Waits for exit and caches the status.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if waiting or retrieving the exit code fails.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        if let Some(status) = self.exit {
            return Ok(status);
        }
        drop(self.stdin.take());
        windows(
            Phase::Runtime,
            Operation::WaitProcess,
            sys::wait_process(self.process.borrow()),
        )?;
        let status = self.exit_status()?;
        self.exit = Some(status);
        Ok(status)
    }

    /// Checks for exit without blocking, returning the cached status thereafter.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if querying the process or its exit code fails.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        let exited = windows(
            Phase::Runtime,
            Operation::WaitProcess,
            sys::try_wait_process(self.process.borrow()),
        )?;
        if !exited {
            return Ok(None);
        }
        let status = self.exit_status()?;
        self.exit = Some(status);
        Ok(self.exit)
    }

    fn exit_status(&self) -> Result<ExitStatus> {
        windows(
            Phase::Runtime,
            Operation::QueryExitCode,
            sys::exit_status(self.process.borrow()),
        )
    }

    /// Waits until the child and, when it owns its tree, every process in the tree has exited, and returns the child's status.
    ///
    /// After [`Self::kill`], it returns once no process of the tree can hold a file or a directory open, so the caller can delete them.
    /// Without [`crate::Command::own_process_tree`], it waits for the child alone, as [`Self::wait`] does.
    /// Like [`Self::wait`], it closes the child's standard input first and waits only for exits,
    /// so a descendant that never exits keeps it waiting.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if listing the tree's processes, opening one, or waiting fails.
    pub fn wait_tree(&mut self) -> Result<ExitStatus> {
        if let Some(job) = &self.kill_job {
            loop {
                let ids = windows(
                    Phase::Runtime,
                    Operation::QueryJob,
                    sys::job_process_ids(job.borrow()),
                )?;
                let mut members = Vec::with_capacity(ids.len());
                for id in ids {
                    members.extend(windows(
                        Phase::Runtime,
                        Operation::OpenProcess,
                        sys::open_job_process(id, job.borrow()),
                    )?);
                }
                drop(self.stdin.take());
                if members.is_empty() {
                    break;
                }
                for member in members {
                    windows(
                        Phase::Runtime,
                        Operation::WaitProcess,
                        sys::wait_process(member.borrow()),
                    )?;
                }
            }
        }
        self.wait()
    }

    /// Waits while draining both output pipes concurrently.
    ///
    /// When the child owns its tree, the descendants are terminated after the child exits and before the readers are joined,
    /// so the pipes reach EOF even if a descendant holds a writer.
    ///
    /// # Errors
    ///
    /// Returns a Windows error from process waiting, pipe reading, or Job termination.
    pub fn wait_with_output(mut self) -> Result<Output> {
        drop(self.stdin.take());

        let stdout_reader = self
            .stdout
            .take()
            .map(|stream| thread::spawn(move || drain_output(stream.handle.borrow())));
        let stderr_reader = self
            .stderr
            .take()
            .map(|stream| thread::spawn(move || drain_output(stream.handle.borrow())));

        let status = self.wait();
        let termination = self
            .kill_job
            .as_ref()
            .map_or(Ok(()), |job| job.terminate(1));
        let (stdout, stderr) = join_readers(stdout_reader, stderr_reader)?;
        termination?;

        Ok(Output {
            status: status?,
            stdout,
            stderr,
        })
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.process.as_handle()
    }
}

impl AsRawHandle for Child {
    fn as_raw_handle(&self) -> RawHandle {
        self.process.as_handle().as_raw_handle()
    }
}

fn drain_output(handle: Borrowed<'_, Pipe>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = windows(
            Phase::Runtime,
            Operation::ReadPipe,
            sys::read_handle(handle, &mut buffer),
        )?;
        let Some(read) = std::num::NonZeroUsize::new(read) else {
            return Ok(bytes);
        };
        bytes.extend(buffer.iter().take(read.get()));
    }
}

/// An output reader thread, which returns what it drained.
type Reader = thread::JoinHandle<Result<Vec<u8>>>;

fn join_reader(reader: Option<Reader>) -> Result<Vec<u8>> {
    match reader {
        Some(reader) => reader.join().map_err(|_payload| {
            SpawnError::windows(
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

/// A process whose main thread has not yet been resumed.
///
/// Dropping this value without resuming terminates the process.
/// The process is already in every Job requested through [`crate::SpawnOptions`].
/// `resume` consumes the value, so a second resume does not compile:
///
/// ```compile_fail
/// use windows_spawn::Command;
///
/// let mut command = Command::new("cmd.exe");
/// let suspended = command.spawn_suspended().unwrap();
/// let _child = suspended.resume().unwrap();
/// let _second = suspended.resume().unwrap();
/// ```
#[derive(Debug)]
#[must_use = "dropping a suspended child terminates it"]
pub struct SuspendedChild {
    child: Child,
    main_thread: Owned<Thread>,
}

impl SuspendedChild {
    pub(crate) const fn new(child: Child, main_thread: Owned<Thread>) -> Self {
        Self { child, main_thread }
    }

    /// Returns the process identifier captured at creation.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.child.id()
    }

    /// Borrows the suspended process's main thread handle.
    ///
    /// Available for thread configuration and inspection before [`Self::resume`].
    #[must_use]
    pub fn main_thread_handle(&self) -> BorrowedHandle<'_> {
        self.main_thread.as_handle()
    }

    /// Resumes the main thread and transitions to an ordinary [`Child`].
    ///
    /// # Errors
    ///
    /// Returns a Windows error if the main thread can't be resumed,
    /// and one of kind [`io::ErrorKind::InvalidData`] if another thread suspended it again, so its suspend count was not one.
    /// The process is terminated in both cases.
    pub fn resume(self) -> Result<Child> {
        let Self {
            mut child,
            main_thread,
        } = self;
        let previous = windows(
            Phase::Resume,
            Operation::ResumeThread,
            sys::resume_thread(main_thread.borrow()),
        )?;
        if previous != 1 {
            return Err(SpawnError::unexpected_suspend_count(previous));
        }
        child.process.mark_started();
        Ok(child)
    }
}

impl AsHandle for SuspendedChild {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.child.as_handle()
    }
}

impl AsRawHandle for SuspendedChild {
    fn as_raw_handle(&self) -> RawHandle {
        self.child.as_raw_handle()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use super::*;

    /// Named to run first, so a running spawn that stays suspended fails before a test waits on it.
    #[test]
    fn a_running_spawn_stays_suspended_until_commit_resumes_it() {
        let mut command = crate::Command::new("cmd.exe");
        command
            .args(["/D", "/C", "set /p x="])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let plan = crate::plan::SpawnPlan::new_running(
            &command,
            crate::SpawnOptions::new(),
            crate::plan::IoMode::Spawn,
        )
        .unwrap();
        let transaction = crate::transaction::SpawnTransaction::new(&plan).unwrap();
        let thread = sys::test_support::duplicate(transaction.main_thread());
        assert_eq!(sys::test_support::suspend_count(thread.as_handle()), 1);
        let mut child = transaction.commit_child().unwrap();
        assert_eq!(sys::test_support::suspend_count(thread.as_handle()), 0);
        child.wait().unwrap();
    }

    #[test]
    fn kill_succeeds_after_an_unobserved_exit_and_ends_the_tree_it_owns() {
        let mut command = crate::Command::new("cmd.exe");
        command
            .args(["/D", "/C", "set /p x="])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut exited = command.spawn().unwrap();
        let mut watched = sys::test_support::ProcessExitGuard::watch(exited.as_handle());
        drop(exited.stdin.take());
        watched.exit_code();
        exited.kill().unwrap();
        assert!(exited.try_wait().unwrap().is_some());
        exited.kill().unwrap();

        let mut owner = command.own_process_tree().spawn().unwrap();
        let mut ended = sys::test_support::ProcessExitGuard::watch(owner.as_handle());
        owner.kill().unwrap();
        assert_eq!(ended.exit_code(), 1);
        owner.kill().unwrap();
        assert_eq!(owner.wait_tree().unwrap().code(), Some(1));
        assert_eq!(owner.wait().unwrap().code(), Some(1));
    }

    #[test]
    fn wait_tree_waits_for_a_running_tree_and_is_wait_without_one() {
        let mut command = crate::Command::new("cmd.exe");
        command
            .args(["/D", "/C", "set /p x="])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .console(crate::ConsoleMode::NoConsole);
        let mut alone = command.spawn().unwrap();
        assert!(alone.wait_tree().unwrap().code().is_some());

        let mut owner = command.own_process_tree().spawn().unwrap();
        let job = owner.kill_job.as_ref().unwrap().borrow();
        let ids = sys::job_process_ids(job).unwrap();
        assert_eq!(ids, [usize::try_from(owner.id()).unwrap()]);
        let member = sys::open_job_process(ids[0], job).unwrap().unwrap();
        assert!(!sys::try_wait_process(member.borrow()).unwrap());
        let outsider = usize::try_from(std::process::id()).unwrap();
        assert!(sys::open_job_process(outsider, job).unwrap().is_none());
        assert!(sys::open_job_process(0, job).unwrap().is_none());
        assert!(owner.wait_tree().unwrap().code().is_some());
        assert_eq!(
            sys::job_process_ids(owner.kill_job.as_ref().unwrap().borrow()).unwrap(),
            Vec::<usize>::new()
        );
    }

    /// A process that survives its handle reads the end of its input and exits 5, and a terminated one exits 1.
    #[test]
    fn only_a_process_that_never_ran_ends_when_its_handle_drops() {
        let mut command = crate::Command::new("cmd.exe");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg("set /p x=&exit /b 5")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let suspended = command.spawn_suspended().unwrap();
        let mut unstarted = sys::test_support::ProcessExitGuard::watch(suspended.as_handle());
        let thread = sys::test_support::duplicate(suspended.main_thread_handle());
        drop(suspended);
        sys::test_support::tempt_resume(thread.as_handle());
        assert_eq!(unstarted.exit_code(), 1);

        for child in [
            command.spawn().unwrap(),
            command.spawn_suspended().unwrap().resume().unwrap(),
        ] {
            let mut started = sys::test_support::ProcessExitGuard::watch(child.as_handle());
            drop(child);
            assert_eq!(started.exit_code(), 5);
        }
    }

    #[test]
    fn reader_errors_reach_the_caller() {
        let (reader, writer) = sys::test_support::pipe();
        assert!(drain_output(Borrowed::adopt(writer.as_handle())).is_err());
        drop(reader);

        let mut exited = crate::Command::new("cmd.exe");
        exited
            .args(["/D", "/C", "exit /b 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let source = exited.spawn().unwrap();
        let (unreadable_reader, unreadable) = sys::test_support::pipe();
        let mut process = ProcessHandle::unstarted(Owned::adopt(sys::test_support::duplicate(
            source.as_handle(),
        )));
        process.mark_started();
        let child = Child::new(
            process,
            source.id(),
            None,
            None,
            Some(Owned::adopt(unreadable)),
            None,
        );
        assert!(child.wait_with_output().is_err());
        drop(unreadable_reader);

        let failed = thread::spawn(|| -> Result<Vec<u8>> {
            Err(SpawnError::windows(
                Phase::Runtime,
                Operation::ReadPipe,
                io::Error::other("read"),
            ))
        });
        let succeeded = thread::spawn(|| Ok(b"out".to_vec()));
        assert!(join_readers(Some(succeeded), Some(failed)).is_err());
    }

    fn operation(result: Result<impl Sized>) -> Option<Operation> {
        match result {
            Err(SpawnError::Windows(error)) => Some(error.operation()),
            Ok(_) | Err(SpawnError::Validation(_) | SpawnError::Cleanup(_)) => None,
        }
    }

    #[test]
    fn absent_and_panicked_output_readers_become_results() {
        assert_eq!(join_reader(None).unwrap(), Vec::<u8>::new());
        let panicked = thread::spawn(|| -> Result<Vec<u8>> { panic!("reader panic") });
        assert_eq!(
            operation(join_reader(Some(panicked))),
            Some(Operation::JoinOutputReader)
        );
    }

    #[test]
    fn both_output_readers_are_joined_when_the_first_panics() {
        let joined = Arc::new(AtomicBool::new(false));
        let stdout = thread::spawn(|| -> Result<Vec<u8>> { panic!("stdout panic") });
        let stderr_joined = Arc::clone(&joined);
        let stderr = thread::spawn(move || {
            stderr_joined.store(true, Ordering::Release);
            Ok(Vec::new())
        });

        assert_eq!(
            operation(join_readers(Some(stdout), Some(stderr))),
            Some(Operation::JoinOutputReader)
        );
        assert!(joined.load(Ordering::Acquire));
    }
}
