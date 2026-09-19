use std::ffi::OsString;
use std::io;
use std::os::windows::io::{BorrowedHandle, OwnedHandle};

use crate::handles::Job;
use crate::resource::ChildHandleValue;
use crate::sys::{self, Inheritability, NullAccess, PipeDirection, StandardStream};
use crate::{BorrowedPseudoConsole, JobClosePolicy};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackendCall {
    StandardHandle,
    OpenNull,
    CreatePipe,
    DuplicateLocal,
    DuplicateRemote,
    CreateJob,
    ConfigureJob,
    CreateAttributes,
    SetHandleList,
    SetParent,
    SetMitigation,
    SetJobs,
    SetPseudoConsole,
    ReadEnvironment,
    QuerySystemDirectory,
    QueryWindowsDirectory,
    CreateProcess,
    ReclaimRemote,
    ResumeThread,
}

pub(crate) struct AttributeAddress<'a, Value>(&'a Value);

impl<'a, Value> AttributeAddress<'a, Value> {
    pub(crate) const fn new(value: &'a Value) -> Self {
        Self(value)
    }

    const fn get(&self) -> &'a Value {
        self.0
    }
}

pub(crate) trait SpawnBackend: Sized {
    fn before(_: BackendCall) -> io::Result<()> {
        Ok(())
    }

    fn standard_handle(stream: StandardStream) -> io::Result<Option<OwnedHandle>> {
        Self::before(BackendCall::StandardHandle)?;
        sys::standard_handle(stream)
    }

    fn null_handle(access: NullAccess) -> io::Result<OwnedHandle> {
        Self::before(BackendCall::OpenNull)?;
        sys::null_handle(access)
    }

    fn create_pipe(direction: PipeDirection) -> io::Result<sys::Pipe> {
        Self::before(BackendCall::CreatePipe)?;
        sys::create_pipe(direction)
    }

    fn duplicate_local(
        source: BorrowedHandle<'_>,
        inheritability: Inheritability,
    ) -> io::Result<OwnedHandle> {
        Self::before(BackendCall::DuplicateLocal)?;
        sys::duplicate_local(source, inheritability)
    }

    fn duplicate_remote<'a>(
        source: BorrowedHandle<'_>,
        target: BorrowedHandle<'a>,
        inheritability: Inheritability,
    ) -> io::Result<sys::RemoteHandle<'a>> {
        Self::before(BackendCall::DuplicateRemote)?;
        sys::duplicate_remote(source, target, inheritability)
    }

    fn create_job() -> io::Result<Job> {
        Self::before(BackendCall::CreateJob)?;
        sys::create_job().map(Job::from_system)
    }

    fn configure_job(job: &Job, policy: JobClosePolicy) -> io::Result<()> {
        use std::os::windows::io::AsHandle;

        Self::before(BackendCall::ConfigureJob)?;
        sys::set_job_close_policy(job.as_handle(), policy)
    }

    fn create_attributes(count: u32) -> io::Result<sys::AttributeList> {
        Self::before(BackendCall::CreateAttributes)?;
        sys::AttributeList::new(count)
    }

    fn set_handle_list<Table>(
        attributes: &mut sys::AttributeList,
        handles: &[ChildHandleValue<Table>],
    ) -> io::Result<()> {
        Self::before(BackendCall::SetHandleList)?;
        attributes.set_handle_list(handles)
    }

    fn set_parent<Table>(
        attributes: &mut sys::AttributeList,
        parent: AttributeAddress<'_, ChildHandleValue<Table>>,
    ) -> io::Result<()> {
        Self::before(BackendCall::SetParent)?;
        attributes.set_parent(parent.get())
    }

    fn set_mitigation(attributes: &mut sys::AttributeList, words: &[u64; 2]) -> io::Result<()> {
        Self::before(BackendCall::SetMitigation)?;
        attributes.set_mitigation(words)
    }

    fn set_jobs<Table>(
        attributes: &mut sys::AttributeList,
        jobs: &[ChildHandleValue<Table>],
    ) -> io::Result<()> {
        Self::before(BackendCall::SetJobs)?;
        attributes.set_jobs(jobs)
    }

    fn set_pseudoconsole(
        attributes: &mut sys::AttributeList,
        pseudoconsole: BorrowedPseudoConsole<'_>,
    ) -> io::Result<()> {
        Self::before(BackendCall::SetPseudoConsole)?;
        attributes.set_pseudoconsole(pseudoconsole)
    }

    fn environment_strings() -> io::Result<Vec<(OsString, OsString)>> {
        Self::before(BackendCall::ReadEnvironment)?;
        sys::environment_strings()
    }

    fn system_directory() -> io::Result<OsString> {
        Self::before(BackendCall::QuerySystemDirectory)?;
        sys::system_directory()
    }

    fn windows_directory() -> io::Result<OsString> {
        Self::before(BackendCall::QueryWindowsDirectory)?;
        sys::windows_directory()
    }

    fn create_process<Table>(
        request: &mut sys::ProcessRequest<'_, Table>,
    ) -> io::Result<sys::CreatedProcess> {
        Self::before(BackendCall::CreateProcess)?;
        sys::create_process(request)
    }

    fn reclaim_remote(handle: sys::RemoteHandle<'_>) -> io::Result<()> {
        Self::before(BackendCall::ReclaimRemote)?;
        handle.reclaim()
    }

    fn resume_thread(thread: BorrowedHandle<'_>) -> io::Result<u32> {
        Self::before(BackendCall::ResumeThread)?;
        sys::resume_thread(thread)
    }
}

pub(crate) struct WindowsBackend;

impl SpawnBackend for WindowsBackend {}

#[cfg(test)]
pub(crate) use fault::{
    calls as fault_calls, configure as configure_fault, configure_many as configure_faults,
    FaultBackend,
};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod fault {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::io;

    use super::{BackendCall, SpawnBackend};

    #[derive(Default)]
    struct FaultPlan {
        fail_at: BTreeSet<usize>,
        calls: Vec<BackendCall>,
    }

    std::thread_local! {
        static PLAN: RefCell<FaultPlan> = RefCell::new(FaultPlan::default());
    }

    pub(crate) struct FaultBackend;

    impl SpawnBackend for FaultBackend {
        fn before(call: BackendCall) -> io::Result<()> {
            PLAN.with(|plan| {
                let mut plan = plan.borrow_mut();
                let index = plan.calls.len();
                plan.calls.push(call);
                if plan.fail_at.contains(&index) {
                    Err(io::Error::from_raw_os_error(5))
                } else {
                    Ok(())
                }
            })
        }
    }

    pub(crate) fn configure(fail_at: Option<usize>) {
        let fail_at = fail_at.into_iter().collect();
        configure_set(fail_at);
    }

    pub(crate) fn configure_many(fail_at: &[usize]) {
        configure_set(fail_at.iter().copied().collect());
    }

    fn configure_set(fail_at: BTreeSet<usize>) {
        PLAN.with(|plan| {
            *plan.borrow_mut() = FaultPlan {
                fail_at,
                calls: Vec::new(),
            };
        });
    }

    pub(crate) fn calls() -> Vec<BackendCall> {
        PLAN.with(|plan| plan.borrow().calls.clone())
    }
}
