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

#[derive(Clone, Copy)]
pub(crate) struct AttributeAddress<'a, Value>(&'a Value);

impl<'a, Value> AttributeAddress<'a, Value> {
    pub(crate) const fn new(value: &'a Value) -> Self {
        Self(value)
    }

    const fn get(&self) -> &'a Value {
        self.0
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BackendAdapter {
    Windows,
    #[cfg(test)]
    Fault,
}

pub(crate) trait SpawnBackend: Sized {
    fn adapter() -> BackendAdapter;
}

impl BackendAdapter {
    fn execute<Value, Operation>(self, call: BackendCall, operation: Operation) -> io::Result<Value>
    where
        Operation: FnOnce() -> io::Result<Value>,
    {
        #[cfg(test)]
        if matches!(self, Self::Fault) {
            fault::gate(call)?;
        }
        #[cfg(not(test))]
        {
            let Self::Windows = self;
            let _ = call;
        }
        operation()
    }

    pub(crate) fn standard_handle(self, stream: StandardStream) -> io::Result<Option<OwnedHandle>> {
        self.execute(BackendCall::StandardHandle, || {
            #[cfg(test)]
            if matches!(self, Self::Fault) && fault::missing_standard_handle() {
                return Ok(None);
            }
            sys::standard_handle(stream)
        })
    }

    pub(crate) fn null_handle(self, access: NullAccess) -> io::Result<OwnedHandle> {
        self.execute(BackendCall::OpenNull, || sys::null_handle(access))
    }

    pub(crate) fn create_pipe(self, direction: PipeDirection) -> io::Result<sys::Pipe> {
        self.execute(BackendCall::CreatePipe, || sys::create_pipe(direction))
    }

    pub(crate) fn duplicate_local(
        self,
        source: BorrowedHandle<'_>,
        inheritability: Inheritability,
    ) -> io::Result<OwnedHandle> {
        self.execute(BackendCall::DuplicateLocal, || {
            sys::duplicate_local(source, inheritability)
        })
    }

    pub(crate) fn duplicate_remote<'a>(
        self,
        source: BorrowedHandle<'_>,
        target: BorrowedHandle<'a>,
        inheritability: Inheritability,
    ) -> io::Result<sys::RemoteHandle<'a>> {
        self.execute(BackendCall::DuplicateRemote, || {
            sys::duplicate_remote(source, target, inheritability)
        })
    }

    pub(crate) fn create_job(self) -> io::Result<Job> {
        self.execute(BackendCall::CreateJob, || {
            sys::create_job().map(Job::from_system)
        })
    }

    pub(crate) fn configure_job(self, job: &Job, policy: JobClosePolicy) -> io::Result<()> {
        use std::os::windows::io::AsHandle;

        self.execute(BackendCall::ConfigureJob, || {
            sys::set_job_close_policy(job.as_handle(), policy)
        })
    }

    pub(crate) fn create_attributes(self, count: u32) -> io::Result<sys::AttributeList> {
        self.execute(BackendCall::CreateAttributes, || {
            sys::AttributeList::new(count)
        })
    }

    pub(crate) fn set_handle_list<Table>(
        self,
        attributes: &mut sys::AttributeList,
        handles: &[ChildHandleValue<Table>],
    ) -> io::Result<()> {
        self.execute(BackendCall::SetHandleList, || {
            attributes.set_handle_list(handles)
        })
    }

    pub(crate) fn set_parent<Table>(
        self,
        attributes: &mut sys::AttributeList,
        parent: AttributeAddress<'_, ChildHandleValue<Table>>,
    ) -> io::Result<()> {
        self.execute(BackendCall::SetParent, || {
            attributes.set_parent(parent.get())
        })
    }

    pub(crate) fn set_mitigation(
        self,
        attributes: &mut sys::AttributeList,
        words: &[u64; 2],
    ) -> io::Result<()> {
        self.execute(BackendCall::SetMitigation, || {
            attributes.set_mitigation(words)
        })
    }

    pub(crate) fn set_jobs<Table>(
        self,
        attributes: &mut sys::AttributeList,
        jobs: &[ChildHandleValue<Table>],
    ) -> io::Result<()> {
        self.execute(BackendCall::SetJobs, || attributes.set_jobs(jobs))
    }

    pub(crate) fn set_pseudoconsole(
        self,
        attributes: &mut sys::AttributeList,
        pseudoconsole: BorrowedPseudoConsole<'_>,
    ) -> io::Result<()> {
        self.execute(BackendCall::SetPseudoConsole, || {
            attributes.set_pseudoconsole(pseudoconsole)
        })
    }

    pub(crate) fn environment_strings(self) -> io::Result<Vec<(OsString, OsString)>> {
        self.execute(BackendCall::ReadEnvironment, sys::environment_strings)
    }

    pub(crate) fn system_directory(self) -> io::Result<sys::SystemPath> {
        self.execute(BackendCall::QuerySystemDirectory, sys::system_directory)
    }

    pub(crate) fn windows_directory(self) -> io::Result<sys::SystemPath> {
        self.execute(BackendCall::QueryWindowsDirectory, sys::windows_directory)
    }

    pub(crate) fn create_process<Table>(
        self,
        request: &mut sys::ProcessRequest<'_, Table>,
    ) -> io::Result<sys::CreatedProcess> {
        self.execute(BackendCall::CreateProcess, || sys::create_process(request))
    }

    pub(crate) fn reclaim_remote(
        self,
        handle: sys::RemoteHandle<'_>,
    ) -> io::Result<sys::ReclaimedRemote> {
        self.execute(BackendCall::ReclaimRemote, || handle.reclaim())
    }

    pub(crate) fn resume_thread(
        self,
        thread: BorrowedHandle<'_>,
    ) -> io::Result<sys::PreviousSuspendCount> {
        self.execute(BackendCall::ResumeThread, || sys::resume_thread(thread))
    }
}

pub(crate) struct WindowsBackend;

impl SpawnBackend for WindowsBackend {
    fn adapter() -> BackendAdapter {
        BackendAdapter::Windows
    }
}

#[cfg(test)]
pub(crate) use fault::{
    calls as fault_calls, configure as configure_fault, configure_many as configure_faults,
    configure_missing_standard_handle, FaultBackend,
};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod fault {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::io;

    use super::{BackendAdapter, BackendCall, SpawnBackend};

    #[derive(Default)]
    struct FaultPlan {
        fail_at: BTreeSet<usize>,
        calls: Vec<BackendCall>,
        missing_standard_handle: bool,
    }

    std::thread_local! {
        static PLAN: RefCell<FaultPlan> = RefCell::new(FaultPlan::default());
    }

    pub(crate) struct FaultBackend;

    impl SpawnBackend for FaultBackend {
        fn adapter() -> BackendAdapter {
            BackendAdapter::Fault
        }
    }

    pub(super) fn gate(call: BackendCall) -> io::Result<()> {
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

    pub(crate) fn configure(fail_at: Option<usize>) {
        let fail_at = fail_at.into_iter().collect();
        configure_set(fail_at);
    }

    pub(crate) fn configure_many(fail_at: &[usize]) {
        configure_set(fail_at.iter().copied().collect());
    }

    pub(crate) fn configure_missing_standard_handle() {
        PLAN.with(|plan| {
            *plan.borrow_mut() = FaultPlan {
                fail_at: BTreeSet::new(),
                calls: Vec::new(),
                missing_standard_handle: true,
            };
        });
    }

    fn configure_set(fail_at: BTreeSet<usize>) {
        PLAN.with(|plan| {
            *plan.borrow_mut() = FaultPlan {
                fail_at,
                calls: Vec::new(),
                missing_standard_handle: false,
            };
        });
    }

    pub(super) fn missing_standard_handle() -> bool {
        PLAN.with(|plan| plan.borrow().missing_standard_handle)
    }

    pub(crate) fn calls() -> Vec<BackendCall> {
        PLAN.with(|plan| plan.borrow().calls.clone())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsHandle;

    use super::{
        configure_fault, configure_missing_standard_handle, fault_calls, BackendCall, FaultBackend,
        SpawnBackend, WindowsBackend,
    };
    use crate::handles::{borrowed_pseudoconsole_for_test, Job};
    use crate::sys::{self, AttributeList, StandardStream};
    use crate::JobClosePolicy;

    #[test]
    fn resume_fault_is_observed_before_the_raw_backend() {
        configure_fault(Some(0));
        let file = File::open("NUL").unwrap();
        assert!(FaultBackend::adapter()
            .resume_thread(file.as_handle())
            .is_err());
        assert_eq!(fault_calls(), vec![BackendCall::ResumeThread]);
    }

    #[test]
    fn every_default_adapter_checks_faults_before_touching_windows() -> io::Result<()> {
        configure_fault(Some(0));
        assert!(FaultBackend::adapter()
            .standard_handle(StandardStream::Input)
            .is_err());
        assert_eq!(fault_calls(), vec![BackendCall::StandardHandle]);

        let owner = ();
        let pseudoconsole = borrowed_pseudoconsole_for_test(&owner);
        let mut attributes = AttributeList::new(1)?;
        configure_fault(Some(0));
        assert!(FaultBackend::adapter()
            .set_pseudoconsole(&mut attributes, pseudoconsole)
            .is_err());
        assert_eq!(fault_calls(), vec![BackendCall::SetPseudoConsole]);

        configure_fault(Some(0));
        assert!(FaultBackend::adapter().windows_directory().is_err());
        assert_eq!(fault_calls(), vec![BackendCall::QueryWindowsDirectory]);

        configure_missing_standard_handle();
        assert!(FaultBackend::adapter()
            .standard_handle(StandardStream::Output)?
            .is_none());
        assert!(WindowsBackend::adapter()
            .standard_handle(StandardStream::Output)?
            .is_some());
        Ok(())
    }

    #[test]
    fn windows_backend_configures_the_requested_job_policy() -> crate::Result<()> {
        let job = Job::create()?;
        WindowsBackend::adapter()
            .configure_job(&job, JobClosePolicy::TerminateProcesses)
            .map_err(|error| {
                crate::Error::windows(
                    crate::Phase::Preparation,
                    crate::Operation::ConfigureJob,
                    error,
                )
            })?;
        assert_eq!(
            sys::job_close_policy(job.as_handle()).map_err(|error| crate::Error::windows(
                crate::Phase::Preparation,
                crate::Operation::ConfigureJob,
                error,
            ))?,
            JobClosePolicy::TerminateProcesses
        );
        WindowsBackend::adapter()
            .configure_job(&job, JobClosePolicy::PreserveProcesses)
            .map_err(|error| {
                crate::Error::windows(
                    crate::Phase::Preparation,
                    crate::Operation::ConfigureJob,
                    error,
                )
            })?;
        assert_eq!(
            sys::job_close_policy(job.as_handle()).map_err(|error| crate::Error::windows(
                crate::Phase::Preparation,
                crate::Operation::ConfigureJob,
                error,
            ))?,
            JobClosePolicy::PreserveProcesses
        );
        let file = File::open("NUL").map_err(|error| {
            crate::Error::windows(crate::Phase::Runtime, crate::Operation::OpenFile, error)
        })?;
        assert!(sys::job_close_policy(file.as_handle()).is_err());
        Ok(())
    }
}
