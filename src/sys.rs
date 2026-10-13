//! The only Win32 FFI boundary in the crate.

use std::cmp::Ordering;
use std::ffi::{c_void, OsString};
use std::io;
use std::marker::PhantomData;
use std::mem::{size_of, size_of_val, ManuallyDrop};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::process::ExitStatus;
use std::ptr;

use windows_sys::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, ERROR_BROKEN_PIPE,
    ERROR_HANDLE_EOF, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA,
    GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Globalization::{
    CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileAttributesW, GetFullPathNameW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    GetProcessId, InitializeProcThreadAttributeList, OpenProcess, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
    PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

use crate::domain::attribute_rules::{self, SizeFault};
use crate::domain::creation_flag_rules::{creation_word, ConsoleFlag, PriorityFlag};
use crate::domain::environment_rules;
use crate::handles::BorrowedPseudoConsole;
use crate::resource::{
    Borrowed, Creator, Foreign, Inheritor, JobObject, Owned, Pipe, Process, Stream, Thread, Value,
};

pub(crate) const INVALID_RAW_HANDLE: isize = -1;

const JOB_LIMITS_SIZE: u32 = dword_const(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>());
const STARTUPINFO_SIZE: u32 = dword_const(size_of::<STARTUPINFOW>());
const STARTUPINFOEX_SIZE: u32 = dword_const(size_of::<STARTUPINFOEXW>());

/// The longest Windows path in UTF-16 units, including the terminator.
const MAXIMUM_PATH: usize = 32_768;
const MAXIMUM_PATH_CAPACITY: u32 = dword_const(MAXIMUM_PATH);

struct EnvironmentBlock(*mut u16);

#[cfg(test)]
static ENVIRONMENT_BLOCK_DROPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
static ATTRIBUTE_LIST_DROPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl Drop for EnvironmentBlock {
    fn drop(&mut self) {
        #[cfg(test)]
        ENVIRONMENT_BLOCK_DROPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: the pointer came from GetEnvironmentStringsW and is freed once.
        unsafe {
            FreeEnvironmentStringsW(self.0);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StandardStream {
    Input,
    Output,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum NullAccess {
    Read,
    Write,
}

/// Duplicates a handle in the current process, keeping its kind.
pub(crate) fn duplicate_local<K>(
    source: Borrowed<'_, K>,
    inheritable: bool,
) -> io::Result<Owned<K>> {
    #[cfg(test)]
    fault::check(fault::Call::DuplicateLocal)?;
    // SAFETY: `GetCurrentProcess` cannot fail and returns a pseudo-handle that stays valid and is never closed.
    let current = unsafe { GetCurrentProcess() };
    duplicate_between(
        current,
        raw(source.untyped()),
        current,
        inheritable,
        DUPLICATE_SAME_ACCESS,
    )
    .map(Owned::adopt)
}

fn duplicate_between(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
    inheritable: bool,
    options: u32,
) -> io::Result<OwnedHandle> {
    let mut duplicate = ptr::null_mut();
    // SAFETY: the process and source handles are valid for the call, and `duplicate` is writable; on success it is uniquely owned.
    if unsafe {
        DuplicateHandle(
            source_process,
            source,
            target_process,
            &mut duplicate,
            0,
            i32::from(inheritable),
            options,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    owned(duplicate)
}

/// A duplicate in an alternate parent's table, which this process closes there.
pub(crate) struct RemoteHandle<'a> {
    process: Borrowed<'a, Process>,
    value: HANDLE,
}

impl RemoteHandle<'_> {
    pub(crate) fn value(&self) -> Value<Inheritor> {
        Value::in_parent(handle_value(self.value))
    }

    /// Closes the remote value and reports a failure, which the drop would discard.
    pub(crate) fn close(self) -> io::Result<()> {
        ManuallyDrop::new(self).reclaim()
    }

    /// Moves the remote value back with close-source duplication and closes the local copy.
    fn reclaim(&self) -> io::Result<()> {
        #[cfg(test)]
        fault::check(fault::Call::ReclaimRemote)?;
        // SAFETY: `GetCurrentProcess` cannot fail and returns a pseudo-handle that stays valid and is never closed.
        let current = unsafe { GetCurrentProcess() };
        duplicate_between(
            raw(self.process.untyped()),
            self.value,
            current,
            false,
            DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE,
        )
        .map(drop)
    }
}

impl std::fmt::Debug for RemoteHandle<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("RemoteHandle")
            .field(&handle_value(self.value))
            .finish()
    }
}

/// Closes a remote value that no explicit close reached.
impl Drop for RemoteHandle<'_> {
    fn drop(&mut self) {
        drop(self.reclaim());
    }
}

pub(crate) fn duplicate_remote<'a, K>(
    source: Borrowed<'_, K>,
    target_process: Borrowed<'a, Process>,
    inheritable: bool,
) -> io::Result<RemoteHandle<'a>> {
    #[cfg(test)]
    fault::check(fault::Call::DuplicateRemote)?;
    let mut value = ptr::null_mut();
    // SAFETY: both process handles and `source` are valid; the returned value belongs to `target_process` and is owned by `RemoteHandle`.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            raw(source.untyped()),
            raw(target_process.untyped()),
            &mut value,
            0,
            i32::from(inheritable),
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(RemoteHandle {
        process: target_process,
        value,
    })
}

pub(crate) fn standard_handle(stream: StandardStream) -> io::Result<Option<Owned<Stream>>> {
    #[cfg(test)]
    fault::check(fault::Call::StandardHandle)?;
    let id = match stream {
        StandardStream::Input => STD_INPUT_HANDLE,
        StandardStream::Output => STD_OUTPUT_HANDLE,
        StandardStream::Error => STD_ERROR_HANDLE,
    };
    // SAFETY: GetStdHandle has no pointer preconditions.
    let handle = unsafe { GetStdHandle(id) };
    if !is_valid_handle(handle) {
        return Ok(None);
    }
    // SAFETY: GetStdHandle returned a live handle; it is borrowed only for DuplicateHandle and never closed.
    let borrowed = unsafe { BorrowedHandle::borrow_raw(handle) };
    duplicate_local(Borrowed::<Stream>::adopt(borrowed), false).map(Some)
}

pub(crate) fn null_handle(access: NullAccess) -> io::Result<Owned<Stream>> {
    #[cfg(test)]
    fault::check(fault::Call::NullHandle)?;
    let name = [u16::from(b'N'), u16::from(b'U'), u16::from(b'L'), 0];
    let desired = match access {
        NullAccess::Read => GENERIC_READ,
        NullAccess::Write => GENERIC_WRITE,
    };
    // SAFETY: `name` is NUL-terminated and the optional pointers are null; the result is adopted only on success.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            desired,
            null_share_mode(),
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    owned(handle).map(Owned::adopt)
}

fn null_share_mode() -> u32 {
    [FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE]
        .into_iter()
        .fold(0, std::ops::BitOr::bitor)
}

/// The two ends of an anonymous pipe: the caller's, and the one the child receives.
pub(crate) struct PipeEnds {
    pub(crate) parent: Owned<Pipe>,
    pub(crate) child: Owned<Pipe>,
}

pub(crate) fn create_pipe(parent_reads: bool) -> io::Result<PipeEnds> {
    #[cfg(test)]
    fault::check(fault::Call::CreatePipe)?;
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    // SAFETY: both output pointers are valid, and null security attributes make both handles non-inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null::<SECURITY_ATTRIBUTES>(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe succeeded, so both handles are valid and distinct; they are adopted together.
    let (read, write) = unsafe {
        (
            Owned::adopt(OwnedHandle::from_raw_handle(read)),
            Owned::adopt(OwnedHandle::from_raw_handle(write)),
        )
    };
    if parent_reads {
        Ok(PipeEnds {
            parent: read,
            child: write,
        })
    } else {
        Ok(PipeEnds {
            parent: write,
            child: read,
        })
    }
}

pub(crate) fn open_parent_process(pid: u32) -> io::Result<Owned<Process>> {
    #[cfg(test)]
    fault::check(fault::Call::OpenProcess)?;
    // SAFETY: OpenProcess has no pointer preconditions.
    let handle = unsafe { OpenProcess(PROCESS_CREATE_PROCESS | PROCESS_DUP_HANDLE, 0, pid) };
    owned(handle).map(Owned::adopt)
}

/// Checks with a process API that a caller's handle names a process.
pub(crate) fn validate_process_handle(handle: Borrowed<'_, Foreign>) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::ValidateProcess)?;
    // SAFETY: the borrowed handle is valid for the query.
    if unsafe { GetProcessId(raw(handle.untyped())) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn create_job() -> io::Result<Owned<JobObject>> {
    #[cfg(test)]
    fault::check(fault::Call::CreateJob)?;
    // SAFETY: null arguments request an unnamed Job with default security.
    owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) }).map(Owned::adopt)
}

/// Checks with a Job API that a caller's handle names a Job.
pub(crate) fn validate_job_handle(handle: Borrowed<'_, Foreign>) -> io::Result<()> {
    query_job_limits(handle.untyped()).map(drop)
}

pub(crate) fn set_job_kill_on_close(job: Borrowed<'_, JobObject>, enable: bool) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::SetJob)?;
    let handle = job.untyped();
    let mut limits = query_job_limits(handle)?;
    if enable {
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    } else {
        limits.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    }
    // SAFETY: `limits` is the structure this information class requires and is readable for the call.
    if unsafe {
        SetInformationJobObject(
            raw(handle),
            JobObjectExtendedLimitInformation,
            ptr::addr_of!(limits).cast(),
            JOB_LIMITS_SIZE,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn query_job_limits(
    handle: BorrowedHandle<'_>,
) -> io::Result<JOBOBJECT_EXTENDED_LIMIT_INFORMATION> {
    #[cfg(test)]
    fault::check(fault::Call::QueryJob)?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: `limits` is writable storage of the size this information class requires, and the returned-size pointer is null.
    if unsafe {
        QueryInformationJobObject(
            raw(handle),
            JobObjectExtendedLimitInformation,
            ptr::addr_of_mut!(limits).cast(),
            JOB_LIMITS_SIZE,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(limits)
    }
}

/// Assigns a process the caller supplied, whose kind `AssignProcessToJobObject` checks.
pub(crate) fn assign_job(
    job: Borrowed<'_, JobObject>,
    process: Borrowed<'_, Foreign>,
) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::AssignJob)?;
    // SAFETY: both handles are valid for the call.
    bool_result(unsafe { AssignProcessToJobObject(raw(job.untyped()), raw(process.untyped())) })
}

pub(crate) fn terminate_job(job: Borrowed<'_, JobObject>, exit_code: u32) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::TerminateJob)?;
    // SAFETY: the Job handle is valid for the call.
    bool_result(unsafe { TerminateJobObject(raw(job.untyped()), exit_code) })
}

/// The `usize` slots of the two-`DWORD` header that `JOBOBJECT_BASIC_PROCESS_ID_LIST` puts before its identifiers.
const PROCESS_ID_LIST_HEADER: usize = size_of::<[u32; 2]>() / size_of::<usize>();
/// The identifiers the first query has room for.
const PROCESS_ID_LIST_ROOM: usize = 16;
/// The factor by which the room grows when processes joined the Job between two queries.
const PROCESS_ID_LIST_GROWTH: usize = 2;

const _: () = assert!(
    size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
        == size_of::<usize>() * (PROCESS_ID_LIST_HEADER + 1),
    "the identifiers of JOBOBJECT_BASIC_PROCESS_ID_LIST follow the header slots"
);

/// The identifiers of the Job's processes whose exit the Job hasn't recorded.
///
/// The Job can still list a process for a moment after a wait for it returns, but an empty list means that no process of the Job still runs.
pub(crate) fn job_process_ids(job: Borrowed<'_, JobObject>) -> io::Result<Vec<usize>> {
    #[cfg(test)]
    fault::check(fault::Call::ListJob)?;
    let mut room = PROCESS_ID_LIST_ROOM;
    loop {
        let mut buffer = vec![0_usize; PROCESS_ID_LIST_HEADER.saturating_add(room)];
        let bytes = u32::try_from(size_of_val(buffer.as_slice()))
            .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
        // SAFETY: `buffer` is writable for `bytes` bytes and aligned for JOBOBJECT_BASIC_PROCESS_ID_LIST, and the returned-size pointer is null.
        let listed = unsafe {
            QueryInformationJobObject(
                raw(job.untyped()),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                bytes,
                ptr::null_mut(),
            )
        };
        let failure = (listed == 0).then(io::Error::last_os_error);
        // SAFETY: `buffer` holds at least one JOBOBJECT_BASIC_PROCESS_ID_LIST, whose header the query wrote or left zero.
        let header = unsafe {
            buffer
                .as_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
                .read()
        };
        match failure {
            None => {
                return Ok(buffer
                    .into_iter()
                    .skip(PROCESS_ID_LIST_HEADER)
                    .take(widen(header.NumberOfProcessIdsInList))
                    .collect());
            }
            Some(error) if is_win32_error(&error, ERROR_MORE_DATA) => {
                room = widen(header.NumberOfAssignedProcesses)
                    .max(room.saturating_mul(PROCESS_ID_LIST_GROWTH));
            }
            Some(error) => return Err(error),
        }
    }
}

/// Opens a process of the Job to wait for it, or returns `None` if it has exited or its identifier now names a process outside the Job.
pub(crate) fn open_job_process(
    id: usize,
    job: Borrowed<'_, JobObject>,
) -> io::Result<Option<Owned<Process>>> {
    #[cfg(test)]
    fault::check(fault::Call::OpenJobProcess)?;
    let id =
        u32::try_from(id).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    // SAFETY: OpenProcess has no pointer preconditions.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            id,
        )
    };
    let process = match owned(handle) {
        Ok(process) => process,
        Err(error) if is_win32_error(&error, ERROR_INVALID_PARAMETER) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut member = 0;
    // SAFETY: both handles are valid for the query and `member` is writable.
    bool_result(unsafe {
        IsProcessInJob(process.as_raw_handle(), raw(job.untyped()), &mut member)
    })?;
    Ok((member != 0).then_some(Owned::adopt(process)))
}

/// A `PROC_THREAD_ATTRIBUTE_LIST` whose attributes point into values that live for `'a`.
///
/// Windows reads those values until `DeleteProcThreadAttributeList` runs, so the borrows keep them alive and in place for as long as the list exists.
pub(crate) struct AttributeList<'a> {
    storage: Box<[usize]>,
    values: PhantomData<&'a ()>,
}

/// Zeroed storage of `words` words, or `OutOfMemory` when no allocation holds them.
fn zeroed_words(words: usize) -> io::Result<Box<[usize]>> {
    #[cfg(test)]
    fault::check(fault::Call::AllocateAttributes)?;
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(words)
        .map_err(|reserve| io::Error::new(io::ErrorKind::OutOfMemory, reserve))?;
    storage.resize(words, 0_usize);
    Ok(storage.into_boxed_slice())
}

impl<'a> AttributeList<'a> {
    pub(crate) fn new(count: u32) -> io::Result<Self> {
        #[cfg(test)]
        fault::check(fault::Call::AttributeList)?;
        let mut bytes = 0_usize;
        // SAFETY: the documented size query passes a null list.
        let probed =
            unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut bytes) };
        let error = io::Error::last_os_error();
        let probe = attribute_rules::probe(
            probed != 0,
            is_win32_error(&error, ERROR_INSUFFICIENT_BUFFER),
            bytes,
        );
        let words = attribute_rules::storage_words(probe).map_err(|fault| match fault {
            SizeFault::UnexpectedSuccess => {
                io::Error::other("attribute-list size probe unexpectedly succeeded")
            }
            SizeFault::Failed => error,
            SizeFault::TooLarge => io::Error::other("attribute-list size exceeds an allocation"),
        })?;
        let mut storage = zeroed_words(words)?;
        let pointer = storage.as_mut_ptr().cast();
        let mut actual = size_of_val(&*storage);
        // SAFETY: the `Box<[usize]>` is word-aligned, stable, at least `bytes` long, and owned by the returned `AttributeList`.
        if unsafe { InitializeProcThreadAttributeList(pointer, count, 0, &mut actual) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            storage,
            values: PhantomData,
        })
    }

    /// Lists the handles the child inherits, by their values in the inheritor's table.
    pub(crate) fn set_handle_list(&mut self, handles: &'a [Value<Inheritor>]) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            handles.as_ptr().cast(),
            size_of_val(handles),
        )
    }

    /// Names the alternate parent by the value of its handle in the creator's table.
    pub(crate) fn set_parent(&mut self, parent: &'a Value<Creator>) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
            ptr::addr_of!(*parent).cast(),
            size_of::<Value<Creator>>(),
        )
    }

    pub(crate) fn set_mitigation(&mut self, words: &'a [u64; 2]) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
            words.as_ptr().cast(),
            size_of::<[u64; 2]>(),
        )
    }

    /// Names the Jobs by the values of their handles in the creator's table.
    pub(crate) fn set_jobs(&mut self, jobs: &'a [Value<Creator>]) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            jobs.as_ptr().cast(),
            size_of_val(jobs),
        )
    }

    /// Unlike the other attributes, `lpValue` is the `HPCON` value itself, not its address.
    pub(crate) fn set_pseudoconsole(
        &mut self,
        pseudoconsole: BorrowedPseudoConsole<'a>,
    ) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
            handle_from_value(pseudoconsole.as_raw()).cast_const(),
            size_of::<isize>(),
        )
    }

    fn update(&mut self, attribute: u32, value: *const c_void, bytes: usize) -> io::Result<()> {
        #[cfg(test)]
        fault::check(fault::Call::UpdateAttribute)?;
        let attribute = widen(attribute);
        // SAFETY: the list is initialized, and `value` is the `HPCON` value or points to `bytes` readable bytes that the setter borrowed for `'a`, so they stay alive and in place until the drop deletes the list.
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute,
                value,
                bytes,
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn pointer(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_ptr().cast_mut().cast()
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        #[cfg(test)]
        ATTRIBUTE_LIST_DROPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: initialization succeeded once and this is its sole owner.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

impl BorrowedPseudoConsole<'_> {
    /// Borrows a raw `HPCON`, for the lifetime the caller chooses.
    ///
    /// # Safety
    ///
    /// `raw` must be a pseudoconsole that `CreatePseudoConsole` returned, and it must stay open for as long as the result lives.
    /// Zero is never one, and `ClosePseudoConsole` ends one.
    /// `CreateProcessW` reads the pseudoconsole behind the value, so a value that breaks this is undefined behavior.
    #[must_use]
    pub const unsafe fn borrow_raw(raw: isize) -> Self {
        Self {
            raw,
            owner: PhantomData,
        }
    }
}

/// The standard handles of a child, by their values in the inheritor's table.
#[derive(Clone, Copy)]
pub(crate) struct StandardHandles {
    pub(crate) stdin: Value<Inheritor>,
    pub(crate) stdout: Value<Inheritor>,
    pub(crate) stderr: Value<Inheritor>,
}

#[derive(Clone, Copy)]
pub(crate) enum StartupStdio {
    Ordinary(StandardHandles),
    PseudoConsole,
}

pub(crate) struct ProcessRequest<'a> {
    pub(crate) application: &'a [u16],
    pub(crate) command_line: &'a mut [u16],
    pub(crate) environment: Option<&'a [u16]>,
    pub(crate) current_dir: Option<&'a [u16]>,
    pub(crate) stdio: StartupStdio,
    pub(crate) inherit_handles: bool,
    /// The policy flags the command chose.
    pub(crate) policy: u32,
    pub(crate) console: ConsoleFlag,
    pub(crate) priority: PriorityFlag,
    pub(crate) attributes: Option<&'a AttributeList<'a>>,
}

pub(crate) struct CreatedProcess {
    pub(crate) process: Owned<Process>,
    pub(crate) thread: Owned<Thread>,
    pub(crate) pid: u32,
}

/// Creates the process suspended, so the caller releases its temporary resources before the main thread runs.
pub(crate) fn create_process(request: &mut ProcessRequest<'_>) -> io::Result<CreatedProcess> {
    #[cfg(test)]
    fault::check(fault::Call::CreateProcess)?;
    let mut startup = STARTUPINFOEXW::default();
    if let Some(attributes) = request.attributes {
        startup.StartupInfo.cb = STARTUPINFOEX_SIZE;
        startup.lpAttributeList = attributes.pointer();
    } else {
        startup.StartupInfo.cb = STARTUPINFO_SIZE;
    }
    set_standard_handles(&mut startup, request.stdio);
    let flags = creation_word(
        request.policy,
        request.console,
        request.priority,
        request.attributes.is_some(),
    );
    let environment = request
        .environment
        .map_or(ptr::null(), |block| block.as_ptr().cast());
    let current_dir = request.current_dir.map_or(ptr::null(), <[u16]>::as_ptr);
    let mut information = PROCESS_INFORMATION::default();

    // SAFETY: every UTF-16 buffer is terminated and live, `command_line` is writable, and the startup handles and attribute allocations outlive the call.
    if unsafe {
        CreateProcessW(
            request.application.as_ptr(),
            request.command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            i32::from(request.inherit_handles),
            flags,
            environment,
            current_dir,
            &startup.StartupInfo,
            &mut information,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: CreateProcessW succeeded, so both handles are valid; they are adopted together so neither can leak.
    let (process, thread) = unsafe {
        (
            Owned::adopt(OwnedHandle::from_raw_handle(information.hProcess)),
            Owned::adopt(OwnedHandle::from_raw_handle(information.hThread)),
        )
    };
    Ok(CreatedProcess {
        process,
        thread,
        pid: information.dwProcessId,
    })
}

fn set_standard_handles(startup: &mut STARTUPINFOEXW, stdio: StartupStdio) {
    startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
    if let StartupStdio::Ordinary(handles) = stdio {
        startup.StartupInfo.hStdInput = handle_from_value(handles.stdin.as_raw());
        startup.StartupInfo.hStdOutput = handle_from_value(handles.stdout.as_raw());
        startup.StartupInfo.hStdError = handle_from_value(handles.stderr.as_raw());
    }
}

pub(crate) fn wait_process(process: Borrowed<'_, Process>) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::WaitProcess)?;
    // SAFETY: the process handle is valid for the wait.
    match unsafe { WaitForSingleObject(raw(process.untyped()), INFINITE) } {
        WAIT_OBJECT_0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

pub(crate) fn try_wait_process(process: Borrowed<'_, Process>) -> io::Result<bool> {
    #[cfg(test)]
    fault::check(fault::Call::TryWaitProcess)?;
    // SAFETY: the process handle is valid for the query.
    match unsafe { WaitForSingleObject(raw(process.untyped()), 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Test-only failure injection: a test fails the Nth wrapper call made on its thread.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::RefCell;
    use std::fmt;
    use std::io;

    /// A fallible Win32 wrapper.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Call {
        DuplicateLocal,
        DuplicateRemote,
        ReclaimRemote,
        StandardHandle,
        NullHandle,
        CreatePipe,
        OpenProcess,
        ValidateProcess,
        CreateJob,
        QueryJob,
        SetJob,
        AssignJob,
        TerminateJob,
        ListJob,
        OpenJobProcess,
        AttributeList,
        AllocateAttributes,
        UpdateAttribute,
        CreateProcess,
        WaitProcess,
        TryWaitProcess,
        ExitStatus,
        TerminateProcess,
        ResumeThread,
        ReadHandle,
        WriteHandle,
        EnvironmentStrings,
        MaximumPath,
    }

    struct State {
        fail_at: Option<usize>,
        seen: Vec<Call>,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    /// Records this thread's wrapper calls, and fails one of them, until dropped.
    pub(crate) struct Plan(());

    impl Plan {
        /// Returns the calls recorded so far.
        pub(crate) fn calls(&self) -> Vec<Call> {
            let Self(()) = self;
            STATE.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .map(|state| state.seen.clone())
                    .unwrap_or_default()
            })
        }
    }

    impl Drop for Plan {
        fn drop(&mut self) {
            STATE.with(|state| *state.borrow_mut() = None);
        }
    }

    /// Records calls without failing any.
    pub(crate) fn record() -> Plan {
        install(None)
    }

    /// Fails the call at `index` in this thread's call order.
    pub(crate) fn fail_at(index: usize) -> Plan {
        install(Some(index))
    }

    fn install(fail_at: Option<usize>) -> Plan {
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                fail_at,
                seen: Vec::new(),
            });
        });
        Plan(())
    }

    /// Fails this call if the installed plan says so.
    pub(crate) fn check(call: Call) -> io::Result<()> {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut() else {
                return Ok(());
            };
            let index = state.seen.len();
            state.seen.push(call);
            if state.fail_at == Some(index) {
                Err(io::Error::other(Injected(call)))
            } else {
                Ok(())
            }
        })
    }

    /// Returns true if `error` is the injected failure.
    pub(crate) fn is_injected(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(<dyn std::error::Error + Send + Sync>::is::<Injected>)
    }

    #[derive(Debug)]
    struct Injected(Call);

    impl fmt::Display for Injected {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "injected failure of {:?}", self.0)
        }
    }

    impl std::error::Error for Injected {}
}

/// Test helpers that call Win32 directly, so mutants of the production wrappers cannot disable them.
#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::Cell;
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
    use std::ptr;
    use std::thread;

    use windows_sys::Win32::Foundation::{
        DuplicateHandle, GetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE_FLAG_INHERIT,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Console::{
        ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, GetProcessHandleCount, ResumeThread, SuspendThread,
        TerminateProcess, WaitForSingleObject, INFINITE,
    };

    const ISOLATED_ARGUMENT: &str = "windows-spawn-isolated";
    const ISOLATED_VARIABLE: &str = "WINDOWS_SPAWN_ISOLATED";

    /// Reruns `test` alone in a fresh test process and returns false, unless this is that process.
    ///
    /// The marker travels as both an argument and a variable, so a mutant that loses one cannot make the rerun recurse.
    pub(crate) fn isolated(test: &str) -> bool {
        if std::env::var_os(ISOLATED_VARIABLE).is_some()
            || std::env::args().any(|argument| argument == ISOLATED_ARGUMENT)
        {
            return true;
        }
        let status = crate::Command::new(std::env::current_exe().expect("the test binary path"))
            .args(["--exact", test, "--test-threads=1", ISOLATED_ARGUMENT])
            .env(ISOLATED_VARIABLE, "1")
            .stdin(crate::Stdio::null())
            .status()
            .expect("the isolated test process starts");
        assert!(status.success(), "{test} failed in isolation");
        false
    }

    /// Returns the current-process pseudo-handle.
    pub(crate) fn current_process() -> BorrowedHandle<'static> {
        // SAFETY: `GetCurrentProcess` cannot fail and returns a pseudo-handle valid for the process lifetime.
        // `BorrowedHandle` never closes it, so a `'static` borrow cannot dangle or double-close.
        unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess()) }
    }

    pub(crate) fn process_handle_count(process: BorrowedHandle<'_>) -> std::io::Result<u32> {
        let mut count = 0;
        // SAFETY: `process` is valid and `count` is writable DWORD storage.
        if unsafe { GetProcessHandleCount(process.as_raw_handle(), &mut count) } == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(count)
        }
    }

    /// Returns a thread's suspend count, leaving it unchanged.
    pub(crate) fn suspend_count(thread: BorrowedHandle<'_>) -> u32 {
        // SAFETY: the thread handle is valid and has THREAD_SUSPEND_RESUME access.
        let previous = unsafe { SuspendThread(thread.as_raw_handle()) };
        assert_ne!(previous, u32::MAX, "SuspendThread failed");
        // SAFETY: the same handle is valid; this undoes the suspension above.
        let resumed = unsafe { ResumeThread(thread.as_raw_handle()) };
        assert_ne!(resumed, u32::MAX, "ResumeThread failed");
        previous
    }

    /// Returns a non-inheritable duplicate with the same access.
    pub(crate) fn duplicate(handle: BorrowedHandle<'_>) -> OwnedHandle {
        let mut duplicate = ptr::null_mut();
        // SAFETY: both pseudo-handles and `handle` are valid, and `duplicate` is writable.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                handle.as_raw_handle(),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        assert_ne!(duplicated, 0, "DuplicateHandle failed");
        // SAFETY: DuplicateHandle returned a new, uniquely owned handle.
        unsafe { OwnedHandle::from_raw_handle(duplicate) }
    }

    /// Returns a non-inheritable duplicate limited to `access`.
    pub(crate) fn duplicate_with_access(handle: BorrowedHandle<'_>, access: u32) -> OwnedHandle {
        let mut duplicate = ptr::null_mut();
        // SAFETY: both pseudo-handles and `handle` are valid, and `duplicate` is writable.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                handle.as_raw_handle(),
                GetCurrentProcess(),
                &mut duplicate,
                access,
                0,
                0,
            )
        };
        assert_ne!(duplicated, 0, "DuplicateHandle failed");
        // SAFETY: DuplicateHandle returned a new, uniquely owned handle.
        unsafe { OwnedHandle::from_raw_handle(duplicate) }
    }

    /// Returns true if `handle` is inheritable.
    pub(crate) fn is_inheritable(handle: BorrowedHandle<'_>) -> bool {
        let mut flags = 0;
        // SAFETY: `handle` is valid and `flags` is writable.
        let queried = unsafe { GetHandleInformation(handle.as_raw_handle(), &mut flags) };
        assert_ne!(queried, 0, "GetHandleInformation failed");
        flags & HANDLE_FLAG_INHERIT != 0
    }

    /// Returns a non-inheritable pipe as (reader, writer).
    pub(crate) fn pipe() -> (OwnedHandle, OwnedHandle) {
        let mut read = ptr::null_mut();
        let mut write = ptr::null_mut();
        // SAFETY: both output pointers are writable, and null security attributes make both handles non-inheritable.
        let created = unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) };
        assert_ne!(created, 0, "CreatePipe failed");
        // SAFETY: CreatePipe succeeded, so both handles are new and distinct.
        unsafe {
            (
                OwnedHandle::from_raw_handle(read),
                OwnedHandle::from_raw_handle(write),
            )
        }
    }

    /// Resumes a thread once, ignoring the result.
    ///
    /// Run after a termination under test: a terminated thread never runs again, while a surviving one runs and exits with its own code.
    pub(crate) fn tempt_resume(thread: BorrowedHandle<'_>) {
        // SAFETY: the thread handle is valid for the call.
        let _ = unsafe { ResumeThread(thread.as_raw_handle()) };
    }

    /// Owns a duplicated process handle and terminates the process on drop unless it was observed to exit.
    pub(crate) struct ProcessExitGuard {
        process: OwnedHandle,
        armed: bool,
    }

    impl ProcessExitGuard {
        pub(crate) fn watch(process: BorrowedHandle<'_>) -> Self {
            Self {
                process: duplicate(process),
                armed: true,
            }
        }

        /// Waits for exit and returns the exit code.
        pub(crate) fn exit_code(&mut self) -> u32 {
            // SAFETY: the owned process handle is valid for the wait.
            let waited = unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) };
            assert_eq!(waited, WAIT_OBJECT_0, "waiting for the process failed");
            self.armed = false;
            let mut code = 0_u32;
            // SAFETY: the owned process handle is valid and `code` is writable.
            let queried = unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) };
            assert_ne!(queried, 0, "GetExitCodeProcess failed");
            code
        }
    }

    impl Drop for ProcessExitGuard {
        fn drop(&mut self) {
            if self.armed {
                // SAFETY: the duplicate has the source handle's access.
                let _ = unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
                // SAFETY: the same handle is valid for the wait.
                let _ = unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) };
            }
        }
    }

    /// The size of the test pseudoconsole, which no test reads.
    const CONSOLE_SIZE: COORD = COORD { X: 80, Y: 25 };

    /// A pseudoconsole created with raw Win32 calls, outside any recorded plan.
    pub(crate) struct TestConsole {
        value: HPCON,
        input: Option<OwnedHandle>,
    }

    impl TestConsole {
        pub(crate) fn create() -> Self {
            let (input_reader, input_writer) = pipe();
            let (output_reader, output_writer) = pipe();
            let mut value = 0;
            // SAFETY: both pipe ends are valid for the call and `value` is writable; this type owns the HPCON.
            let created = unsafe {
                CreatePseudoConsole(
                    CONSOLE_SIZE,
                    input_reader.as_raw_handle(),
                    output_writer.as_raw_handle(),
                    0,
                    &mut value,
                )
            };
            assert!(created >= 0, "CreatePseudoConsole failed with {created:#x}");
            drop((input_reader, output_writer));
            let mut output = File::from(output_reader);
            drop(thread::spawn(move || {
                let mut buffer = [0_u8; 4096];
                while matches!(output.read(&mut buffer), Ok(read) if read > 0) {}
            }));
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
            drop(thread::spawn(move || {
                // SAFETY: this type uniquely owns the HPCON.
                unsafe { ClosePseudoConsole(value) };
            }));
        }
    }

    impl crate::AsPseudoConsole for TestConsole {
        fn as_pseudo_console(&self) -> crate::BorrowedPseudoConsole<'_> {
            // SAFETY: TestConsole owns the HPCON CreatePseudoConsole returned until it drops, which the borrow of `self` outlives.
            unsafe { crate::BorrowedPseudoConsole::borrow_raw(self.value) }
        }
    }

    /// A pseudoconsole value that a test changes after lending it, to show that options read it once.
    pub(crate) struct ChangingPseudoConsole(pub(crate) Cell<isize>);

    impl crate::AsPseudoConsole for ChangingPseudoConsole {
        fn as_pseudo_console(&self) -> crate::BorrowedPseudoConsole<'_> {
            // SAFETY: only the snapshot test of `SpawnOptions::pseudo_console` lends it, and that test never spawns, so the value never reaches Win32.
            unsafe { crate::BorrowedPseudoConsole::borrow_raw(self.0.get()) }
        }
    }
}

pub(crate) fn exit_status(process: Borrowed<'_, Process>) -> io::Result<ExitStatus> {
    use std::os::windows::process::ExitStatusExt;

    #[cfg(test)]
    fault::check(fault::Call::ExitStatus)?;

    let mut code = 0_u32;
    // SAFETY: `code` is writable and the process handle is valid.
    if unsafe { GetExitCodeProcess(raw(process.untyped()), &mut code) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ExitStatus::from_raw(code))
    }
}

pub(crate) fn terminate_process(process: Borrowed<'_, Process>, exit_code: u32) -> io::Result<()> {
    #[cfg(test)]
    fault::check(fault::Call::TerminateProcess)?;
    // SAFETY: the process handle is valid for the call.
    bool_result(unsafe { TerminateProcess(raw(process.untyped()), exit_code) })
}

pub(crate) fn resume_thread(thread: Borrowed<'_, Thread>) -> io::Result<u32> {
    #[cfg(test)]
    fault::check(fault::Call::ResumeThread)?;
    // SAFETY: the thread handle is valid for the call.
    let previous = unsafe { ResumeThread(raw(thread.untyped())) };
    if previous == u32::MAX {
        Err(io::Error::last_os_error())
    } else {
        Ok(previous)
    }
}

pub(crate) fn read_handle(handle: Borrowed<'_, Pipe>, buffer: &mut [u8]) -> io::Result<usize> {
    #[cfg(test)]
    fault::check(fault::Call::ReadHandle)?;
    if buffer.is_empty() {
        return Ok(0);
    }
    let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    let mut read = 0_u32;
    // SAFETY: `buffer` is writable for `length` bytes, the handle is valid, and a null OVERLAPPED requests synchronous I/O.
    if unsafe {
        ReadFile(
            raw(handle.untyped()),
            buffer.as_mut_ptr(),
            length,
            &mut read,
            ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if is_win32_error(&error, ERROR_BROKEN_PIPE) || is_win32_error(&error, ERROR_HANDLE_EOF) {
            Ok(0)
        } else {
            Err(error)
        }
    } else {
        Ok(widen(read))
    }
}

pub(crate) fn write_handle(handle: Borrowed<'_, Pipe>, buffer: &[u8]) -> io::Result<usize> {
    #[cfg(test)]
    fault::check(fault::Call::WriteHandle)?;
    if buffer.is_empty() {
        return Ok(0);
    }
    let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    let mut written = 0_u32;
    // SAFETY: `buffer` is readable for `length` bytes, the handle is valid, and a null OVERLAPPED requests synchronous I/O.
    if unsafe {
        WriteFile(
            raw(handle.untyped()),
            buffer.as_ptr(),
            length,
            &mut written,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(widen(written))
    }
}

pub(crate) fn environment_strings() -> io::Result<Vec<(OsString, OsString)>> {
    #[cfg(test)]
    fault::check(fault::Call::EnvironmentStrings)?;
    // SAFETY: GetEnvironmentStringsW returns a double-NUL block that stays valid until FreeEnvironmentStringsW.
    let base = unsafe { GetEnvironmentStringsW() };
    if base.is_null() {
        return Err(io::Error::last_os_error());
    }
    let guard = EnvironmentBlock(base);
    let mut length = 0_usize;
    let mut previous = None;
    loop {
        // SAFETY: the block ends with a NUL that starts it or follows another NUL, and the scan stops there.
        let unit = unsafe { *guard.0.add(length) };
        if environment_rules::ends_block(previous, unit) {
            break;
        }
        previous = Some(unit);
        length = length.saturating_add(1);
    }
    // SAFETY: the scan read every unit before the block's terminator, which stays valid until `guard` drops.
    let block = unsafe { std::slice::from_raw_parts(guard.0, length) };
    Ok(environment_rules::entries(block)
        .map(|(key, value)| (OsString::from_wide(key), OsString::from_wide(value)))
        .collect())
}

pub(crate) fn compare_ordinal(left: &[u16], right: &[u16]) -> Ordering {
    let left_len = i32::try_from(left.len()).unwrap_or(i32::MAX);
    let right_len = i32::try_from(right.len()).unwrap_or(i32::MAX);
    // SAFETY: both pointers are readable for their checked lengths.
    match unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) } {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => left.cmp(right),
    }
}

pub(crate) fn program_exists(path: &[u16]) -> bool {
    // SAFETY: callers pass a NUL-terminated path.
    unsafe { GetFileAttributesW(path.as_ptr()) != INVALID_FILE_ATTRIBUTES }
}

pub(crate) fn system_directory() -> io::Result<OsString> {
    system_path(false)
}

pub(crate) fn windows_directory() -> io::Result<OsString> {
    system_path(true)
}

fn system_path(windows: bool) -> io::Result<OsString> {
    let path = maximum_path(|buffer, length| {
        if windows {
            // SAFETY: `buffer` is writable for `length` UTF-16 units.
            unsafe { GetWindowsDirectoryW(buffer, length) }
        } else {
            // SAFETY: `buffer` is writable for `length` UTF-16 units.
            unsafe { GetSystemDirectoryW(buffer, length) }
        }
    })?;
    Ok(OsString::from_wide(&path))
}

/// Returns the absolute form of a NUL-terminated path, without the terminator.
pub(crate) fn full_path(path: &[u16]) -> io::Result<Vec<u16>> {
    if path.last() != Some(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not NUL-terminated",
        ));
    }
    maximum_path(|buffer, length| {
        // SAFETY: `path` is NUL-terminated, `buffer` is writable for `length` units, and the file-part pointer is null.
        unsafe { GetFullPathNameW(path.as_ptr(), length, buffer, ptr::null_mut()) }
    })
}

/// Calls `fill` with a buffer of the maximum Windows path length and returns what it wrote.
///
/// `fill` returns the written length without the terminator, or zero on failure.
/// A maximum-size buffer avoids a size-query retry loop.
fn maximum_path(fill: impl FnOnce(*mut u16, u32) -> u32) -> io::Result<Vec<u16>> {
    #[cfg(test)]
    fault::check(fault::Call::MaximumPath)?;
    let mut buffer = vec![0_u16; MAXIMUM_PATH];
    let length = widen(fill(buffer.as_mut_ptr(), MAXIMUM_PATH_CAPACITY));
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(io::Error::other(
            "path exceeds the maximum Windows path length",
        ));
    }
    buffer.truncate(length);
    Ok(buffer)
}

fn raw(handle: BorrowedHandle<'_>) -> HANDLE {
    handle.as_raw_handle()
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if is_valid_handle(handle) {
        // SAFETY: callers pass a new handle and transfer its only local ownership.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_valid_handle(handle: HANDLE) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
}

/// Returns a handle's numeric value, as a child sees it in an argument or the environment.
#[allow(clippy::as_conversions)]
pub(crate) fn handle_value(handle: HANDLE) -> isize {
    handle as isize
}

/// Returns the handle a numeric value names.
#[allow(clippy::as_conversions)]
pub(crate) const fn handle_from_value(value: isize) -> HANDLE {
    value as HANDLE
}

/// Converts a compile-time size to a Win32 `DWORD`; a size that does not fit fails constant evaluation.
#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
const fn dword_const(value: usize) -> u32 {
    let wide = value as u64;
    assert!(wide <= u32::MAX as u64, "size does not fit a DWORD");
    wide as u32
}

/// Widens a `DWORD`; `usize` has at least 32 bits on every Windows target.
#[allow(clippy::as_conversions)]
const fn widen(value: u32) -> usize {
    value as usize
}

/// Returns true if `error` carries the Win32 error `code`.
fn is_win32_error(error: &io::Error, code: u32) -> bool {
    error
        .raw_os_error()
        .is_some_and(|raw| u32::try_from(raw) == Ok(code))
}

fn bool_result(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use std::path::PathBuf;

    use super::test_support::{current_process, isolated, process_handle_count};
    use super::*;

    /// Compares this process's handle count, so it reruns alone in a fresh test process.
    #[test]
    fn pipe_null_and_duplicate_primitives_preserve_ownership() -> io::Result<()> {
        if !isolated("sys::tests::pipe_null_and_duplicate_primitives_preserve_ownership") {
            return Ok(());
        }
        assert_eq!(
            null_share_mode(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        );
        let readable_null = null_handle(NullAccess::Read)?;
        let writable_null = null_handle(NullAccess::Write)?;
        let mut unread = [0_u8; 1];
        assert_eq!(File::from(readable_null.into_inner()).read(&mut unread)?, 0);
        let mut discard = File::from(duplicate_local(writable_null.borrow(), false)?.into_inner());
        assert_eq!(discard.write(b"discard")?, 7);
        drop(discard);

        let parent_reads = create_pipe(true)?;
        assert_eq!(read_handle(parent_reads.parent.borrow(), &mut [])?, 0);
        assert_eq!(write_handle(parent_reads.child.borrow(), &[])?, 0);
        assert_eq!(write_handle(parent_reads.child.borrow(), b"a")?, 1);
        let mut byte = [0_u8; 1];
        assert_eq!(read_handle(parent_reads.parent.borrow(), &mut byte)?, 1);
        assert_eq!(byte, [b'a']);

        let parent_writes = create_pipe(false)?;
        assert_eq!(write_handle(parent_writes.parent.borrow(), b"b")?, 1);
        assert_eq!(read_handle(parent_writes.child.borrow(), &mut byte)?, 1);
        assert_eq!(byte, [b'b']);

        let private = duplicate_local(writable_null.borrow(), false)?;
        let inheritable = duplicate_local(writable_null.borrow(), true)?;
        drop((private, inheritable));

        let mut host_command = crate::Command::new("cmd.exe");
        host_command
            .args(["/D", "/C", "exit /b 0"])
            .stdin(crate::Stdio::null())
            .stdout(crate::Stdio::null())
            .stderr(crate::Stdio::null());
        let host = host_command.spawn_suspended()?;
        let _host_guard = test_support::ProcessExitGuard::watch(host.as_handle());
        let target = open_parent_process(host.id())?;
        drop(target);
        let before = process_handle_count(host.as_handle())?;
        let local_before = process_handle_count(current_process())?;
        let remote = duplicate_remote(
            writable_null.borrow(),
            Borrowed::adopt(host.as_handle()),
            true,
        )?;
        assert_ne!(remote.value().as_raw(), 0);
        assert!(process_handle_count(host.as_handle())? > before);
        drop(remote);
        assert_eq!(process_handle_count(host.as_handle())?, before);
        assert_eq!(process_handle_count(current_process())?, local_before);
        drop(host);
        Ok(())
    }

    #[test]
    fn jobs_attributes_and_standard_handles_cover_all_ffi_shapes() -> io::Result<()> {
        let drops_before = ATTRIBUTE_LIST_DROPS.load(std::sync::atomic::Ordering::Relaxed);
        let job = create_job()?;
        validate_job_handle(Borrowed::foreign(&job))?;
        set_job_kill_on_close(job.borrow(), true)?;
        assert_ne!(
            query_job_limits(job.as_handle())?
                .BasicLimitInformation
                .LimitFlags
                & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
        set_job_kill_on_close(job.borrow(), false)?;
        assert_eq!(
            query_job_limits(job.as_handle())?
                .BasicLimitInformation
                .LimitFlags
                & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
        let process = open_parent_process(std::process::id())?;
        let inherited = duplicate_local(job.borrow(), true)?;
        let handles = [Value::<Creator>::of(inherited.as_handle()).inherited()];
        let jobs = [Value::<Creator>::of(job.as_handle())];
        let parent = Value::<Creator>::of(process.as_handle());
        let words = [1_u64, 0_u64];
        let mut attributes = AttributeList::new(4)?;
        attributes.set_handle_list(&handles)?;
        attributes.set_parent(&parent)?;
        attributes.set_mitigation(&words)?;
        attributes.set_jobs(&jobs)?;
        drop(attributes);

        let mut pseudoconsole = AttributeList::new(1)?;
        // SAFETY: UpdateProcThreadAttribute stores the value without reading the pseudoconsole, and no process is created from this list.
        let unread = unsafe { BorrowedPseudoConsole::borrow_raw(1) };
        drop(pseudoconsole.set_pseudoconsole(unread));
        drop(pseudoconsole);
        assert!(ATTRIBUTE_LIST_DROPS.load(std::sync::atomic::Ordering::Relaxed) > drops_before);

        let input = standard_handle(StandardStream::Input)?;
        let output = standard_handle(StandardStream::Output)?;
        let error = standard_handle(StandardStream::Error)?;
        assert!(input.is_some() || output.is_some() || error.is_some());
        assert_eq!(INVALID_RAW_HANDLE, -1);
        Ok(())
    }

    #[test]
    fn startup_info_distinguishes_pseudoconsole_and_ordinary_stdio() {
        let mut conpty = STARTUPINFOEXW::default();
        set_standard_handles(&mut conpty, StartupStdio::PseudoConsole);
        assert_ne!(conpty.StartupInfo.dwFlags & STARTF_USESTDHANDLES, 0);
        assert!(conpty.StartupInfo.hStdInput.is_null());
        assert!(conpty.StartupInfo.hStdOutput.is_null());
        assert!(conpty.StartupInfo.hStdError.is_null());

        let mut ordinary = STARTUPINFOEXW::default();
        set_standard_handles(
            &mut ordinary,
            StartupStdio::Ordinary(StandardHandles {
                stdin: Value::in_parent(1),
                stdout: Value::in_parent(2),
                stderr: Value::in_parent(3),
            }),
        );
        assert_ne!(ordinary.StartupInfo.dwFlags & STARTF_USESTDHANDLES, 0);
        assert_eq!(handle_value(ordinary.StartupInfo.hStdInput), 1);
        assert_eq!(handle_value(ordinary.StartupInfo.hStdOutput), 2);
        assert_eq!(handle_value(ordinary.StartupInfo.hStdError), 3);
    }

    #[test]
    fn environment_paths_comparison_and_error_helpers_work() -> io::Result<()> {
        let drops_before = ENVIRONMENT_BLOCK_DROPS.load(std::sync::atomic::Ordering::Relaxed);
        assert_ne!(environment_strings()?, Vec::new());
        assert!(ENVIRONMENT_BLOCK_DROPS.load(std::sync::atomic::Ordering::Relaxed) > drops_before);
        assert_eq!(
            compare_ordinal(&[u16::from(b'a')], &[u16::from(b'B')]),
            Ordering::Less
        );
        assert_eq!(
            compare_ordinal(&[u16::from(b'a')], &[u16::from(b'A')]),
            Ordering::Equal
        );
        assert_eq!(
            compare_ordinal(&[u16::from(b'z')], &[u16::from(b'A')]),
            Ordering::Greater
        );
        assert_eq!(
            compare_ordinal(&[u16::from(b'B')], &[u16::from(b'a')]),
            Ordering::Greater
        );

        let system = system_directory()?;
        let windows = windows_directory()?;
        assert!(!system.is_empty() && !windows.is_empty());
        let mut executable = PathBuf::from(system);
        executable.push("cmd.exe");
        let mut wide: Vec<u16> = executable.as_os_str().encode_wide().collect();
        wide.push(0);
        assert!(program_exists(&wide));
        let missing = std::env::temp_dir().join(format!(
            "windows-spawn-definitely-missing-{}",
            std::process::id()
        ));
        let mut missing_wide: Vec<u16> = missing.as_os_str().encode_wide().collect();
        missing_wide.push(0);
        assert!(!program_exists(&missing_wide));

        assert!(owned(ptr::null_mut()).is_err());
        assert!(owned(INVALID_HANDLE_VALUE).is_err());
        assert!(bool_result(0).is_err());
        bool_result(1)?;

        assert!(open_parent_process(u32::MAX).is_err());

        let file = File::open("NUL")?;
        assert!(is_valid_handle(file.as_raw_handle()));
        assert!(!is_valid_handle(ptr::null_mut()));
        assert!(!is_valid_handle(INVALID_HANDLE_VALUE));
        assert!(validate_process_handle(Borrowed::foreign(&file)).is_err());
        assert!(validate_job_handle(Borrowed::foreign(&file)).is_err());
        Ok(())
    }

    #[test]
    fn failed_win32_calls_return_their_errors() -> io::Result<()> {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };

        /// `JOB_OBJECT_QUERY` from winnt.h, outside the enabled windows-sys features.
        const JOB_OBJECT_QUERY: u32 = 0x0004;

        let unwaitable = test_support::duplicate_with_access(
            current_process(),
            PROCESS_QUERY_LIMITED_INFORMATION,
        );
        assert!(wait_process(Borrowed::adopt(unwaitable.as_handle())).is_err());
        assert!(try_wait_process(Borrowed::adopt(unwaitable.as_handle())).is_err());
        let unqueryable =
            test_support::duplicate_with_access(current_process(), PROCESS_SYNCHRONIZE);
        assert!(exit_status(Borrowed::adopt(unqueryable.as_handle())).is_err());

        // SAFETY: `GetCurrentThread` cannot fail and returns a pseudo-handle valid for this call.
        let thread = unsafe { BorrowedHandle::borrow_raw(GetCurrentThread()) };
        let unresumable = test_support::duplicate_with_access(thread, PROCESS_SYNCHRONIZE);
        assert!(resume_thread(Borrowed::adopt(unresumable.as_handle())).is_err());

        let (reader, writer) = test_support::pipe();
        assert!(read_handle(Borrowed::adopt(writer.as_handle()), &mut [0_u8; 1]).is_err());
        assert!(write_handle(Borrowed::adopt(reader.as_handle()), b"x").is_err());

        let job = create_job()?;
        let query_only = test_support::duplicate_with_access(job.as_handle(), JOB_OBJECT_QUERY);
        assert!(set_job_kill_on_close(Borrowed::adopt(query_only.as_handle()), true).is_err());

        assert!(AttributeList::new(u32::MAX).is_err());
        let jobs = [Value::<Creator>::of(job.as_handle())];
        let words = [1_u64, 0_u64];
        let mut full = AttributeList::new(1)?;
        full.set_jobs(&jobs)?;
        assert!(full.set_mitigation(&words).is_err());

        assert_eq!(
            full_path(&[u16::from(b'a')]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(maximum_path(|_, _| 0).is_err());
        assert!(maximum_path(|_, capacity| capacity).is_err());
        let written = maximum_path(|buffer, _| {
            // SAFETY: `maximum_path` passes a buffer writable for its capacity, which exceeds one unit.
            unsafe { buffer.write(u16::from(b'x')) };
            1
        })?;
        assert_eq!(written, [u16::from(b'x')]);
        Ok(())
    }

    #[test]
    fn storage_beyond_any_allocation_is_out_of_memory() -> io::Result<()> {
        let largest = (usize::MAX >> 1) / size_of::<usize>();
        assert_eq!(
            zeroed_words(largest).unwrap_err().kind(),
            io::ErrorKind::OutOfMemory
        );
        assert_eq!(*zeroed_words(3)?, [0, 0, 0]);
        Ok(())
    }

    #[test]
    fn system_and_windows_directories_are_distinct() -> io::Result<()> {
        let system = system_directory()?.to_string_lossy().to_lowercase();
        let windows = windows_directory()?.to_string_lossy().to_lowercase();
        assert_ne!(system, windows);
        assert!(system.starts_with(&windows));
        assert!(system.ends_with("system32"));
        Ok(())
    }

    #[test]
    fn standard_handles_are_private_duplicates() -> io::Result<()> {
        let output =
            standard_handle(StandardStream::Output)?.expect("the test has standard output");
        assert!(!test_support::is_inheritable(output.as_handle()));
        Ok(())
    }

    /// Changes this process's standard error slot, so it runs alone in a fresh test process.
    #[test]
    fn a_missing_standard_handle_is_none() -> io::Result<()> {
        use windows_sys::Win32::System::Console::SetStdHandle;

        if !isolated("sys::tests::a_missing_standard_handle_is_none") {
            return Ok(());
        }
        // SAFETY: this isolated process no longer needs its standard error slot.
        let cleared = unsafe { SetStdHandle(STD_ERROR_HANDLE, ptr::null_mut()) };
        assert_ne!(cleared, 0);
        assert!(standard_handle(StandardStream::Error)?.is_none());
        Ok(())
    }
}
