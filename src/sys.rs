use std::cmp::Ordering;
use std::ffi::{c_void, OsString};
use std::io;
use std::marker::PhantomData;
use std::mem::{size_of, size_of_val};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::ptr;

use windows_sys::Win32::Foundation::{
    DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, ERROR_BROKEN_PIPE,
    ERROR_HANDLE_EOF, ERROR_INSUFFICIENT_BUFFER, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Globalization::{
    CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileAttributesW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
#[cfg(test)]
use windows_sys::Win32::System::Threading::GetProcessHandleCount;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    GetProcessId, InitializeProcThreadAttributeList, OpenProcess, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_BREAKAWAY_FROM_JOB,
    CREATE_DEFAULT_ERROR_MODE, CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
    CREATE_PRESERVE_CODE_AUTHZ_LEVEL, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    DETACHED_PROCESS, EXTENDED_STARTUPINFO_PRESENT, INFINITE, INHERIT_PARENT_AFFINITY,
    PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE, PROCESS_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
    PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::options::CreationFlags;
use crate::resource::ChildHandleValue;

const MAXIMUM_WINDOWS_PATH_UNITS: u32 = 32_768;
const INSUFFICIENT_BUFFER_CODE: i32 = 122;
#[cfg(target_pointer_width = "64")]
const JOB_LIMITS_SIZE: u32 = 144;
#[cfg(target_pointer_width = "32")]
const JOB_LIMITS_SIZE: u32 = 112;
#[cfg(target_pointer_width = "64")]
const STARTUP_INFO_SIZE: u32 = 104;
#[cfg(target_pointer_width = "32")]
const STARTUP_INFO_SIZE: u32 = 68;
#[cfg(target_pointer_width = "64")]
const STARTUP_INFO_EX_SIZE: u32 = 112;
#[cfg(target_pointer_width = "32")]
const STARTUP_INFO_EX_SIZE: u32 = 72;
const REMOTE_CLOSE_OPTIONS: u32 = DUPLICATE_SAME_ACCESS.wrapping_add(DUPLICATE_CLOSE_SOURCE);
const PARENT_PROCESS_ACCESS: u32 = PROCESS_CREATE_PROCESS
    .wrapping_add(PROCESS_DUP_HANDLE)
    .wrapping_add(PROCESS_QUERY_LIMITED_INFORMATION);
const _: () = assert!(DUPLICATE_SAME_ACCESS & DUPLICATE_CLOSE_SOURCE == 0);
const _: () = assert!(PROCESS_CREATE_PROCESS & PROCESS_DUP_HANDLE == 0);
const _: () = assert!(PROCESS_CREATE_PROCESS & PROCESS_QUERY_LIMITED_INFORMATION == 0);
const _: () = assert!(PROCESS_DUP_HANDLE & PROCESS_QUERY_LIMITED_INFORMATION == 0);
const _: () = assert!(CREATE_UNICODE_ENVIRONMENT & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_UNICODE_ENVIRONMENT & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_SUSPENDED & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_NEW_PROCESS_GROUP & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_NEW_PROCESS_GROUP & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_NEW_PROCESS_GROUP & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(INHERIT_PARENT_AFFINITY & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(INHERIT_PARENT_AFFINITY & CREATE_SUSPENDED == 0);
const _: () = assert!(INHERIT_PARENT_AFFINITY & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_BREAKAWAY_FROM_JOB & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_BREAKAWAY_FROM_JOB & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_BREAKAWAY_FROM_JOB & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_PRESERVE_CODE_AUTHZ_LEVEL & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_PRESERVE_CODE_AUTHZ_LEVEL & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_PRESERVE_CODE_AUTHZ_LEVEL & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_DEFAULT_ERROR_MODE & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_DEFAULT_ERROR_MODE & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_DEFAULT_ERROR_MODE & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(DETACHED_PROCESS & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(DETACHED_PROCESS & CREATE_SUSPENDED == 0);
const _: () = assert!(DETACHED_PROCESS & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_NEW_CONSOLE & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_NEW_CONSOLE & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_NEW_CONSOLE & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(CREATE_NO_WINDOW & CREATE_UNICODE_ENVIRONMENT == 0);
const _: () = assert!(CREATE_NO_WINDOW & CREATE_SUSPENDED == 0);
const _: () = assert!(CREATE_NO_WINDOW & EXTENDED_STARTUPINFO_PRESENT == 0);
const _: () = assert!(ERROR_INSUFFICIENT_BUFFER == 122);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() == 144);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() == 112);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<STARTUPINFOEXW>() == 112);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<STARTUPINFOEXW>() == 72);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<windows_sys::Win32::System::Threading::STARTUPINFOW>() == 104);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<windows_sys::Win32::System::Threading::STARTUPINFOW>() == 68);

struct EnvironmentBlock(ptr::NonNull<u16>);

enum EnvironmentUnit {
    Terminator,
    Content,
}

const fn classify_environment_unit(unit: u16) -> EnvironmentUnit {
    match unit {
        0 => EnvironmentUnit::Terminator,
        _ => EnvironmentUnit::Content,
    }
}

const _: () = assert!(matches!(
    classify_environment_unit(0),
    EnvironmentUnit::Terminator
));
const _: () = assert!(matches!(
    classify_environment_unit(1),
    EnvironmentUnit::Content
));

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
        // SAFETY: the pointer came from GetEnvironmentStringsW and is
        // released exactly once.
        unsafe {
            FreeEnvironmentStringsW(self.0.as_ptr());
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Inheritability {
    Private,
    Inheritable,
}

impl Inheritability {
    const fn as_win32(self) -> i32 {
        match self {
            Self::Private => 0,
            Self::Inheritable => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PipeDirection {
    ParentReads,
    ParentWrites,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitialState {
    Suspended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SystemDirectory {
    System,
    Windows,
}

pub(crate) fn duplicate_local(
    source: BorrowedHandle<'_>,
    inheritability: Inheritability,
) -> io::Result<OwnedHandle> {
    // SAFETY: `GetCurrentProcess` takes no arguments, cannot fail, and returns
    // the current-process pseudo-handle. The value is a constant that stays
    // valid for the lifetime of the process and must never be closed.
    let current = unsafe { GetCurrentProcess() };
    duplicate_between(
        current,
        raw(source),
        current,
        inheritability,
        DUPLICATE_SAME_ACCESS,
    )
}

fn duplicate_between(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
    inheritability: Inheritability,
    options: u32,
) -> io::Result<OwnedHandle> {
    let mut duplicate = ptr::null_mut();
    // SAFETY: process and source handles are valid for the call; `duplicate`
    // points to writable storage and becomes uniquely owned on success.
    let result = unsafe {
        DuplicateHandle(
            source_process,
            source,
            target_process,
            &mut duplicate,
            0,
            inheritability.as_win32(),
            options,
        )
    };
    match classify_win32_call(result) {
        Win32CallOutcome::Failed => return Err(io::Error::last_os_error()),
        Win32CallOutcome::Succeeded => {}
    }
    owned(duplicate)
}

#[derive(Debug)]
enum ReclaimState {
    Open,
    Reclaimed,
}

#[derive(Debug)]
pub(crate) struct RemoteHandle<'a> {
    process: BorrowedHandle<'a>,
    value: std::num::NonZeroIsize,
    state: ReclaimState,
    marker: PhantomData<(
        crate::resource::RemoteKind,
        crate::resource::AlternateTable<'a>,
    )>,
}

pub(crate) struct ReclaimedRemote(());

impl RemoteHandle<'_> {
    pub(crate) fn value<Table>(&self) -> ChildHandleValue<Table> {
        ChildHandleValue::from_raw(self.value.get())
    }

    pub(crate) fn reclaim(mut self) -> io::Result<ReclaimedRemote> {
        close_remote(self.process, self.value).map(|()| {
            self.state = ReclaimState::Reclaimed;
            ReclaimedRemote(())
        })
    }
}

impl Drop for RemoteHandle<'_> {
    fn drop(&mut self) {
        if matches!(self.state, ReclaimState::Open) {
            let _ = close_remote(self.process, self.value);
        }
    }
}

fn close_remote(process: BorrowedHandle<'_>, value: std::num::NonZeroIsize) -> io::Result<()> {
    // SAFETY: `GetCurrentProcess` takes no arguments, cannot fail, and
    // returns the current-process pseudo-handle. The value is a constant
    // that stays valid for the lifetime of the process and is never closed.
    let current = unsafe { GetCurrentProcess() };
    duplicate_between(
        raw(process),
        value.get() as HANDLE,
        current,
        Inheritability::Private,
        REMOTE_CLOSE_OPTIONS,
    )
    .map(drop)
}

pub(crate) fn duplicate_remote<'a>(
    source: BorrowedHandle<'_>,
    target_process: BorrowedHandle<'a>,
    inheritability: Inheritability,
) -> io::Result<RemoteHandle<'a>> {
    let mut value = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` has no preconditions and returns a stable
    // pseudo-handle which is not closed by this module.
    let current = unsafe { GetCurrentProcess() };
    // SAFETY: both process handles and `source` remain valid. The returned
    // numeric handle belongs to `target_process` and is owned by RemoteHandle.
    let result = unsafe {
        DuplicateHandle(
            current,
            raw(source),
            raw(target_process),
            &mut value,
            0,
            inheritability.as_win32(),
            DUPLICATE_SAME_ACCESS,
        )
    };
    match classify_win32_call(result) {
        Win32CallOutcome::Failed => return Err(io::Error::last_os_error()),
        Win32CallOutcome::Succeeded => {}
    }
    // SAFETY: successful DuplicateHandle returns a non-null handle value in
    // the target process and ownership transfers to RemoteHandle.
    let value = unsafe { std::num::NonZeroIsize::new_unchecked(value as isize) };
    Ok(RemoteHandle {
        process: target_process,
        value,
        state: ReclaimState::Open,
        marker: PhantomData,
    })
}

pub(crate) fn standard_handle(stream: StandardStream) -> io::Result<Option<OwnedHandle>> {
    let id = match stream {
        StandardStream::Input => STD_INPUT_HANDLE,
        StandardStream::Output => STD_OUTPUT_HANDLE,
        StandardStream::Error => STD_ERROR_HANDLE,
    };
    // SAFETY: GetStdHandle has no pointer preconditions.
    let handle = unsafe { GetStdHandle(id) };
    standard_handle_value(handle)
}

fn standard_handle_value(handle: HANDLE) -> io::Result<Option<OwnedHandle>> {
    if !is_valid_handle(handle) {
        return Ok(None);
    }
    // SAFETY: GetStdHandle returned a live borrowed handle. The borrow is used
    // only during DuplicateHandle and is never closed.
    let borrowed = unsafe { BorrowedHandle::borrow_raw(handle as RawHandle) };
    duplicate_local(borrowed, Inheritability::Private).map(Some)
}

pub(crate) fn null_handle(access: NullAccess) -> io::Result<OwnedHandle> {
    let name = [u16::from(b'N'), u16::from(b'U'), u16::from(b'L'), 0];
    let desired = match access {
        NullAccess::Read => GENERIC_READ,
        NullAccess::Write => GENERIC_WRITE,
    };
    // SAFETY: `name` is NUL-terminated; optional pointers are null. The return
    // value is transferred into OwnedHandle on success.
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
    owned(handle)
}

fn null_share_mode() -> u32 {
    [FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE]
        .into_iter()
        .fold(0, std::ops::BitOr::bitor)
}

pub(crate) struct Pipe {
    pub(crate) parent: OwnedHandle,
    pub(crate) child: OwnedHandle,
}

enum Win32CallOutcome {
    Failed,
    Succeeded,
}

const fn classify_win32_call(result: i32) -> Win32CallOutcome {
    match result {
        0 => Win32CallOutcome::Failed,
        _ => Win32CallOutcome::Succeeded,
    }
}

const _: () = assert!(matches!(classify_win32_call(0), Win32CallOutcome::Failed));
const _: () = assert!(matches!(
    classify_win32_call(1),
    Win32CallOutcome::Succeeded
));

pub(crate) fn create_pipe(direction: PipeDirection) -> io::Result<Pipe> {
    create_pipe_with(direction, |read, write| {
        // SAFETY: both output pointers are valid. Null security attributes make
        // both initial handles private; the child end is duplicated immediately
        // before CreateProcessW.
        unsafe { CreatePipe(read, write, ptr::null::<SECURITY_ATTRIBUTES>(), 0) }
    })
}

fn create_pipe_with(
    direction: PipeDirection,
    create: fn(&mut HANDLE, &mut HANDLE) -> i32,
) -> io::Result<Pipe> {
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    match classify_win32_call(create(&mut read, &mut write)) {
        Win32CallOutcome::Failed => return Err(io::Error::last_os_error()),
        Win32CallOutcome::Succeeded => {}
    }
    // SAFETY: successful CreatePipe returns a valid owned read handle.
    let read = unsafe { OwnedHandle::from_raw_handle(read as RawHandle) };
    // SAFETY: successful CreatePipe returns a distinct valid owned write handle.
    let write = unsafe { OwnedHandle::from_raw_handle(write as RawHandle) };
    match direction {
        PipeDirection::ParentReads => Ok(Pipe {
            parent: read,
            child: write,
        }),
        PipeDirection::ParentWrites => Ok(Pipe {
            parent: write,
            child: read,
        }),
    }
}

pub(crate) fn open_parent_process(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: OpenProcess has no pointer preconditions.
    let handle = unsafe { OpenProcess(PARENT_PROCESS_ACCESS, 0, pid) };
    owned(handle)
}

pub(crate) fn validate_process_handle(handle: BorrowedHandle<'_>) -> io::Result<()> {
    // SAFETY: the borrowed handle remains valid for the query.
    if unsafe { GetProcessId(raw(handle)) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn create_job() -> io::Result<OwnedHandle> {
    // SAFETY: null arguments request an unnamed Job with default security.
    owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })
}

pub(crate) fn validate_job_handle(handle: BorrowedHandle<'_>) -> io::Result<()> {
    query_job_limits(handle).map(drop)
}

pub(crate) fn set_job_close_policy(
    handle: BorrowedHandle<'_>,
    policy: crate::JobClosePolicy,
) -> io::Result<()> {
    let mut limits = query_job_limits(handle)?;
    match policy {
        crate::JobClosePolicy::PreserveProcesses => {
            limits.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        crate::JobClosePolicy::TerminateProcesses => {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
    }
    set_job_limits(handle, &limits)
}

fn set_job_limits(
    handle: BorrowedHandle<'_>,
    limits: &JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
) -> io::Result<()> {
    // SAFETY: `limits` is the exact structure required by the information
    // class and remains readable for the call.
    if unsafe {
        SetInformationJobObject(
            raw(handle),
            JobObjectExtendedLimitInformation,
            (limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
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
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: `limits` is correctly sized writable storage for the selected
    // information class; the optional returned-size pointer is null.
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

#[cfg(test)]
pub(crate) fn job_close_policy(handle: BorrowedHandle<'_>) -> io::Result<crate::JobClosePolicy> {
    let flags = query_job_limits(handle)?.BasicLimitInformation.LimitFlags;
    if flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0 {
        Ok(crate::JobClosePolicy::PreserveProcesses)
    } else {
        Ok(crate::JobClosePolicy::TerminateProcesses)
    }
}

pub(crate) fn assign_job(job: BorrowedHandle<'_>, process: BorrowedHandle<'_>) -> io::Result<()> {
    // SAFETY: both handles remain valid for the call.
    bool_result(unsafe { AssignProcessToJobObject(raw(job), raw(process)) })
}

pub(crate) fn terminate_job(job: BorrowedHandle<'_>, exit_code: u32) -> io::Result<()> {
    // SAFETY: the Job handle remains valid for the call.
    bool_result(unsafe { TerminateJobObject(raw(job), exit_code) })
}

pub(crate) struct AttributeList {
    _storage: Box<[usize]>,
    pointer: ptr::NonNull<c_void>,
}

struct AttributeLayout {
    words: usize,
    bytes: usize,
}

struct AttributeInitialization;

#[derive(Clone, Copy)]
enum AttributeProbeMode {
    Native,
    #[cfg(test)]
    Failure,
}

impl AttributeProbeMode {
    fn invoke(self, count: u32, bytes: &mut usize) -> (i32, io::Error) {
        match self {
            Self::Native => {
                // SAFETY: a null list pointer is the documented size probe and
                // `bytes` is writable for the returned size.
                let result =
                    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, bytes) };
                (result, io::Error::last_os_error())
            }
            #[cfg(test)]
            Self::Failure => (0, io::Error::from_raw_os_error(5)),
        }
    }
}

#[derive(Clone, Copy)]
enum AttributeInitializationMode {
    Native,
    #[cfg(test)]
    Failure,
}

impl AttributeInitializationMode {
    fn invoke(
        self,
        pointer: ptr::NonNull<c_void>,
        count: u32,
        bytes: &mut usize,
    ) -> (i32, io::Error) {
        match self {
            Self::Native => {
                // SAFETY: the pointer addresses stable writable storage sized
                // from the successful probe and `bytes` reports that size.
                let result =
                    unsafe { InitializeProcThreadAttributeList(pointer.as_ptr(), count, 0, bytes) };
                (result, io::Error::last_os_error())
            }
            #[cfg(test)]
            Self::Failure => (0, io::Error::from_raw_os_error(5)),
        }
    }
}

fn attribute_layout(
    probe: i32,
    probe_error: io::Error,
    bytes: usize,
) -> io::Result<AttributeLayout> {
    match classify_win32_call(probe) {
        Win32CallOutcome::Failed => {}
        Win32CallOutcome::Succeeded => {
            return Err(io::Error::other(
                "attribute-list size probe unexpectedly succeeded",
            ));
        }
    }
    if probe_error.raw_os_error() != Some(INSUFFICIENT_BUFFER_CODE) {
        return Err(probe_error);
    }
    if bytes == 0 {
        return Err(probe_error);
    }
    let Some(rounded_bytes) = bytes.checked_add(size_of::<usize>() - 1) else {
        return Err(io::Error::other("attribute list is too large"));
    };
    let words = rounded_bytes / size_of::<usize>();
    let bytes = rounded_bytes - (rounded_bytes % size_of::<usize>());
    Ok(AttributeLayout { words, bytes })
}

fn attribute_initialization(
    initialized: i32,
    initialize_error: io::Error,
) -> io::Result<AttributeInitialization> {
    match classify_win32_call(initialized) {
        Win32CallOutcome::Failed => Err(initialize_error),
        Win32CallOutcome::Succeeded => Ok(AttributeInitialization),
    }
}

impl AttributeList {
    pub(crate) fn new(count: u32) -> io::Result<Self> {
        Self::new_with_modes(
            count,
            AttributeProbeMode::Native,
            AttributeInitializationMode::Native,
        )
    }

    fn new_with_modes(
        count: u32,
        probe_mode: AttributeProbeMode,
        initialization_mode: AttributeInitializationMode,
    ) -> io::Result<Self> {
        let mut bytes = 0_usize;
        let (probe, probe_error) = probe_mode.invoke(count, &mut bytes);
        let layout = attribute_layout(probe, probe_error, bytes)?;
        let mut storage = vec![0_usize; layout.words].into_boxed_slice();
        // SAFETY: slice pointers are non-null even for an empty slice; the
        // positive validated probe additionally makes this allocation nonempty.
        let pointer = unsafe { ptr::NonNull::new_unchecked(storage.as_mut_ptr().cast()) };
        let mut actual = layout.bytes;
        let (initialized, initialize_error) =
            initialization_mode.invoke(pointer, count, &mut actual);
        let _initialization = attribute_initialization(initialized, initialize_error)?;
        Ok(Self {
            _storage: storage,
            pointer,
        })
    }

    pub(crate) fn set_handle_list<Table>(
        &mut self,
        handles: &[ChildHandleValue<Table>],
    ) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(handles),
        )
    }

    pub(crate) fn set_parent<Table>(&mut self, parent: &ChildHandleValue<Table>) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
            (parent as *const ChildHandleValue<Table>).cast(),
            size_of::<isize>(),
        )
    }

    pub(crate) fn set_mitigation(&mut self, words: &[u64; 2]) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY as usize,
            words.as_ptr().cast(),
            size_of::<[u64; 2]>(),
        )
    }

    pub(crate) fn set_jobs<Table>(&mut self, jobs: &[ChildHandleValue<Table>]) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            jobs.as_ptr().cast(),
            size_of_val(jobs),
        )
    }

    pub(crate) fn set_pseudoconsole(
        &mut self,
        pseudoconsole: crate::BorrowedPseudoConsole<'_>,
    ) -> io::Result<()> {
        self.update(
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            pseudoconsole.value.get() as *const c_void,
            size_of::<isize>(),
        )
    }

    fn update(&mut self, attribute: usize, value: *const c_void, bytes: usize) -> io::Result<()> {
        // SAFETY: the list is initialized, `value` points to `bytes` readable
        // bytes (or is the documented HPCON value), and the transaction keeps
        // every backing allocation stable through CreateProcessW.
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer().as_ptr(),
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

    fn pointer(&self) -> ptr::NonNull<c_void> {
        self.pointer
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        #[cfg(test)]
        ATTRIBUTE_LIST_DROPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: initialization succeeded once and this is its sole owner.
        unsafe { DeleteProcThreadAttributeList(self.pointer().as_ptr()) };
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StandardHandles<Table> {
    pub(crate) stdin: ChildHandleValue<Table>,
    pub(crate) stdout: ChildHandleValue<Table>,
    pub(crate) stderr: ChildHandleValue<Table>,
}

#[derive(Clone, Copy)]
pub(crate) enum StartupStdio<Table> {
    Ordinary(StandardHandles<Table>),
    PseudoConsole,
}

pub(crate) struct ProcessRequest<'a, Table> {
    pub(crate) application: &'a [u16],
    pub(crate) command_line: &'a mut [u16],
    pub(crate) environment: Option<&'a [u16]>,
    pub(crate) current_dir: Option<&'a [u16]>,
    pub(crate) stdio: StartupStdio<Table>,
    pub(crate) inheritability: Inheritability,
    pub(crate) creation_flags: CreationFlags,
    pub(crate) initial_state: InitialState,
    pub(crate) attributes: Option<&'a AttributeList>,
}

pub(crate) struct CreatedProcess {
    pub(crate) process: OwnedHandle,
    pub(crate) thread: OwnedHandle,
    pub(crate) pid: u32,
}

pub(crate) fn create_process<Table>(
    request: &mut ProcessRequest<'_, Table>,
) -> io::Result<CreatedProcess> {
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = if request.attributes.is_some() {
        STARTUP_INFO_EX_SIZE
    } else {
        STARTUP_INFO_SIZE
    };
    set_standard_handles(&mut startup, &request.stdio);
    startup.lpAttributeList = request
        .attributes
        .map_or(ptr::null_mut(), |attributes| attributes.pointer().as_ptr());

    let mut flags = request
        .creation_flags
        .bits()
        .wrapping_add(CREATE_UNICODE_ENVIRONMENT);
    match request.initial_state {
        InitialState::Suspended => flags = flags.wrapping_add(CREATE_SUSPENDED),
    }
    if request.attributes.is_some() {
        flags = flags.wrapping_add(EXTENDED_STARTUPINFO_PRESENT);
    }
    let environment = request
        .environment
        .map_or(ptr::null(), |block| block.as_ptr().cast());
    let current_dir = request.current_dir.map_or(ptr::null(), <[u16]>::as_ptr);
    let mut information = PROCESS_INFORMATION::default();

    // SAFETY: all UTF-16 buffers are correctly terminated and remain live;
    // command_line is writable as required by CreateProcessW. Startup handles
    // and every attribute backing allocation remain live for the call.
    let result = unsafe {
        CreateProcessW(
            request.application.as_ptr(),
            request.command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            request.inheritability.as_win32(),
            flags,
            environment,
            current_dir,
            &startup.StartupInfo,
            &mut information,
        )
    };
    match classify_win32_call(result) {
        Win32CallOutcome::Failed => return Err(io::Error::last_os_error()),
        Win32CallOutcome::Succeeded => {}
    }

    // SAFETY: successful CreateProcessW transfers a valid process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess as RawHandle) };
    // SAFETY: successful CreateProcessW transfers a distinct valid thread handle.
    let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread as RawHandle) };
    Ok(CreatedProcess {
        process,
        thread,
        pid: information.dwProcessId,
    })
}

fn set_standard_handles<Table>(startup: &mut STARTUPINFOEXW, stdio: &StartupStdio<Table>) {
    startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
    if let StartupStdio::Ordinary(handles) = stdio {
        startup.StartupInfo.hStdInput = handles.stdin.as_raw() as HANDLE;
        startup.StartupInfo.hStdOutput = handles.stdout.as_raw() as HANDLE;
        startup.StartupInfo.hStdError = handles.stderr.as_raw() as HANDLE;
    }
}

pub(crate) fn child_handle_value<Table>(handle: BorrowedHandle<'_>) -> ChildHandleValue<Table> {
    ChildHandleValue::from_raw(handle.as_raw_handle() as isize)
}

pub(crate) fn wait_process(process: BorrowedHandle<'_>) -> io::Result<()> {
    // SAFETY: the process handle remains valid while waiting.
    let result = unsafe { WaitForSingleObject(raw(process), INFINITE) };
    wait_result(result)
}

fn wait_result(result: u32) -> io::Result<()> {
    match result {
        WAIT_OBJECT_0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

pub(crate) fn try_wait_process(process: BorrowedHandle<'_>) -> io::Result<bool> {
    // SAFETY: the process handle remains valid while querying.
    let result = unsafe { WaitForSingleObject(raw(process), 0) };
    try_wait_result(result)
}

fn try_wait_result(result: u32) -> io::Result<bool> {
    match result {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn wait_process_for_test(
    process: BorrowedHandle<'_>,
    timeout_millis: u32,
) -> io::Result<bool> {
    // SAFETY: the process handle remains valid while querying.
    match unsafe { WaitForSingleObject(raw(process), timeout_millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn cleanup_process_for_test(process: BorrowedHandle<'_>) {
    // SAFETY: tests pass a duplicate with the source process handle's access.
    let _ = unsafe { TerminateProcess(raw(process), 1) };
    // SAFETY: the same borrowed process handle remains valid for the wait.
    let _ = unsafe { WaitForSingleObject(raw(process), 5_000) };
}

pub(crate) fn exit_status(process: BorrowedHandle<'_>) -> io::Result<ExitStatus> {
    let mut code = 0_u32;
    // SAFETY: `code` is writable and the process handle remains valid.
    let succeeded = unsafe { GetExitCodeProcess(raw(process), &mut code) };
    exit_status_result(succeeded, code)
}

fn exit_status_result(succeeded: i32, code: u32) -> io::Result<ExitStatus> {
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ExitStatus::from_raw(code))
    }
}

pub(crate) fn terminate_process(process: BorrowedHandle<'_>, exit_code: u32) -> io::Result<()> {
    // SAFETY: the process handle remains valid for the call.
    bool_result(unsafe { TerminateProcess(raw(process), exit_code) })
}

pub(crate) struct PreviousSuspendCount(pub(crate) u32);

pub(crate) fn resume_thread(thread: BorrowedHandle<'_>) -> io::Result<PreviousSuspendCount> {
    // SAFETY: the thread handle remains valid for the call.
    let previous = unsafe { ResumeThread(raw(thread)) };
    resume_result(previous)
}

fn resume_result(previous: u32) -> io::Result<PreviousSuspendCount> {
    if previous == u32::MAX {
        Err(io::Error::last_os_error())
    } else {
        Ok(PreviousSuspendCount(previous))
    }
}

pub(crate) struct ReadCount(pub(crate) usize);

pub(crate) struct WriteCount(pub(crate) usize);

pub(crate) fn read_handle(handle: BorrowedHandle<'_>, buffer: &mut [u8]) -> io::Result<ReadCount> {
    if buffer.is_empty() {
        return Ok(ReadCount(0));
    }
    let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    let mut read = 0_u32;
    // SAFETY: buffer is writable for `length` bytes, the synchronous handle
    // remains valid, and a null OVERLAPPED requests synchronous I/O.
    if unsafe {
        ReadFile(
            raw(handle),
            buffer.as_mut_ptr(),
            length,
            &mut read,
            ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        read_error(error)
    } else {
        Ok(ReadCount(read as usize))
    }
}

fn read_error(error: io::Error) -> io::Result<ReadCount> {
    if matches!(
        error.raw_os_error(),
        Some(code)
            if win32_code_is(code, ERROR_BROKEN_PIPE)
                || win32_code_is(code, ERROR_HANDLE_EOF)
    ) {
        Ok(ReadCount(0))
    } else {
        Err(error)
    }
}

pub(crate) fn write_handle(handle: BorrowedHandle<'_>, buffer: &[u8]) -> io::Result<WriteCount> {
    if buffer.is_empty() {
        return Ok(WriteCount(0));
    }
    let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    let mut written = 0_u32;
    // SAFETY: buffer is readable for `length` bytes, the synchronous handle
    // remains valid, and a null OVERLAPPED requests synchronous I/O.
    if unsafe {
        WriteFile(
            raw(handle),
            buffer.as_ptr(),
            length,
            &mut written,
            ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(WriteCount(written as usize))
    }
}

pub(crate) fn environment_strings() -> io::Result<Vec<(OsString, OsString)>> {
    environment_strings_with(get_environment_strings)
}

fn get_environment_strings() -> *mut u16 {
    // SAFETY: GetEnvironmentStringsW returns a process-owned double-NUL block
    // which remains valid until FreeEnvironmentStringsW below.
    unsafe { GetEnvironmentStringsW() }
}

fn environment_strings_with(get: fn() -> *mut u16) -> io::Result<Vec<(OsString, OsString)>> {
    let base = get();
    let base = environment_base(base)?;
    let guard = EnvironmentBlock(base);
    let mut entries = Vec::new();
    let mut cursor = guard.0.as_ptr();
    loop {
        // SAFETY: cursor walks one NUL-terminated entry at a time inside the
        // double-NUL-terminated environment block.
        match classify_environment_unit(unsafe { *cursor }) {
            EnvironmentUnit::Terminator => break,
            EnvironmentUnit::Content => {}
        }
        let mut length = 0_usize;
        loop {
            // SAFETY: the OS-provided current entry is NUL-terminated and
            // `length` advances only within that entry.
            let unit = unsafe { cursor.add(length) };
            // SAFETY: `unit` addresses the current NUL-terminated entry.
            match classify_environment_unit(unsafe { *unit }) {
                EnvironmentUnit::Terminator => break,
                EnvironmentUnit::Content => {}
            }
            length = length.saturating_add(1);
        }
        // SAFETY: the just-computed range lies within the current entry.
        let entry = unsafe { std::slice::from_raw_parts(cursor, length) };
        entries.extend(parse_environment_entry(entry));
        let advance = length.saturating_add(1);
        // SAFETY: `advance` moves to the first unit after this entry's
        // terminator, which is still inside the double-NUL-terminated block.
        cursor = unsafe { cursor.add(advance) };
    }
    Ok(entries)
}

fn environment_base(base: *mut u16) -> io::Result<ptr::NonNull<u16>> {
    ptr::NonNull::new(base).ok_or_else(io::Error::last_os_error)
}

fn parse_environment_entry(entry: &[u16]) -> Option<(OsString, OsString)> {
    let searchable = entry.get(1..)?;
    let separator = searchable
        .iter()
        .position(|unit| *unit == u16::from(b'='))?
        .saturating_add(1);
    let name: Vec<u16> = entry.iter().take(separator).copied().collect();
    let value: Vec<u16> = entry
        .iter()
        .skip(separator.saturating_add(1))
        .copied()
        .collect();
    Some((OsString::from_wide(&name), OsString::from_wide(&value)))
}

pub(crate) fn compare_ordinal(left: &[u16], right: &[u16]) -> Ordering {
    let left_len = i32::try_from(left.len()).unwrap_or(i32::MAX);
    let right_len = i32::try_from(right.len()).unwrap_or(i32::MAX);
    // SAFETY: both pointers are readable for their checked lengths.
    let comparison =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    ordinal_result(comparison, left, right)
}

fn ordinal_result(comparison: i32, left: &[u16], right: &[u16]) -> Ordering {
    match comparison {
        CSTR_LESS_THAN => Ordering::Less,
        CSTR_EQUAL => Ordering::Equal,
        CSTR_GREATER_THAN => Ordering::Greater,
        _ => left.cmp(right),
    }
}

pub(crate) fn program_exists(path: &[u16]) -> bool {
    // SAFETY: callers supply a NUL-terminated path buffer.
    unsafe { GetFileAttributesW(path.as_ptr()) != INVALID_FILE_ATTRIBUTES }
}

#[derive(Debug)]
pub(crate) struct SystemPath(OsString);

impl SystemPath {
    pub(crate) fn into_path_buf(self) -> std::path::PathBuf {
        std::path::PathBuf::from(self.0)
    }
}

pub(crate) fn system_directory() -> io::Result<SystemPath> {
    system_path(SystemDirectory::System)
}

pub(crate) fn windows_directory() -> io::Result<SystemPath> {
    system_path(SystemDirectory::Windows)
}

fn system_path(directory: SystemDirectory) -> io::Result<SystemPath> {
    let mut buffer = vec![0_u16; MAXIMUM_WINDOWS_PATH_UNITS as usize];
    let length = match directory {
        SystemDirectory::Windows => {
            // SAFETY: buffer is writable for its reported length.
            unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), MAXIMUM_WINDOWS_PATH_UNITS) }
        }
        SystemDirectory::System => {
            // SAFETY: buffer is writable for its reported length.
            unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), MAXIMUM_WINDOWS_PATH_UNITS) }
        }
    };
    let error = io::Error::last_os_error();
    decode_system_path(buffer, length, error)
}

fn decode_system_path(
    mut buffer: Vec<u16>,
    length: u32,
    error: io::Error,
) -> io::Result<SystemPath> {
    if length == 0 {
        return Err(error);
    }
    if length >= MAXIMUM_WINDOWS_PATH_UNITS {
        return Err(io::Error::other(
            "Windows directory exceeds the maximum path length",
        ));
    }
    let length = length as usize;
    buffer.truncate(length);
    Ok(SystemPath(OsString::from_wide(&buffer)))
}

fn raw(handle: BorrowedHandle<'_>) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

fn win32_code_is(actual: i32, expected: u32) -> bool {
    i32::try_from(expected).ok() == Some(actual)
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if is_valid_handle(handle) {
        // SAFETY: callers pass a newly-created or newly-duplicated handle and
        // transfer its sole local ownership into this function.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_valid_handle(handle: HANDLE) -> bool {
    !handle.is_null() && handle != INVALID_HANDLE_VALUE
}

fn bool_result(result: i32) -> io::Result<()> {
    match classify_win32_call(result) {
        Win32CallOutcome::Failed => Err(io::Error::last_os_error()),
        Win32CallOutcome::Succeeded => Ok(()),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) fn current_process_handle_count() -> io::Result<u32> {
    let mut count = 0;
    // SAFETY: GetCurrentProcess has no preconditions and returns a stable pseudo-handle.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: the pseudo-handle remains valid and count is writable DWORD storage.
    if unsafe { GetProcessHandleCount(process, &mut count) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::fs::File;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsHandle, AsRawHandle};

    use super::*;
    fn process_handle_count(process: BorrowedHandle<'_>) -> io::Result<u32> {
        let mut count = 0;
        // SAFETY: `process` remains valid and count is writable DWORD storage.
        if unsafe { GetProcessHandleCount(raw(process), &mut count) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count)
        }
    }

    fn current_process() -> BorrowedHandle<'static> {
        // SAFETY: `GetCurrentProcess` cannot fail and returns the
        // current-process pseudo-handle, a constant that stays valid for the
        // whole process lifetime. `BorrowedHandle` never closes what it borrows,
        // so a `'static` borrow of it can never dangle or double-close.
        let current = unsafe { GetCurrentProcess() };
        // SAFETY: the pseudo-handle above has process lifetime and is never
        // closed through the returned borrow.
        unsafe { BorrowedHandle::borrow_raw(current as RawHandle) }
    }

    #[test]
    fn pipe_null_and_duplicate_primitives_preserve_ownership() -> io::Result<()> {
        assert_eq!(Inheritability::Private.as_win32(), 0);
        assert_eq!(Inheritability::Inheritable.as_win32(), 1);
        assert_eq!(
            null_share_mode(),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        );
        let readable_null = null_handle(NullAccess::Read)?;
        let writable_null = null_handle(NullAccess::Write)?;
        assert_eq!(read_handle(readable_null.as_handle(), &mut [])?.0, 0);
        assert_eq!(write_handle(writable_null.as_handle(), &[])?.0, 0);
        assert_eq!(write_handle(writable_null.as_handle(), b"discard")?.0, 7);

        let parent_reads = create_pipe(PipeDirection::ParentReads)?;
        assert_eq!(write_handle(parent_reads.child.as_handle(), b"a")?.0, 1);
        let mut byte = [0_u8; 1];
        assert_eq!(
            read_handle(parent_reads.parent.as_handle(), &mut byte)?.0,
            1
        );
        assert_eq!(byte, [b'a']);

        let parent_writes = create_pipe(PipeDirection::ParentWrites)?;
        assert_eq!(write_handle(parent_writes.parent.as_handle(), b"b")?.0, 1);
        assert_eq!(
            read_handle(parent_writes.child.as_handle(), &mut byte)?.0,
            1
        );
        assert_eq!(byte, [b'b']);

        let private = duplicate_local(writable_null.as_handle(), Inheritability::Private)?;
        let inheritable = duplicate_local(writable_null.as_handle(), Inheritability::Inheritable)?;
        drop((private, inheritable));

        let mut host = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "ping -n 5 127.0.0.1 >nul"])
            .spawn()?;
        let target = open_parent_process(host.id())?;
        drop(target);
        let before = process_handle_count(host.as_handle())?;
        let local_before = process_handle_count(current_process())?;
        let remote = duplicate_remote(
            writable_null.as_handle(),
            host.as_handle(),
            Inheritability::Inheritable,
        )?;
        assert_ne!(remote.value::<crate::resource::CurrentTable>().as_raw(), 0);
        assert!(process_handle_count(host.as_handle())? > before);
        let _reclaimed = remote.reclaim()?;
        assert_eq!(process_handle_count(host.as_handle())?, before);

        let remote = duplicate_remote(
            writable_null.as_handle(),
            host.as_handle(),
            Inheritability::Inheritable,
        )?;
        assert!(process_handle_count(host.as_handle())? > before);
        drop(remote);
        assert_eq!(process_handle_count(host.as_handle())?, before);
        assert_eq!(process_handle_count(current_process())?, local_before);
        let _ = host.kill();
        let _ = host.wait();
        Ok(())
    }

    #[test]
    fn resume_reports_the_previous_suspend_count() -> io::Result<()> {
        let system = system_directory()?.into_path_buf();
        let mut application: Vec<u16> = system.as_os_str().encode_wide().collect();
        if !application.ends_with(&[u16::from(b'\\')]) {
            application.push(u16::from(b'\\'));
        }
        application.extend("cmd.exe".encode_utf16());
        application.push(0);
        let mut command_line: Vec<u16> = "\"cmd.exe\" /D /C exit /b 0\0".encode_utf16().collect();
        let input = null_handle(NullAccess::Read)?;
        let output = null_handle(NullAccess::Write)?;
        let mut request = ProcessRequest {
            application: &application,
            command_line: &mut command_line,
            environment: None,
            current_dir: None,
            stdio: StartupStdio::Ordinary(StandardHandles {
                stdin: child_handle_value::<crate::resource::SelectedTable<'static>>(
                    input.as_handle(),
                ),
                stdout: child_handle_value::<crate::resource::SelectedTable<'static>>(
                    output.as_handle(),
                ),
                stderr: child_handle_value::<crate::resource::SelectedTable<'static>>(
                    output.as_handle(),
                ),
            }),
            inheritability: Inheritability::Private,
            creation_flags: CreationFlags::new(
                crate::options::CreationPolicy::default(),
                crate::ConsoleMode::Inherit,
            ),
            initial_state: InitialState::Suspended,
            attributes: None,
        };
        let created = create_process(&mut request)?;
        assert!(!try_wait_process(created.process.as_handle())?);
        let job = create_job()?;
        assign_job(job.as_handle(), created.process.as_handle())?;
        let first = resume_thread(created.thread.as_handle());
        let second = resume_thread(created.thread.as_handle());
        cleanup_process_for_test(created.process.as_handle());
        assert_eq!(first?.0, 1);
        assert_eq!(second?.0, 0);

        let mut missing_application: Vec<u16> =
            format!("C:\\windows-spawn-missing-{}.exe", std::process::id())
                .encode_utf16()
                .collect();
        missing_application.push(0);
        let mut missing_command: Vec<u16> = "windows-spawn-missing.exe\0".encode_utf16().collect();
        let mut missing_request = ProcessRequest {
            application: &missing_application,
            command_line: &mut missing_command,
            environment: None,
            current_dir: None,
            stdio: request.stdio,
            inheritability: Inheritability::Private,
            creation_flags: CreationFlags::new(
                crate::options::CreationPolicy::default(),
                crate::ConsoleMode::Inherit,
            ),
            initial_state: InitialState::Suspended,
            attributes: None,
        };
        assert!(create_process(&mut missing_request).is_err());
        Ok(())
    }

    #[test]
    fn jobs_attributes_and_standard_handles_cover_all_ffi_shapes() -> io::Result<()> {
        let drops_before = ATTRIBUTE_LIST_DROPS.load(std::sync::atomic::Ordering::Relaxed);
        let job = create_job()?;
        validate_job_handle(job.as_handle())?;
        set_job_close_policy(job.as_handle(), crate::JobClosePolicy::TerminateProcesses)?;
        assert_ne!(
            query_job_limits(job.as_handle())?
                .BasicLimitInformation
                .LimitFlags
                & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
        set_job_close_policy(job.as_handle(), crate::JobClosePolicy::PreserveProcesses)?;
        assert_eq!(
            query_job_limits(job.as_handle())?
                .BasicLimitInformation
                .LimitFlags
                & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            0
        );
        let process = open_parent_process(std::process::id())?;
        let inherited = duplicate_local(job.as_handle(), Inheritability::Inheritable)?;
        let handles = [child_handle_value::<crate::resource::CurrentTable>(
            inherited.as_handle(),
        )];
        let jobs = [child_handle_value::<crate::resource::CurrentTable>(
            job.as_handle(),
        )];
        let parent = child_handle_value::<crate::resource::CurrentTable>(process.as_handle());
        let words = [1_u64, 0_u64];
        let mut attributes = AttributeList::new(4)?;
        attributes.set_handle_list(&handles)?;
        attributes.set_parent(&parent)?;
        attributes.set_mitigation(&words)?;
        attributes.set_jobs(&jobs)?;
        drop(attributes);

        let mut pseudoconsole = AttributeList::new(1)?;
        let owner = ();
        let value = std::num::NonZeroIsize::new(1)
            .ok_or_else(|| io::Error::other("nonzero test pseudoconsole expected"))?;
        // SAFETY: the sentinel is used only to exercise attribute encoding and is never submitted to CreateProcessW.
        let borrowed = unsafe { crate::BorrowedPseudoConsole::from_raw(value, &owner) };
        let _ = pseudoconsole.set_pseudoconsole(borrowed);
        drop(pseudoconsole);
        assert!(ATTRIBUTE_LIST_DROPS.load(std::sync::atomic::Ordering::Relaxed) > drops_before);

        let input = standard_handle(StandardStream::Input)?;
        let output = standard_handle(StandardStream::Output)?;
        let error = standard_handle(StandardStream::Error)?;
        assert!(input.is_some() || output.is_some() || error.is_some());
        assert_eq!(
            ChildHandleValue::<crate::resource::CurrentTable>::INVALID.as_raw(),
            -1
        );
        Ok(())
    }

    #[test]
    fn startup_info_distinguishes_pseudoconsole_and_ordinary_stdio() {
        let mut conpty = STARTUPINFOEXW::default();
        set_standard_handles(
            &mut conpty,
            &StartupStdio::<crate::resource::CurrentTable>::PseudoConsole,
        );
        assert_ne!(conpty.StartupInfo.dwFlags & STARTF_USESTDHANDLES, 0);
        assert!(conpty.StartupInfo.hStdInput.is_null());
        assert!(conpty.StartupInfo.hStdOutput.is_null());
        assert!(conpty.StartupInfo.hStdError.is_null());

        let mut ordinary = STARTUPINFOEXW::default();
        set_standard_handles(
            &mut ordinary,
            &StartupStdio::Ordinary(StandardHandles {
                stdin: ChildHandleValue::<crate::resource::CurrentTable>::from_raw(1),
                stdout: ChildHandleValue::<crate::resource::CurrentTable>::from_raw(2),
                stderr: ChildHandleValue::<crate::resource::CurrentTable>::from_raw(3),
            }),
        );
        assert_ne!(ordinary.StartupInfo.dwFlags & STARTF_USESTDHANDLES, 0);
        assert_eq!(ordinary.StartupInfo.hStdInput as isize, 1);
        assert_eq!(ordinary.StartupInfo.hStdOutput as isize, 2);
        assert_eq!(ordinary.StartupInfo.hStdError as isize, 3);
    }

    #[test]
    fn environment_paths_comparison_and_error_helpers_work() -> io::Result<()> {
        let drops_before = ENVIRONMENT_BLOCK_DROPS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(!environment_strings()?.is_empty());
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

        let system = system_directory()?.into_path_buf();
        let windows = windows_directory()?.into_path_buf();
        assert!(!system.as_os_str().is_empty() && !windows.as_os_str().is_empty());
        let mut executable = system;
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
        assert!(win32_code_is(
            i32::try_from(ERROR_BROKEN_PIPE).unwrap_or_default(),
            ERROR_BROKEN_PIPE
        ));
        assert!(!win32_code_is(123, ERROR_BROKEN_PIPE));

        assert!(open_parent_process(u32::MAX).is_err());

        let file = File::open("NUL")?;
        assert!(is_valid_handle(file.as_raw_handle() as HANDLE));
        assert!(!is_valid_handle(ptr::null_mut()));
        assert!(!is_valid_handle(INVALID_HANDLE_VALUE));
        assert!(validate_process_handle(file.as_handle()).is_err());
        assert!(validate_job_handle(file.as_handle()).is_err());
        Ok(())
    }

    #[test]
    fn deterministic_raw_failures_are_classified_without_leaks() -> io::Result<()> {
        let file = File::open("NUL")?;
        assert!(duplicate_between(
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            Inheritability::Private,
            DUPLICATE_SAME_ACCESS,
        )
        .is_err());
        assert!(
            duplicate_remote(file.as_handle(), file.as_handle(), Inheritability::Private,).is_err()
        );
        assert!(standard_handle_value(ptr::null_mut())?.is_none());
        assert!(create_pipe_with(PipeDirection::ParentReads, |_, _| 0).is_err());

        let failed_remote = RemoteHandle {
            process: file.as_handle(),
            value: std::num::NonZeroIsize::new(1)
                .ok_or_else(|| io::Error::other("nonzero remote handle expected"))?,
            state: ReclaimState::Open,
            marker: PhantomData,
        };
        assert!(failed_remote.reclaim().is_err());

        assert!(
            set_job_close_policy(file.as_handle(), crate::JobClosePolicy::TerminateProcesses,)
                .is_err()
        );
        assert!(set_job_limits(
            file.as_handle(),
            &JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default(),
        )
        .is_err());

        let probe_success = attribute_layout(1, io::Error::from_raw_os_error(5), 0);
        let Err(probe_success) = probe_success else {
            return Err(io::Error::other("size probe failure expected"));
        };
        assert_eq!(
            probe_success.to_string(),
            "attribute-list size probe unexpectedly succeeded"
        );
        let wrong_probe_error =
            attribute_layout(0, io::Error::from_raw_os_error(5), size_of::<usize>());
        let Err(wrong_probe_error) = wrong_probe_error else {
            return Err(io::Error::other("wrong probe code failure expected"));
        };
        assert_eq!(wrong_probe_error.raw_os_error(), Some(5));
        let empty_probe =
            attribute_layout(0, io::Error::from_raw_os_error(INSUFFICIENT_BUFFER_CODE), 0);
        let Err(empty_probe) = empty_probe else {
            return Err(io::Error::other("empty probe failure expected"));
        };
        assert_eq!(empty_probe.raw_os_error(), Some(INSUFFICIENT_BUFFER_CODE));
        let oversized_probe = attribute_layout(
            0,
            io::Error::from_raw_os_error(INSUFFICIENT_BUFFER_CODE),
            usize::MAX,
        );
        assert!(oversized_probe.is_err());
        let layout = attribute_layout(
            0,
            io::Error::from_raw_os_error(INSUFFICIENT_BUFFER_CODE),
            size_of::<usize>(),
        )?;
        assert_eq!(layout.words, 1);
        assert_eq!(layout.bytes, size_of::<usize>());
        let rounded_layout = attribute_layout(
            0,
            io::Error::from_raw_os_error(INSUFFICIENT_BUFFER_CODE),
            size_of::<usize>().saturating_add(1),
        )?;
        assert_eq!(rounded_layout.words, 2);
        assert_eq!(rounded_layout.bytes, size_of::<usize>().saturating_mul(2));
        assert!(attribute_initialization(0, io::Error::from_raw_os_error(5)).is_err());
        let _initialization = attribute_initialization(1, io::Error::from_raw_os_error(5))?;
        assert!(AttributeList::new_with_modes(
            1,
            AttributeProbeMode::Failure,
            AttributeInitializationMode::Native,
        )
        .is_err());
        assert!(AttributeList::new_with_modes(
            1,
            AttributeProbeMode::Native,
            AttributeInitializationMode::Failure,
        )
        .is_err());
        let mut attributes = AttributeList::new(1)?;
        assert!(attributes.update(usize::MAX, ptr::null(), 0).is_err());
        Ok(())
    }

    #[test]
    fn deterministic_wait_and_pipe_failures_are_classified() -> io::Result<()> {
        assert!(wait_result(u32::MAX).is_err());
        assert!(try_wait_result(WAIT_OBJECT_0)?);
        assert!(!try_wait_result(WAIT_TIMEOUT)?);
        assert!(try_wait_result(u32::MAX).is_err());
        assert!(exit_status_result(0, 0).is_err());
        assert_eq!(resume_result(0)?.0, 0);
        assert!(resume_result(u32::MAX).is_err());

        let parent_reads = create_pipe(PipeDirection::ParentReads)?;
        let mut byte = [0_u8; 1];
        assert!(read_handle(parent_reads.child.as_handle(), &mut byte).is_err());
        assert!(write_handle(parent_reads.parent.as_handle(), b"x").is_err());
        assert_eq!(
            read_error(io::Error::from_raw_os_error(
                i32::try_from(ERROR_BROKEN_PIPE).unwrap_or_default(),
            ))?
            .0,
            0
        );
        assert_eq!(
            read_error(io::Error::from_raw_os_error(
                i32::try_from(ERROR_HANDLE_EOF).unwrap_or_default(),
            ))?
            .0,
            0
        );
        assert!(read_error(io::Error::from_raw_os_error(5)).is_err());
        Ok(())
    }

    #[test]
    fn deterministic_environment_and_path_failures_are_classified() -> io::Result<()> {
        assert!(environment_strings_with(ptr::null_mut).is_err());
        assert!(environment_base(ptr::null_mut()).is_err());
        let mut unit = 1_u16;
        let unit_pointer = ptr::addr_of_mut!(unit);
        assert_eq!(environment_base(unit_pointer)?.as_ptr(), unit_pointer);
        assert!(parse_environment_entry(&[]).is_none());
        assert!(parse_environment_entry(&[u16::from(b'A')]).is_none());
        let parsed = parse_environment_entry(&[
            u16::from(b'='),
            u16::from(b'C'),
            u16::from(b':'),
            u16::from(b'='),
            u16::from(b'x'),
        ])
        .ok_or_else(|| io::Error::other("environment entry expected"))?;
        assert_eq!(parsed.0, OsString::from("=C:"));
        assert_eq!(parsed.1, OsString::from("x"));

        assert_eq!(ordinal_result(0, &[2], &[1]), Ordering::Greater);
        let failure = io::Error::from_raw_os_error(5);
        assert_eq!(
            decode_system_path(vec![0; 4], 0, failure)
                .unwrap_err()
                .raw_os_error(),
            Some(5)
        );
        assert!(decode_system_path(
            vec![0; MAXIMUM_WINDOWS_PATH_UNITS as usize],
            MAXIMUM_WINDOWS_PATH_UNITS,
            io::Error::from_raw_os_error(5),
        )
        .is_err());
        assert_eq!(
            decode_system_path(vec![u16::from(b'X'), 0], 1, io::Error::from_raw_os_error(5),)?
                .into_path_buf(),
            std::path::PathBuf::from("X")
        );
        Ok(())
    }
}
