//! The attribute list and the values it can point at.
//!
//! A `PROC_THREAD_ATTRIBUTE_LIST` is an opaque, variable-sized blob that
//! `CreateProcessW` reads through `STARTUPINFOEXW::lpAttributeList`. Building
//! one correctly means getting three things right:
//!
//! 1. **Two-phase allocation.** `InitializeProcThreadAttributeList` is called
//!    once with a null buffer to learn the required size (it "fails" with
//!    `ERROR_INSUFFICIENT_BUFFER`, which is the success path), then again with a
//!    buffer of that size. The size depends on the attribute *count*, which
//!    therefore has to be known before any attribute is added.
//! 2. **Pointer, not value.** `UpdateProcThreadAttribute` stores the pointer you
//!    give it. The pointee must outlive the `CreateProcessW` call and must not
//!    move — no `Vec` reallocation, no temporaries, no stack frame going away.
//! 3. **Deallocation.** `DeleteProcThreadAttributeList` must be called before
//!    the buffer is freed.
//!
//! [`AttributeList`] covers (1) and (3) with RAII, and (2) with the lifetime
//! parameter `'a`: every borrow handed to the builder is captured in `'a`, so
//! the list cannot outlive any value it points at. See
//! `docs/adr/0003-attribute-lifetime-model.md`.

use std::marker::PhantomData;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle};

use crate::child::Child;
use crate::error::Result;
use crate::mitigation::MitigationPolicy;

/// The `PROC_THREAD_ATTRIBUTE_*` values this crate passes to
/// `UpdateProcThreadAttribute`, taken from `windows-sys` so that the magic
/// numbers are never hand-copied out of `WinBase.h`.
mod attr {
    use windows_sys::Win32::System::Threading as sys;

    pub(super) const HANDLE_LIST: u32 = sys::PROC_THREAD_ATTRIBUTE_HANDLE_LIST;
    pub(super) const PARENT_PROCESS: u32 = sys::PROC_THREAD_ATTRIBUTE_PARENT_PROCESS;
    pub(super) const MITIGATION_POLICY: u32 = sys::PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY;
    pub(super) const JOB_LIST: u32 = sys::PROC_THREAD_ATTRIBUTE_JOB_LIST;
    pub(super) const PSEUDOCONSOLE: u32 = sys::PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE;
}

/// A borrowed Win32 `HANDLE` that is valid for `'a`.
///
/// This is a thin newtype over [`BorrowedHandle`] and exists so that a handle
/// list reads as a list of *borrows* rather than a list of integers. The
/// lifetime is what stops a handle from being closed between the moment it is
/// added to the list and the moment `CreateProcessW` reads it.
#[derive(Clone, Copy, Debug)]
pub struct RawHandleRef<'a> {
    handle: BorrowedHandle<'a>,
}

impl<'a> RawHandleRef<'a> {
    /// Wrap an already-borrowed handle.
    pub fn new(handle: BorrowedHandle<'a>) -> Self {
        todo!("wrap the borrowed handle")
    }

    /// Borrow the handle out of anything that owns one — a [`File`](std::fs::File),
    /// a pipe end, a [`Child`], a job object.
    pub fn borrow<T: AsHandle>(source: &'a T) -> Self {
        todo!("call source.as_handle() and wrap it")
    }

    /// The underlying borrowed handle.
    pub fn as_borrowed(self) -> BorrowedHandle<'a> {
        todo!("return the inner BorrowedHandle")
    }

    /// Whether the handle is marked inheritable (`HANDLE_FLAG_INHERIT`).
    ///
    /// Handles listed in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` must be
    /// inheritable *and* `bInheritHandles` must be `TRUE`; a non-inheritable
    /// handle in the list makes `CreateProcessW` fail with
    /// `ERROR_INVALID_PARAMETER`. Checking up front turns that into
    /// [`Error::Incompatible`](crate::Error::Incompatible).
    pub fn is_inheritable(self) -> Result<bool> {
        todo!("GetHandleInformation and test HANDLE_FLAG_INHERIT")
    }
}

/// An initialised `PROC_THREAD_ATTRIBUTE_LIST`.
///
/// The lifetime `'a` is the intersection of the lifetimes of every value the
/// list points at. Because the list borrows rather than owns those values, this
/// does not compile:
///
/// ```ignore
/// let list = {
///     let file = File::open("log.txt")?;
///     let handles = [RawHandleRef::borrow(&file)];
///     AttributeList::builder().inherit_handles(&handles).build()?
/// }; // error[E0597]: `file` does not live long enough
/// ```
///
/// Dropping the list calls `DeleteProcThreadAttributeList` and then frees the
/// backing allocation, in that order.
#[derive(Debug)]
pub struct AttributeList<'a> {
    /// The opaque blob `InitializeProcThreadAttributeList` writes into. Boxed
    /// (never a `Vec` that could reallocate) so its address is stable.
    buffer: Box<[u8]>,
    /// Number of attributes actually written; `CreateProcessW` does not need
    /// it, but `Debug` and the duplicate check do.
    count: usize,
    /// Ties the list to everything it points at without owning any of it.
    _values: PhantomData<&'a ()>,
}

impl<'a> AttributeList<'a> {
    /// Start building an attribute list.
    pub fn builder() -> AttributeListBuilder<'a> {
        todo!("AttributeListBuilder::new()")
    }

    /// How many attributes this list carries.
    pub fn len(&self) -> usize {
        todo!("return the attribute count")
    }

    /// Whether the list carries no attributes at all.
    ///
    /// An empty list is legal but pointless: `spawnkit` skips `STARTUPINFOEX`
    /// entirely and spawns through a plain `STARTUPINFOW` in that case.
    pub fn is_empty(&self) -> bool {
        todo!("return self.len() == 0")
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // TODO(sys): DeleteProcThreadAttributeList(self.buffer.as_mut_ptr()),
        // then let the Box free the allocation. Deliberately not `todo!()`:
        // a panicking destructor would be a landmine even in a scaffold.
    }
}

/// Builder for [`AttributeList`].
///
/// Each method records one `PROC_THREAD_ATTRIBUTE_*` value. Setting the same
/// attribute twice is rejected at [`build`](AttributeListBuilder::build) with
/// [`Error::DuplicateAttribute`](crate::Error::DuplicateAttribute) rather than
/// silently overwriting, because `UpdateProcThreadAttribute` itself rejects
/// duplicates and the failure is much easier to read here.
#[derive(Debug, Default)]
pub struct AttributeListBuilder<'a> {
    handles: Option<&'a [RawHandleRef<'a>]>,
    parent: Option<&'a ParentProcess>,
    mitigation: Option<MitigationPolicy>,
    job: Option<&'a Job>,
    pcon: Option<&'a Pcon>,
}

impl<'a> AttributeListBuilder<'a> {
    /// An empty builder.
    pub fn new() -> Self {
        todo!("Self::default()")
    }

    /// `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` — the exact set of handles the child
    /// may inherit.
    ///
    /// Without this attribute, `bInheritHandles = TRUE` leaks *every*
    /// inheritable handle in the process to the child, including handles opened
    /// concurrently by unrelated threads. With it, inheritance becomes an
    /// explicit list.
    ///
    /// The slice is borrowed for `'a`: it must stay put until the spawn
    /// completes, because the attribute list stores a pointer into it.
    pub fn inherit_handles(self, handles: &'a [RawHandleRef<'a>]) -> Self {
        todo!("record the handle list")
    }

    /// `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` — spawn the child as a child of
    /// another process.
    ///
    /// The new process inherits the target's handle table policy, token,
    /// affinity and job membership rather than the caller's. Note that the
    /// `HANDLE` values in [`inherit_handles`](Self::inherit_handles) are then
    /// interpreted in the *parent's* handle table, not the caller's.
    pub fn parent_process(self, parent: &'a ParentProcess) -> Self {
        todo!("record the parent process handle")
    }

    /// `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` — opt the child into process
    /// creation mitigations.
    pub fn mitigation(self, policy: MitigationPolicy) -> Self {
        todo!("record the mitigation policy word")
    }

    /// `PROC_THREAD_ATTRIBUTE_JOB_LIST` — put the child in a job object
    /// *atomically with creation*.
    ///
    /// This closes the race that `AssignProcessToJobObject` cannot: a process
    /// created suspended and assigned afterwards can still have run code (DLL
    /// notifications, TLS callbacks) before the assignment lands, and the
    /// assignment can fail outright if the child has already joined another
    /// job. See `docs/adr/0004-job-attachment.md` for when to prefer
    /// [`Job::assign`].
    pub fn attach_to_job(self, job: &'a Job) -> Self {
        todo!("record the job handle")
    }

    /// `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` — attach the child to a ConPTY.
    ///
    /// Mutually exclusive with redirecting the standard handles through
    /// `STARTUPINFOW`: the pseudoconsole *is* the child's console.
    pub fn pseudoconsole(self, pcon: &'a Pcon) -> Self {
        todo!("record the HPCON")
    }

    /// Allocate and populate the attribute list.
    ///
    /// This is where the two-phase `InitializeProcThreadAttributeList` dance
    /// happens, followed by one `UpdateProcThreadAttribute` per recorded
    /// attribute.
    pub fn build(self) -> Result<AttributeList<'a>> {
        todo!("size, allocate, initialise and populate the attribute list")
    }
}

/// A process to be used as the parent of a spawned child.
///
/// Requires `PROCESS_CREATE_PROCESS` access to the target.
#[derive(Debug)]
pub struct ParentProcess {
    handle: OwnedHandle,
}

impl ParentProcess {
    /// Open a process by PID with the access rights needed for re-parenting.
    pub fn open(pid: u32) -> Result<Self> {
        todo!("OpenProcess(PROCESS_CREATE_PROCESS, FALSE, pid)")
    }

    /// Adopt an already-open process handle.
    pub fn from_handle(handle: OwnedHandle) -> Self {
        todo!("wrap the owned handle")
    }
}

impl AsHandle for ParentProcess {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        todo!("borrow the process handle")
    }
}

/// A Windows job object.
///
/// `spawnkit` owns just enough of the job API to attach a child at creation
/// time and to bound its lifetime. Anything richer — CPU rate control, IO rate
/// control, completion ports, notification limits — belongs in `win32job` or
/// `process-wrap`, and this type is designed to interoperate with them rather
/// than to replace them: [`Job::from_handle`] adopts a handle they created, and
/// [`AsHandle`] hands one back.
#[derive(Debug)]
pub struct Job {
    handle: OwnedHandle,
}

impl Job {
    /// Create an unnamed job object.
    pub fn create() -> Result<Self> {
        todo!("CreateJobObjectW(NULL, NULL)")
    }

    /// Adopt a job handle created elsewhere (`win32job`, `process-wrap`, ...).
    pub fn from_handle(handle: OwnedHandle) -> Self {
        todo!("wrap the owned handle")
    }

    /// Set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    ///
    /// With this set, dropping the last handle to the job terminates every
    /// process still in it — the primitive behind
    /// [`WindowsCommand::kill_tree_on_drop`](crate::WindowsCommand::kill_tree_on_drop).
    pub fn kill_on_close(&self, enable: bool) -> Result<()> {
        todo!("SetInformationJobObject with JOBOBJECT_EXTENDED_LIMIT_INFORMATION")
    }

    /// Assign an already-running process to this job.
    ///
    /// The post-hoc alternative to
    /// [`AttributeListBuilder::attach_to_job`]; see
    /// `docs/adr/0004-job-attachment.md` for the trade-off.
    pub fn assign(&self, child: &Child) -> Result<()> {
        todo!("AssignProcessToJobObject(job, child)")
    }
}

impl AsHandle for Job {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        todo!("borrow the job handle")
    }
}

/// The raw `HPCON` value of a pseudoconsole.
///
/// `HPCON` is an opaque pointer-sized value from `Win32_System_Console`. It is
/// deliberately *not* exposed as a `windows-sys` type: that would put a
/// pre-1.0 dependency in `spawnkit`'s public API and tie the two crates'
/// semver together.
pub type RawPseudoconsole = isize;

/// A pseudoconsole (ConPTY) that a child can be attached to.
///
/// `spawnkit` does not create pseudoconsoles — `CreatePseudoConsole`, the
/// resize protocol and the pipe plumbing are a library's worth of subtlety on
/// their own, and `conpty-oxide` already does it. This type only *borrows* an
/// `HPCON` long enough to put it in an attribute list.
#[derive(Debug)]
pub struct Pcon {
    raw: RawPseudoconsole,
}

impl Pcon {
    /// Wrap an `HPCON` owned by someone else.
    ///
    /// The caller guarantees the pseudoconsole outlives every spawn that uses
    /// it. This is a plain `fn` only because the crate is
    /// `#![deny(unsafe_code)]` while it is a scaffold; when the `sys` module
    /// lands this becomes an `unsafe fn` with that guarantee written as a
    /// `# Safety` section.
    pub fn from_raw(raw: RawPseudoconsole) -> Self {
        todo!("wrap the HPCON")
    }

    /// The wrapped `HPCON`.
    pub fn as_raw(&self) -> RawPseudoconsole {
        todo!("return the raw HPCON")
    }
}
