//! Owned capability wrappers used by process creation.

use std::fmt;
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};

use crate::error::{Operation, Phase, Result, SpawnError};
use crate::sys;

/// Runs one Windows operation and types its failure with the phase and operation.
pub(crate) fn windows<T>(phase: Phase, operation: Operation, outcome: io::Result<T>) -> Result<T> {
    outcome.map_err(|error| SpawnError::windows(phase, operation, error))
}

/// Describes a standard stream source while keeping any supplied handle owned.
///
/// Like [`std::process::Stdio`], it converts from a [`File`], an [`OwnedHandle`], a child's pipe end such as [`crate::ChildStdout`],
/// and [`io::stdout`] or [`io::stderr`], which the spawn reads when it runs.
pub struct Stdio {
    pub(crate) inner: StdioInner,
}

pub(crate) enum StdioInner {
    Inherit,
    Null,
    Piped,
    Owned(OwnedHandle),
    /// The caller's standard output, in whichever slot the child receives it.
    CallerStdout,
    /// The caller's standard error, in whichever slot the child receives it.
    CallerStderr,
}

impl fmt::Debug for Stdio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.inner {
            StdioInner::Inherit => "Inherit",
            StdioInner::Null => "Null",
            StdioInner::Piped => "Piped",
            StdioInner::Owned(_) => "Owned",
            StdioInner::CallerStdout => "Stdout",
            StdioInner::CallerStderr => "Stderr",
        };
        formatter.debug_tuple("Stdio").field(&name).finish()
    }
}

impl Stdio {
    /// Inherits the corresponding standard stream from the caller.
    #[must_use]
    pub const fn inherit() -> Self {
        Self {
            inner: StdioInner::Inherit,
        }
    }

    /// Connects the stream to the Windows null device.
    #[must_use]
    pub const fn null() -> Self {
        Self {
            inner: StdioInner::Null,
        }
    }

    /// Creates an anonymous pipe and returns the caller's end on [`crate::Child`].
    #[must_use]
    pub const fn piped() -> Self {
        Self {
            inner: StdioInner::Piped,
        }
    }

    /// Duplicates a borrowed handle into private, non-inheritable ownership.
    ///
    /// The original handle may be closed afterwards.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if duplication fails.
    pub fn from_borrowed<T: AsHandle>(source: &T) -> Result<Self> {
        windows(
            Phase::Preparation,
            Operation::DuplicateLocalHandle,
            sys::duplicate_local(source.as_handle(), false),
        )
        .map(Self::from)
    }
}

impl From<OwnedHandle> for Stdio {
    fn from(handle: OwnedHandle) -> Self {
        Self {
            inner: StdioInner::Owned(handle),
        }
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Self {
        Self::from(OwnedHandle::from(file))
    }
}

impl From<io::Stdout> for Stdio {
    /// Gives the child the caller's standard output, as the spawn finds it.
    fn from(_: io::Stdout) -> Self {
        Self {
            inner: StdioInner::CallerStdout,
        }
    }
}

impl From<io::Stderr> for Stdio {
    /// Gives the child the caller's standard error, as the spawn finds it.
    fn from(_: io::Stderr) -> Self {
        Self {
            inner: StdioInner::CallerStderr,
        }
    }
}

/// A process handle validated for use as `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`.
#[derive(Debug)]
pub struct ParentProcess {
    handle: OwnedHandle,
}

impl ParentProcess {
    /// Opens a process with process-creation and handle-duplication rights.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if the PID cannot be opened with the required rights.
    pub fn open(pid: u32) -> Result<Self> {
        windows(
            Phase::Preparation,
            Operation::OpenProcess,
            sys::open_parent_process(pid),
        )
        .map(|handle| Self { handle })
    }

    /// Adopts and validates an existing process handle.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if the handle does not identify a process.
    pub fn from_handle(handle: OwnedHandle) -> Result<Self> {
        windows(
            Phase::Preparation,
            Operation::OpenProcess,
            sys::validate_process_handle(handle.as_handle()),
        )?;
        Ok(Self { handle })
    }
}

impl AsHandle for ParentProcess {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl AsRawHandle for ParentProcess {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl From<ParentProcess> for OwnedHandle {
    fn from(parent: ParentProcess) -> Self {
        parent.handle
    }
}

/// An owned Windows Job object.
#[derive(Debug)]
pub struct Job {
    handle: OwnedHandle,
}

impl Job {
    /// Creates an unnamed Job object.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if Job creation fails.
    pub fn create() -> Result<Self> {
        windows(Phase::Preparation, Operation::CreateJob, sys::create_job())
            .map(|handle| Self { handle })
    }

    /// Adopts an existing handle after verifying it is a Job handle.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if Job limit information cannot be queried.
    pub fn from_handle(handle: OwnedHandle) -> Result<Self> {
        windows(
            Phase::Preparation,
            Operation::QueryJob,
            sys::validate_job_handle(handle.as_handle()),
        )?;
        Ok(Self { handle })
    }

    /// Creates an independent duplicate of this Job handle.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if duplication fails.
    pub fn duplicate(&self) -> Result<Self> {
        windows(
            Phase::Preparation,
            Operation::DuplicateLocalHandle,
            sys::duplicate_local(self.handle.as_handle(), false),
        )
        .map(|handle| Self { handle })
    }

    /// Assigns a running process to the Job: a [`crate::Child`], a [`crate::SuspendedChild`], a [`std::process::Child`], or any process handle.
    ///
    /// A spawn attaches Jobs before the child runs with [`crate::SpawnOptions::job`]; this is for a process that already exists.
    ///
    /// # Errors
    ///
    /// Returns a Windows error when Windows rejects the assignment, for example because `process` is no process handle.
    pub fn assign<P: AsHandle + ?Sized>(&self, process: &P) -> Result<()> {
        windows(
            Phase::Runtime,
            Operation::AssignJob,
            sys::assign_job(self.handle.as_handle(), process.as_handle()),
        )
    }

    /// Terminates every process in the Job with `exit_code`.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if Job termination fails.
    pub fn terminate(&self, exit_code: u32) -> Result<()> {
        windows(
            Phase::Runtime,
            Operation::TerminateJob,
            sys::terminate_job(self.handle.as_handle(), exit_code),
        )
    }

    /// Sets whether closing the last handle of the Job terminates its processes, without changing other Job limits.
    ///
    /// This is the Job's `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` limit.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if querying or updating Job limits fails.
    pub fn set_kill_on_close(&self, kill: bool) -> Result<()> {
        windows(
            Phase::Preparation,
            Operation::ConfigureJob,
            sys::set_job_kill_on_close(self.handle.as_handle(), kill),
        )
    }
}

impl AsHandle for Job {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl AsRawHandle for Job {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl From<Job> for OwnedHandle {
    fn from(job: Job) -> Self {
        job.handle
    }
}

/// A borrowed pseudoconsole (`HPCON`), valid for the lifetime `'a`.
///
/// It plays the part [`BorrowedHandle`] plays for handles: [`AsPseudoConsole`] lends one from an owning value,
/// and [`Self::borrow_raw`] makes one from a raw `HPCON`, such as one that `CreatePseudoConsole` returned.
/// windows-spawn never closes it; its owner decides when the pseudoconsole closes and when its pipes reach EOF.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BorrowedPseudoConsole<'a> {
    pub(crate) raw: isize,
    pub(crate) owner: PhantomData<&'a ()>,
}

impl BorrowedPseudoConsole<'_> {
    /// Returns the raw `HPCON`.
    #[must_use]
    pub const fn as_raw(self) -> isize {
        self.raw
    }
}

impl<'a, T: AsPseudoConsole + ?Sized> From<&'a T> for BorrowedPseudoConsole<'a> {
    fn from(owner: &'a T) -> Self {
        owner.as_pseudo_console()
    }
}

/// A value that owns or borrows a pseudoconsole and lends it out, as [`AsHandle`] does for handles.
///
/// A terminal library implements it once for its pseudoconsole type with [`BorrowedPseudoConsole::borrow_raw`];
/// an application then passes a reference to that value to [`crate::SpawnOptions::pseudo_console`].
pub trait AsPseudoConsole {
    /// Borrows the pseudoconsole.
    fn as_pseudo_console(&self) -> BorrowedPseudoConsole<'_>;
}

impl AsPseudoConsole for BorrowedPseudoConsole<'_> {
    fn as_pseudo_console(&self) -> BorrowedPseudoConsole<'_> {
        *self
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsRawHandle;

    use super::*;

    const ACCESS_DENIED: i32 = 5;

    #[test]
    fn stored_duplicates_are_private() -> Result<()> {
        use crate::sys::test_support::is_inheritable;

        let nul = File::open("NUL").map_err(|error| {
            SpawnError::windows(Phase::Preparation, Operation::OpenNullDevice, error)
        })?;
        let stdio = Stdio::from_borrowed(&nul)?;
        let StdioInner::Owned(owned) = &stdio.inner else {
            panic!("from_borrowed stores an owned handle");
        };
        assert!(!is_inheritable(owned.as_handle()));
        let duplicate = Job::create()?.duplicate()?;
        assert!(!is_inheritable(duplicate.as_handle()));
        Ok(())
    }

    #[test]
    fn windows_failures_keep_their_phase_and_operation() {
        let failed = windows::<()>(
            Phase::Preparation,
            Operation::OpenProcess,
            Err(io::Error::from_raw_os_error(ACCESS_DENIED)),
        );
        let Err(SpawnError::Windows(error)) = failed else {
            panic!("a failed call becomes a Windows error");
        };
        assert_eq!(error.phase(), Phase::Preparation);
        assert_eq!(error.operation(), Operation::OpenProcess);
        assert_eq!(error.raw_os_error(), Some(ACCESS_DENIED));
    }

    #[test]
    fn owned_handle_adoption_validates_resource_kind() {
        let mut host_command = crate::Command::new("cmd.exe");
        host_command
            .args(["/D", "/C", "exit /b 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let host = host_command.spawn_suspended().unwrap();
        let _host_guard = sys::test_support::ProcessExitGuard::watch(host.as_handle());
        let parent = ParentProcess::open(host.id()).unwrap();
        assert!(format!("{parent:?}").contains("ParentProcess"));
        let adopted_parent =
            ParentProcess::from_handle(sys::duplicate_local(host.as_handle(), false).unwrap())
                .unwrap();
        assert_ne!(
            adopted_parent.as_handle().as_raw_handle(),
            std::ptr::null_mut()
        );

        let job = Job::create().unwrap();
        let duplicate = job.duplicate().unwrap();
        let adopted_job = Job::from_handle(duplicate.handle).unwrap();
        adopted_job.set_kill_on_close(true).unwrap();
        adopted_job.set_kill_on_close(false).unwrap();

        let file = File::open("NUL").unwrap();
        let not_process = sys::duplicate_local(file.as_handle(), false).unwrap();
        assert!(ParentProcess::from_handle(not_process).is_err());
        let not_job = sys::duplicate_local(file.as_handle(), false).unwrap();
        assert!(Job::from_handle(not_job).is_err());
        drop(host);
    }
}
