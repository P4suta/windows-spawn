use std::fmt;
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::num::NonZeroIsize;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle as SystemOwnedHandle};

use crate::child::Child;
use crate::resource::{CurrentTable, IoKind, JobKind, OwnedHandle, ProcessKind};
use crate::sys;
use crate::trace::{self, ResourceKind};

fn typed_io<T>(
    phase: crate::Phase,
    operation: crate::Operation,
    resource: ResourceKind,
    call: impl FnOnce() -> io::Result<T>,
) -> crate::Result<T> {
    trace::io(phase, operation, resource, call)
        .map_err(|error| crate::Error::windows(phase, operation, error))
}

/// Describes a standard stream source while keeping any supplied handle owned.
pub struct Stdio {
    pub(crate) inner: StdioInner,
}

pub(crate) enum StdioInner {
    Inherit,
    Null,
    Piped,
    Owned(OwnedHandle<IoKind, CurrentTable>),
}

impl fmt::Debug for Stdio {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.inner {
            StdioInner::Inherit => "Inherit",
            StdioInner::Null => "Null",
            StdioInner::Piped => "Piped",
            StdioInner::Owned(_) => "Owned",
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

    /// Creates an anonymous pipe and returns the caller's end on [`Child`].
    #[must_use]
    pub const fn piped() -> Self {
        Self {
            inner: StdioInner::Piped,
        }
    }

    /// Duplicates a borrowed handle into private, non-inheritable ownership.
    ///
    /// The original handle may be closed immediately after this call.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error if duplication fails.
    pub fn from_borrowed<T: AsHandle>(source: &T) -> crate::Result<Self> {
        Self::from_borrowed_with(source, |handle| {
            sys::duplicate_local(handle, sys::Inheritability::Private)
        })
    }

    fn from_borrowed_with<T: AsHandle>(
        source: &T,
        duplicate: impl FnOnce(BorrowedHandle<'_>) -> io::Result<SystemOwnedHandle>,
    ) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::DuplicateLocalHandle,
            ResourceKind::Handle,
            || duplicate(source.as_handle()),
        )
        .map(Self::from)
    }
}

impl From<SystemOwnedHandle> for Stdio {
    fn from(handle: SystemOwnedHandle) -> Self {
        Self {
            inner: StdioInner::Owned(OwnedHandle::from_system(handle)),
        }
    }
}

impl From<File> for Stdio {
    fn from(file: File) -> Self {
        Self::from(SystemOwnedHandle::from(file))
    }
}

/// A process handle validated for use as `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`.
#[derive(Debug)]
pub struct ParentProcess {
    handle: OwnedHandle<ProcessKind, CurrentTable>,
}

impl ParentProcess {
    /// Opens a process with process-creation and handle-duplication rights.
    ///
    /// # Errors
    ///
    /// Returns an error if the PID cannot be opened with the required rights.
    pub fn open(pid: u32) -> crate::Result<Self> {
        Self::open_with(pid, sys::open_parent_process)
    }

    fn open_with(
        pid: u32,
        open: impl FnOnce(u32) -> io::Result<SystemOwnedHandle>,
    ) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::OpenProcess,
            ResourceKind::Process,
            || open(pid),
        )
        .map(|handle| Self {
            handle: OwnedHandle::from_system(handle),
        })
    }

    /// Adopts and validates an existing process handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle does not identify a process.
    pub fn from_handle(handle: SystemOwnedHandle) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::OpenProcess,
            ResourceKind::Process,
            || sys::validate_process_handle(handle.as_handle()),
        )?;
        Ok(Self {
            handle: OwnedHandle::from_system(handle),
        })
    }
}

impl AsHandle for ParentProcess {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// An owned Windows Job object.
#[derive(Debug)]
pub struct Job {
    handle: OwnedHandle<JobKind, CurrentTable>,
}

impl Job {
    pub(crate) fn from_system(handle: SystemOwnedHandle) -> Self {
        Self {
            handle: OwnedHandle::from_system(handle),
        }
    }

    /// Creates an unnamed Job object.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error if Job creation fails.
    pub fn create() -> crate::Result<Self> {
        Self::create_with(sys::create_job)
    }

    fn create_with(create: impl FnOnce() -> io::Result<SystemOwnedHandle>) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::CreateJob,
            ResourceKind::Job,
            create,
        )
        .map(Self::from_system)
    }

    /// Adopts an existing handle after verifying it is a Job handle.
    ///
    /// # Errors
    ///
    /// Returns an error if Job limit information cannot be queried.
    pub fn from_handle(handle: SystemOwnedHandle) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::ConfigureJob,
            ResourceKind::Job,
            || sys::validate_job_handle(handle.as_handle()),
        )?;
        Ok(Self {
            handle: OwnedHandle::from_system(handle),
        })
    }

    /// Creates an independent duplicate of this Job handle.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error if duplication fails.
    pub fn duplicate(&self) -> crate::Result<Self> {
        self.duplicate_with(|handle| sys::duplicate_local(handle, sys::Inheritability::Private))
    }

    fn duplicate_with(
        &self,
        duplicate: impl FnOnce(BorrowedHandle<'_>) -> io::Result<SystemOwnedHandle>,
    ) -> crate::Result<Self> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::DuplicateLocalHandle,
            ResourceKind::Handle,
            || duplicate(self.handle.as_handle()),
        )
        .map(|handle| Self {
            handle: OwnedHandle::from_system(handle),
        })
    }

    /// Assigns an existing child to the Job.
    ///
    /// # Errors
    ///
    /// Returns an error when Windows rejects the Job assignment.
    pub fn assign(&self, child: &Child) -> crate::Result<()> {
        typed_io(
            crate::Phase::Runtime,
            crate::Operation::AssignJob,
            ResourceKind::Job,
            || sys::assign_job(self.handle.as_handle(), child.process_handle()),
        )
    }

    /// Terminates every process in the Job with `exit_code`.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error if Job termination fails.
    pub fn terminate(&self, exit_code: u32) -> crate::Result<()> {
        typed_io(
            crate::Phase::Cleanup,
            crate::Operation::TerminateJob,
            ResourceKind::Job,
            || sys::terminate_job(self.handle.as_handle(), exit_code),
        )
    }

    /// Selects close behavior without overwriting other Job limits.
    ///
    /// # Errors
    ///
    /// Returns an error if querying or updating Job limits fails.
    pub fn set_close_policy(&self, policy: crate::JobClosePolicy) -> crate::Result<()> {
        typed_io(
            crate::Phase::Preparation,
            crate::Operation::ConfigureJob,
            ResourceKind::Job,
            || sys::set_job_close_policy(self.handle.as_handle(), policy),
        )
    }
}

impl AsHandle for Job {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// A nonzero pseudoconsole value borrowed from its owner.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BorrowedPseudoConsole<'a> {
    pub(crate) value: NonZeroIsize,
    owner: PhantomData<&'a ()>,
}

impl<'a> BorrowedPseudoConsole<'a> {
    /// Borrows a raw `HPCON` value for no longer than `owner` remains alive.
    ///
    /// # Safety
    ///
    /// `value` must identify an open pseudoconsole owned by `owner`. The owner
    /// must keep that value open and unchanged for the returned lifetime.
    #[allow(unsafe_code)]
    pub unsafe fn from_raw<T: ?Sized>(value: NonZeroIsize, owner: &'a T) -> Self {
        let _ = owner;
        Self {
            value,
            owner: PhantomData,
        }
    }
}

/// A borrowed pseudoconsole capability.
///
/// # Safety
///
/// Implementations must return a valid, nonzero `HPCON` and keep it open and
/// unchanged for the full lifetime of every borrow passed to
/// [`crate::SpawnOptions::pseudo_console`]. The implementation retains
/// ownership: windows-spawn borrows the value for process creation and never closes
/// or releases it.
#[allow(unsafe_code)]
pub unsafe trait AsPseudoConsole {
    /// Returns a typed borrow of the owned pseudoconsole.
    fn as_pseudo_console(&self) -> BorrowedPseudoConsole<'_>;
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn borrowed_pseudoconsole_for_test(_owner: &()) -> BorrowedPseudoConsole<'_> {
    let value = NonZeroIsize::new(1).unwrap_or(NonZeroIsize::MIN);
    BorrowedPseudoConsole {
        value,
        owner: PhantomData,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::os::windows::io::AsRawHandle;

    use super::*;

    #[test]
    fn owned_handle_adoption_validates_resource_kind() {
        let synthetic = typed_io::<()>(
            crate::Phase::Preparation,
            crate::Operation::OpenProcess,
            ResourceKind::Process,
            || Err(io::Error::from_raw_os_error(5)),
        )
        .unwrap_err();
        assert!(matches!(synthetic, crate::Error::Windows(_)));
        let mut host = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "ping -n 5 127.0.0.1 >nul"])
            .spawn()
            .unwrap();
        let parent = ParentProcess::open(host.id()).unwrap();
        assert!(format!("{parent:?}").contains("ParentProcess"));
        let adopted_parent = ParentProcess::from_handle(
            sys::duplicate_local(host.as_handle(), sys::Inheritability::Private).unwrap(),
        )
        .unwrap();
        assert_ne!(
            adopted_parent.as_handle().as_raw_handle(),
            std::ptr::null_mut()
        );

        let job = Job::create().unwrap();
        let duplicate = job.duplicate().unwrap();
        let adopted_job = Job::from_handle(
            sys::duplicate_local(duplicate.as_handle(), sys::Inheritability::Private).unwrap(),
        )
        .unwrap();
        adopted_job
            .set_close_policy(crate::JobClosePolicy::TerminateProcesses)
            .unwrap();
        adopted_job
            .set_close_policy(crate::JobClosePolicy::PreserveProcesses)
            .unwrap();

        let file = File::open("NUL").unwrap();
        assert!(
            Stdio::from_borrowed_with(&file, |_| { Err(io::Error::from_raw_os_error(5)) }).is_err()
        );
        assert!(
            ParentProcess::open_with(u32::MAX, |_| { Err(io::Error::from_raw_os_error(5)) })
                .is_err()
        );
        assert!(Job::create_with(|| Err(io::Error::from_raw_os_error(5))).is_err());
        assert!(job
            .duplicate_with(|_| Err(io::Error::from_raw_os_error(5)))
            .is_err());
        let not_process =
            sys::duplicate_local(file.as_handle(), sys::Inheritability::Private).unwrap();
        assert!(ParentProcess::from_handle(not_process).is_err());
        let not_job = sys::duplicate_local(file.as_handle(), sys::Inheritability::Private).unwrap();
        assert!(Job::from_handle(not_job).is_err());
        let _ = host.kill();
        let _ = host.wait();
    }
}
