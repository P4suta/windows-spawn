use std::cmp::Ordering;
use std::collections::btree_map::{BTreeMap, Entry};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::pin::Pin;

#[cfg(test)]
use crate::backend::WindowsBackend;
use crate::backend::{AttributeAddress, BackendAdapter, SpawnBackend};
use crate::child::{Child, JobOwnership, ProcessOwner, SuspendedChild};
use crate::command::{Arg, Command, EnvOp, EnvValue, EnvironmentBase};
use crate::core_logic::{CommandLine, CommandLineError};
use crate::error::{CleanupError, Error, Operation, Phase, Result, WindowsError};
use crate::handles::StdioInner;
use crate::options::{ConsoleMode, CreationFlags, JobClosePolicy, SpawnOptions, TerminalMode};
use crate::plan::{
    DesiredState, IoMode, Running, StandardHandles, StandardIo, StdioSpec, Suspended, ValidatedPlan,
};
use crate::resource::{ChildHandleValue, CurrentTable, SelectedTable};
use crate::sys::{self, Inheritability, InitialState, NullAccess, PipeDirection, StandardStream};
use crate::trace::{self, ResourceKind};

pub(crate) fn spawn_running_with_backend<Backend: SpawnBackend>(
    command: &Command,
    options: SpawnOptions<'_>,
    io_mode: IoMode,
) -> Result<Child> {
    spawn_running(command, options, io_mode, Backend::adapter())
}

fn spawn_running(
    command: &Command,
    options: SpawnOptions<'_>,
    io_mode: IoMode,
    backend: BackendAdapter,
) -> Result<Child> {
    ValidatedPlan::running(command, options, io_mode)
        .and_then(|plan| PreparedSpawn::<Running, SelectedTable<'_>>::prepare(plan, backend))
        .and_then(PreparedSpawn::create_suspended)
        .and_then(CreatedSuspended::reclaim)
        .and_then(Reclaimed::resume)
}

pub(crate) fn spawn_suspended_with_backend<Backend: SpawnBackend>(
    command: &Command,
    options: SpawnOptions<'_>,
) -> Result<SuspendedChild> {
    spawn_suspended(command, options, Backend::adapter())
}

fn spawn_suspended(
    command: &Command,
    options: SpawnOptions<'_>,
    backend: BackendAdapter,
) -> Result<SuspendedChild> {
    ValidatedPlan::suspended(command, options, IoMode::Spawn)
        .and_then(|plan| PreparedSpawn::<Suspended, SelectedTable<'_>>::prepare(plan, backend))
        .and_then(PreparedSpawn::create_suspended)
        .and_then(CreatedSuspended::reclaim)
        .map(Reclaimed::into_suspended)
}

pub(crate) fn output_with_backend<Backend: SpawnBackend>(
    command: &Command,
    options: SpawnOptions<'_>,
) -> Result<std::process::Output> {
    output(command, options, Backend::adapter())
}

fn output(
    command: &Command,
    options: SpawnOptions<'_>,
    backend: BackendAdapter,
) -> Result<std::process::Output> {
    ValidatedPlan::running(command, options, IoMode::Output)
        .and_then(|plan| PreparedSpawn::<Running, SelectedTable<'_>>::prepare(plan, backend))
        .and_then(PreparedSpawn::create_suspended)
        .and_then(CreatedSuspended::reclaim)
        .and_then(Reclaimed::resume)
        .and_then(Child::wait_with_output)
}

struct AttributeBacking<'a, Table> {
    inherited_values: Box<[ChildHandleValue<Table>]>,
    parent_value: Option<Box<ChildHandleValue<CurrentTable>>>,
    mitigation_value: Option<Box<[u64; 2]>>,
    job_values: Box<[ChildHandleValue<CurrentTable>]>,
    pseudoconsole: Option<crate::BorrowedPseudoConsole<'a>>,
}

struct ProcessAttributeList<'a, Table> {
    list: Option<sys::AttributeList>,
    _backing: Pin<Box<AttributeBacking<'a, Table>>>,
}

impl<'a, Table> ProcessAttributeList<'a, Table> {
    fn new(backing: AttributeBacking<'a, Table>, backend: BackendAdapter) -> Result<Self> {
        let backing = Box::pin(backing);
        let values = backing.as_ref().get_ref();
        let attribute_count = u32::from(!values.inherited_values.is_empty())
            + u32::from(values.parent_value.is_some())
            + u32::from(values.mitigation_value.is_some())
            + u32::from(!values.job_values.is_empty())
            + u32::from(values.pseudoconsole.is_some());
        let mut list = if attribute_count == 0 {
            None
        } else {
            Some(
                backend
                    .create_attributes(attribute_count)
                    .map_err(attribute_error)?,
            )
        };
        if let Some(attributes) = &mut list {
            if !values.inherited_values.is_empty() {
                backend
                    .set_handle_list(attributes, &values.inherited_values)
                    .map_err(attribute_error)?;
            }
            if let Some(parent) = &values.parent_value {
                backend
                    .set_parent(attributes, AttributeAddress::new(parent.as_ref()))
                    .map_err(attribute_error)?;
            }
            if let Some(mitigation) = &values.mitigation_value {
                backend
                    .set_mitigation(attributes, mitigation)
                    .map_err(attribute_error)?;
            }
            if !values.job_values.is_empty() {
                backend
                    .set_jobs(attributes, &values.job_values)
                    .map_err(attribute_error)?;
            }
            if let Some(pseudoconsole) = values.pseudoconsole {
                backend
                    .set_pseudoconsole(attributes, pseudoconsole)
                    .map_err(attribute_error)?;
            }
        }
        Ok(Self {
            list,
            _backing: backing,
        })
    }

    fn as_list(&self) -> Option<&sys::AttributeList> {
        self.list.as_ref()
    }
}

fn attribute_error(error: io::Error) -> Error {
    Error::windows(Phase::Preparation, Operation::ConfigureAttributes, error)
}

pub(crate) struct PreparedSpawn<'options, State, Table> {
    resources: PreparedResources<'options, Table>,
    state: PhantomData<State>,
}

struct PreparedResources<'options, Table> {
    application: Vec<u16>,
    command_line: Vec<u16>,
    environment: Environment,
    current_dir: Option<Vec<u16>>,
    stdio_values: sys::StartupStdio<Table>,
    stdio: StandardHandles<Option<OwnedHandle>>,
    job: JobOwnership,
    transfer: HandleTransfer<'options>,
    attributes: ProcessAttributeList<'options, Table>,
    creation_flags: CreationFlags,
}

impl<'options, State: DesiredState> PreparedSpawn<'options, State, SelectedTable<'options>> {
    fn prepare(plan: ValidatedPlan<'_, 'options, State>, backend: BackendAdapter) -> Result<Self> {
        let (command, options, stdio_plan, current_dir) = plan.into_parts();
        PreparedResources::<SelectedTable<'options>>::prepare(
            command,
            &options,
            &stdio_plan,
            current_dir,
            backend,
        )
        .map(|resources| Self {
            resources,
            state: PhantomData,
        })
    }

    pub(crate) fn create_suspended(
        self,
    ) -> Result<CreatedSuspended<'options, State, SelectedTable<'options>>> {
        self.resources
            .create_suspended()
            .map(|resources| CreatedSuspended {
                resources,
                state: PhantomData,
            })
    }
}

impl<'options> PreparedResources<'options, SelectedTable<'options>> {
    fn prepare(
        command: &Command,
        options: &SpawnOptions<'options>,
        stdio_plan: &StandardIo<'_>,
        current_dir: Option<Vec<u16>>,
        backend: BackendAdapter,
    ) -> Result<Self> {
        let parent: Option<BorrowedHandle<'options>> = options.parent.map(AsHandle::as_handle);
        let mut transfer = HandleTransfer::new(parent, backend);
        let (stdio_values, stdio) = prepare_standard_io(stdio_plan, &mut transfer)?;
        let (job, job_values) = prepare_job(options, backend)?;

        let command_line = build_command_line(command, &mut transfer)?;
        let environment = build_environment(command, &mut transfer).map_err(|error| {
            Error::windows(Phase::Preparation, Operation::ReadEnvironment, error)
        })?;
        let child_path = environment.path.as_deref();
        let application =
            resolve_executable(&command.program, child_path, backend).map_err(|error| {
                Error::windows(Phase::Preparation, Operation::ResolveExecutable, error)
            })?;
        let attributes = prepare_attributes(options, &transfer, job_values)?;
        Ok(Self {
            application,
            command_line,
            environment,
            current_dir,
            stdio_values,
            stdio,
            job,
            transfer,
            attributes,
            creation_flags: CreationFlags::new(
                options.creation,
                match options.terminal {
                    TerminalMode::Console(mode) => mode,
                    TerminalMode::PseudoConsole(_) => ConsoleMode::Inherit,
                },
            ),
        })
    }

    fn create_suspended(mut self) -> Result<CreatedResources<'options, SelectedTable<'options>>> {
        let mut request = sys::ProcessRequest {
            application: &self.application,
            command_line: &mut self.command_line,
            environment: self.environment.block.as_deref(),
            current_dir: self.current_dir.as_deref(),
            stdio: self.stdio_values,
            inheritability: process_inheritability(self.transfer.inherited_values()),
            creation_flags: self.creation_flags,
            initial_state: InitialState::Suspended,
            attributes: self.attributes.as_list(),
        };
        let created = trace::io(
            Phase::Creation,
            Operation::CreateProcess,
            ResourceKind::Process,
            || self.transfer.backend().create_process(&mut request),
        );
        match created {
            Ok(created) => Ok(CreatedResources {
                process: ProcessOwner::new(created),
                job: self.job,
                stdio: self.stdio,
                transfer: self.transfer,
                attributes: self.attributes,
            }),
            Err(source) => {
                let primary = WindowsError::new(Phase::Creation, Operation::CreateProcess, source);
                drop(self.attributes);
                match self.transfer.reclaim() {
                    Ok(()) => Err(Error::Windows(primary)),
                    Err(mut cleanup) => {
                        cleanup.set_primary(primary);
                        Err(Error::Cleanup(cleanup))
                    }
                }
            }
        }
    }
}

fn process_inheritability(
    inherited_values: &[ChildHandleValue<SelectedTable<'_>>],
) -> Inheritability {
    if inherited_values.is_empty() {
        Inheritability::Private
    } else {
        Inheritability::Inheritable
    }
}

fn prepare_standard_io<'options>(
    plan: &StandardIo<'_>,
    transfer: &mut HandleTransfer<'options>,
) -> Result<(
    sys::StartupStdio<SelectedTable<'options>>,
    StandardHandles<Option<OwnedHandle>>,
)> {
    match plan {
        StandardIo::Ordinary(specs) => {
            let prepared = prepare_standard_handles(specs, transfer).map_err(|error| {
                Error::windows(Phase::Preparation, Operation::DuplicateLocalHandle, error)
            })?;
            let values = sys::StartupStdio::Ordinary(sys::StandardHandles {
                stdin: prepared.stdin.child,
                stdout: prepared.stdout.child,
                stderr: prepared.stderr.child,
            });
            let owners = StandardHandles {
                stdin: prepared.stdin.parent,
                stdout: prepared.stdout.parent,
                stderr: prepared.stderr.parent,
            };
            Ok((values, owners))
        }
        StandardIo::PseudoConsole => Ok((
            sys::StartupStdio::PseudoConsole,
            StandardHandles {
                stdin: None,
                stdout: None,
                stderr: None,
            },
        )),
    }
}

fn prepare_job(
    options: &SpawnOptions<'_>,
    backend: BackendAdapter,
) -> Result<(JobOwnership, Vec<ChildHandleValue<CurrentTable>>)> {
    let job = match options.job_close {
        JobClosePolicy::PreserveProcesses => JobOwnership::Preserve,
        JobClosePolicy::TerminateProcesses => {
            let job = trace::io(
                Phase::Preparation,
                Operation::CreateJob,
                ResourceKind::Job,
                || backend.create_job(),
            )
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreateJob, error))?;
            trace::io(
                Phase::Preparation,
                Operation::ConfigureJob,
                ResourceKind::Job,
                || backend.configure_job(&job, JobClosePolicy::TerminateProcesses),
            )
            .map_err(|error| Error::windows(Phase::Preparation, Operation::ConfigureJob, error))?;
            JobOwnership::Terminate(job)
        }
    };
    let mut values: Vec<ChildHandleValue<CurrentTable>> = options
        .jobs
        .iter()
        .map(|job| sys::child_handle_value(job.as_handle()))
        .collect();
    if let JobOwnership::Terminate(job) = &job {
        values.push(sys::child_handle_value(job.as_handle()));
    }
    Ok((job, values))
}

fn prepare_attributes<'options>(
    options: &SpawnOptions<'options>,
    transfer: &HandleTransfer<'options>,
    job_values: Vec<ChildHandleValue<CurrentTable>>,
) -> Result<ProcessAttributeList<'options, SelectedTable<'options>>> {
    let inherited_values = transfer.inherited_values().to_vec().into_boxed_slice();
    let parent_value = transfer
        .parent()
        .map(|handle| Box::new(sys::child_handle_value(handle)));
    let mitigation_words = options.mitigation.words();
    let mitigation_value = (mitigation_words != [0, 0]).then(|| Box::new(mitigation_words));
    let pseudoconsole = match options.terminal {
        TerminalMode::Console(_) => None,
        TerminalMode::PseudoConsole(value) => Some(value),
    };
    ProcessAttributeList::new(
        AttributeBacking {
            inherited_values,
            parent_value,
            mitigation_value,
            job_values: job_values.into_boxed_slice(),
            pseudoconsole,
        },
        transfer.backend(),
    )
}

pub(crate) struct CreatedSuspended<'options, State, Table> {
    resources: CreatedResources<'options, Table>,
    state: PhantomData<State>,
}

struct CreatedResources<'options, Table> {
    process: ProcessOwner,
    job: JobOwnership,
    stdio: StandardHandles<Option<OwnedHandle>>,
    transfer: HandleTransfer<'options>,
    attributes: ProcessAttributeList<'options, Table>,
}

impl<State, Table> CreatedSuspended<'_, State, Table> {
    pub(crate) fn reclaim(self) -> Result<Reclaimed<State, Table>> {
        self.resources.reclaim().map(|resources| Reclaimed {
            resources,
            state: PhantomData,
        })
    }
}

impl<Table> CreatedResources<'_, Table> {
    fn reclaim(self) -> Result<ReclaimedResources> {
        let Self {
            process,
            job,
            stdio,
            transfer,
            attributes,
        } = self;
        let backend = transfer.backend();
        drop(attributes);
        transfer
            .reclaim()
            .map_err(Error::Cleanup)
            .map(|()| ReclaimedResources {
                process,
                job,
                stdio,
                backend,
            })
    }
}

pub(crate) struct Reclaimed<State, Table> {
    resources: ReclaimedResources,
    state: PhantomData<(State, Table)>,
}

struct ReclaimedResources {
    process: ProcessOwner,
    job: JobOwnership,
    stdio: StandardHandles<Option<OwnedHandle>>,
    backend: BackendAdapter,
}

impl ReclaimedResources {
    fn into_child(self) -> Child {
        Child::new(
            self.process,
            self.job,
            self.stdio.stdin,
            self.stdio.stdout,
            self.stdio.stderr,
        )
    }
}

impl<Table> Reclaimed<Running, Table> {
    pub(crate) fn resume(self) -> Result<Child> {
        let backend = self.resources.backend;
        let mut child = self.resources.into_child();
        let resumed = match backend {
            BackendAdapter::Windows => {
                child.resume_initial_with(|thread| BackendAdapter::Windows.resume_thread(thread))
            }
            #[cfg(test)]
            BackendAdapter::Fault => {
                child.resume_initial_with(|thread| BackendAdapter::Fault.resume_thread(thread))
            }
        };
        resumed.map(|_| child)
    }
}

impl<Table> Reclaimed<Suspended, Table> {
    pub(crate) fn into_suspended(self) -> SuspendedChild {
        SuspendedChild::new(self.resources.into_child())
    }
}

struct PreparedStdio<'parent> {
    child: ChildHandleValue<SelectedTable<'parent>>,
    parent: Option<OwnedHandle>,
}

fn prepare_standard_handles<'parent>(
    specs: &StandardHandles<StdioSpec<'_>>,
    transfer: &mut HandleTransfer<'parent>,
) -> io::Result<StandardHandles<PreparedStdio<'parent>>> {
    Ok(StandardHandles {
        stdin: prepare_stdio(specs.stdin, StandardStream::Input, transfer)?,
        stdout: prepare_stdio(specs.stdout, StandardStream::Output, transfer)?,
        stderr: prepare_stdio(specs.stderr, StandardStream::Error, transfer)?,
    })
}

fn prepare_stdio<'parent>(
    spec: StdioSpec<'_>,
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent>,
) -> io::Result<PreparedStdio<'parent>> {
    match spec {
        StdioSpec::Inherit => prepare_inherit(stream, transfer),
        StdioSpec::Null => prepare_null(stream, transfer),
        StdioSpec::Piped => prepare_pipe(stream, transfer),
        StdioSpec::Configured(stdio) => match &stdio.inner {
            StdioInner::Inherit => prepare_inherit(stream, transfer),
            StdioInner::Null => prepare_null(stream, transfer),
            StdioInner::Piped => prepare_pipe(stream, transfer),
            StdioInner::Owned(handle) => Ok(PreparedStdio {
                child: transfer.lower(handle.as_handle())?,
                parent: None,
            }),
        },
    }
}

fn prepare_inherit<'parent>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent>,
) -> io::Result<PreparedStdio<'parent>> {
    match transfer.backend().standard_handle(stream)? {
        Some(handle) => Ok(PreparedStdio {
            child: transfer.lower(handle.as_handle())?,
            parent: None,
        }),
        None => Ok(PreparedStdio {
            child: ChildHandleValue::INVALID,
            parent: None,
        }),
    }
}

fn prepare_null<'parent>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent>,
) -> io::Result<PreparedStdio<'parent>> {
    let access = match stream {
        StandardStream::Input => NullAccess::Read,
        StandardStream::Output | StandardStream::Error => NullAccess::Write,
    };
    let handle = transfer.backend().null_handle(access)?;
    Ok(PreparedStdio {
        child: transfer.lower(handle.as_handle())?,
        parent: None,
    })
}

fn prepare_pipe<'parent>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent>,
) -> io::Result<PreparedStdio<'parent>> {
    let direction = match stream {
        StandardStream::Input => PipeDirection::ParentWrites,
        StandardStream::Output | StandardStream::Error => PipeDirection::ParentReads,
    };
    let pipe = trace::io(
        Phase::Preparation,
        Operation::CreatePipe,
        ResourceKind::Pipe,
        || transfer.backend().create_pipe(direction),
    )?;
    let child = transfer.lower(pipe.child.as_handle())?;
    Ok(PreparedStdio {
        child,
        parent: Some(pipe.parent),
    })
}

struct HandleTransfer<'a> {
    parent: Option<BorrowedHandle<'a>>,
    local: Vec<OwnedHandle>,
    remote: Vec<sys::RemoteHandle<'a>>,
    inherited: Vec<ChildHandleValue<SelectedTable<'a>>>,
    backend: BackendAdapter,
}

impl<'a> HandleTransfer<'a> {
    fn new(parent: Option<BorrowedHandle<'a>>, backend: BackendAdapter) -> Self {
        Self {
            parent,
            local: Vec::new(),
            remote: Vec::new(),
            inherited: Vec::new(),
            backend,
        }
    }

    fn lower(
        &mut self,
        source: BorrowedHandle<'_>,
    ) -> io::Result<ChildHandleValue<SelectedTable<'a>>> {
        let value = if let Some(parent) = self.parent {
            let handle = trace::io(
                Phase::Preparation,
                Operation::DuplicateRemoteHandle,
                ResourceKind::RemoteHandle,
                || {
                    self.backend
                        .duplicate_remote(source, parent, Inheritability::Inheritable)
                },
            )?;
            let value = handle.value();
            self.remote.push(handle);
            value
        } else {
            let handle = trace::io(
                Phase::Preparation,
                Operation::DuplicateLocalHandle,
                ResourceKind::Handle,
                || {
                    self.backend
                        .duplicate_local(source, Inheritability::Inheritable)
                },
            )?;
            let value = sys::child_handle_value(handle.as_handle());
            self.local.push(handle);
            value
        };
        self.inherited.push(value);
        Ok(value)
    }

    fn parent(&self) -> Option<BorrowedHandle<'a>> {
        self.parent
    }

    fn backend(&self) -> BackendAdapter {
        self.backend
    }

    fn duplicate_operation(&self) -> Operation {
        match self.parent {
            Some(_) => Operation::DuplicateRemoteHandle,
            None => Operation::DuplicateLocalHandle,
        }
    }

    fn inherited_values(&self) -> &[ChildHandleValue<SelectedTable<'a>>] {
        &self.inherited
    }

    fn reclaim(self) -> std::result::Result<(), CleanupError> {
        let mut failures = Vec::new();
        for handle in self.remote {
            if let Err(error) = trace::io(
                Phase::Reclamation,
                Operation::ReclaimRemoteHandle,
                ResourceKind::RemoteHandle,
                || self.backend.reclaim_remote(handle),
            ) {
                failures.push(WindowsError::new(
                    Phase::Reclamation,
                    Operation::ReclaimRemoteHandle,
                    error,
                ));
            }
        }
        let mut failures = failures.into_iter();
        let Some(first) = failures.next() else {
            return Ok(());
        };
        let mut cleanup = CleanupError::new(first);
        for failure in failures {
            cleanup.push(failure);
        }
        Err(cleanup)
    }
}

struct Environment {
    block: Option<Vec<u16>>,
    path: Option<OsString>,
}

#[derive(Debug)]
struct EnvKey {
    text: OsString,
    wide: Vec<u16>,
}

impl EnvKey {
    fn new(text: OsString) -> Self {
        let wide = text.encode_wide().collect();
        Self { text, wide }
    }
}

impl Ord for EnvKey {
    fn cmp(&self, other: &Self) -> Ordering {
        sys::compare_ordinal(&self.wide, &other.wide)
    }
}

impl PartialOrd for EnvKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for EnvKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for EnvKey {}

fn build_environment(
    command: &Command,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<Environment> {
    if command.environment_base == EnvironmentBase::Inherit && command.env_ops.is_empty() {
        return Ok(Environment {
            block: None,
            path: None,
        });
    }

    let mut map: BTreeMap<EnvKey, OsString> = BTreeMap::new();
    if command.environment_base == EnvironmentBase::Inherit {
        for (key, value) in transfer.backend().environment_strings()? {
            map.insert(EnvKey::new(key), value);
        }
    }
    for operation in &command.env_ops {
        match operation {
            EnvOp::Set(key, value) => {
                let value = match value {
                    EnvValue::Text(value) => value.clone(),
                    EnvValue::Handle(handle) => {
                        OsString::from(transfer.lower(handle.as_handle())?.to_string())
                    }
                };
                match map.entry(EnvKey::new(key.clone())) {
                    Entry::Occupied(mut entry) => {
                        entry.insert(value);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            }
            EnvOp::Remove(key) => {
                map.remove(&EnvKey::new(key.clone()));
            }
        }
    }

    let path = map.get(&EnvKey::new(OsString::from("PATH"))).cloned();
    let mut block = Vec::new();
    for (key, value) in map {
        block.extend(key.text.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(Environment {
        block: Some(block),
        path,
    })
}

fn build_command_line(command: &Command, transfer: &mut HandleTransfer<'_>) -> Result<Vec<u16>> {
    let program: Vec<u16> = command.program.encode_wide().collect();
    let mut result = CommandLine::new(&program).map_err(command_line_error)?;
    for argument in &command.args {
        match argument {
            Arg::Text(text) => {
                let units: Vec<u16> = text.encode_wide().collect();
                result.push_regular(&units).map_err(command_line_error)?;
            }
            Arg::Raw(text) => {
                let units: Vec<u16> = text.encode_wide().collect();
                result.push_raw(&units).map_err(command_line_error)?;
            }
            Arg::Handle(handle) => {
                let operation = transfer.duplicate_operation();
                let value = transfer
                    .lower(handle.as_handle())
                    .map_err(|error| Error::windows(Phase::Preparation, operation, error))?;
                let units: Vec<u16> = value.to_string().encode_utf16().collect();
                result.push_regular(&units).map_err(command_line_error)?;
            }
        }
    }
    Ok(result.finish())
}

fn command_line_error(error: CommandLineError) -> Error {
    match error {
        CommandLineError::TooLong => Error::Validation(crate::ValidationError::CommandLineTooLong),
        CommandLineError::Allocation(source) => Error::windows(
            Phase::Preparation,
            Operation::BuildCommandLine,
            io::Error::other(source),
        ),
    }
}

fn resolve_executable(
    program: &OsStr,
    child_path: Option<&OsStr>,
    backend: BackendAdapter,
) -> io::Result<Vec<u16>> {
    resolve_executable_with(
        program,
        child_path,
        env::current_exe(),
        env::var_os("PATH").as_deref(),
        backend,
    )
}

fn resolve_executable_with(
    program: &OsStr,
    child_path: Option<&OsStr>,
    current_application: io::Result<PathBuf>,
    process_path: Option<&OsStr>,
    backend: BackendAdapter,
) -> io::Result<Vec<u16>> {
    let path = Path::new(program);
    let has_exe_suffix = program
        .as_encoded_bytes()
        .get(program.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b".exe"));
    let is_file_name = path.file_name() == Some(program);

    if !is_file_name {
        if has_exe_suffix {
            return wide_nul(program);
        }
        let mut appended = program.to_os_string();
        appended.push(".exe");
        let appended_wide = wide_nul(&appended)?;
        if sys::program_exists(&appended_wide) {
            return Ok(appended_wide);
        }
        return wide_nul(program);
    }

    let has_extension = program.as_encoded_bytes().contains(&b'.');
    let search = |mut directory: PathBuf| -> Option<Vec<u16>> {
        directory.push(program);
        if !has_extension {
            directory.set_extension("exe");
        }
        let wide = wide_nul(directory.as_os_str()).ok()?;
        sys::program_exists(&wide).then_some(wide)
    };

    if let Some(paths) = child_path {
        for directory in env::split_paths(paths).filter(|path| !path.as_os_str().is_empty()) {
            if let Some(found) = search(directory) {
                return Ok(found);
            }
        }
    }
    if let Ok(mut application) = current_application {
        application.pop();
        if let Some(found) = search(application) {
            return Ok(found);
        }
    }
    if let Some(found) = search(backend.system_directory()?.into_path_buf()) {
        return Ok(found);
    }
    if let Some(found) = search(backend.windows_directory()?.into_path_buf()) {
        return Ok(found);
    }
    if let Some(paths) = process_path {
        for directory in env::split_paths(paths).filter(|path| !path.as_os_str().is_empty()) {
            if let Some(found) = search(directory) {
                return Ok(found);
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "program not found"))
}

fn wide_nul(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "string contains an interior NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::fs::File;
    use std::os::windows::ffi::OsStringExt;

    use super::*;
    use crate::backend::{
        configure_fault, configure_faults, configure_missing_standard_handle, fault_calls,
        BackendCall, FaultBackend,
    };
    use crate::{ConsoleMode, DepPolicy, MitigationPolicy, ParentProcess, Stdio, TerminalMode};
    use windows_sys::Win32::System::Threading::{CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP};

    fn windows_backend() -> BackendAdapter {
        WindowsBackend::adapter()
    }

    fn fault_backend() -> BackendAdapter {
        FaultBackend::adapter()
    }

    struct ProbeFile {
        path: PathBuf,
    }

    impl ProbeFile {
        fn create(path: PathBuf) -> Self {
            drop(File::create(&path).unwrap());
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for ProbeFile {
        fn drop(&mut self) {
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => eprintln!("test probe cleanup failed: {error}"),
            }
        }
    }

    fn attribute_backing(
        mitigation_value: Option<Box<[u64; 2]>>,
    ) -> AttributeBacking<'static, SelectedTable<'static>> {
        AttributeBacking {
            inherited_values: Vec::new().into_boxed_slice(),
            parent_value: None,
            mitigation_value,
            job_values: Vec::new().into_boxed_slice(),
            pseudoconsole: None,
        }
    }

    #[test]
    fn attribute_presence_and_disjoint_creation_flags_are_observable() -> Result<()> {
        let empty = ProcessAttributeList::<SelectedTable<'static>>::new(
            attribute_backing(None),
            windows_backend(),
        )?;
        assert!(empty.as_list().is_none());
        let populated = ProcessAttributeList::<SelectedTable<'static>>::new(
            attribute_backing(Some(Box::new([1, 0]))),
            windows_backend(),
        )?;
        assert!(populated.as_list().is_some());

        configure_fault(None);
        let command = Command::new("cmd.exe");
        let options = SpawnOptions::new()
            .terminal(TerminalMode::Console(ConsoleMode::NewConsole))
            .new_process_group();
        let plan = ValidatedPlan::running(&command, options, IoMode::Spawn)?;
        let prepared = PreparedSpawn::<Running, SelectedTable<'_>>::prepare(plan, fault_backend())?;
        assert_eq!(
            prepared.resources.creation_flags.bits(),
            CREATE_NEW_PROCESS_GROUP | CREATE_NEW_CONSOLE
        );

        let job = crate::Job::create()?;
        let job_options = SpawnOptions::new().job(&job);
        let (_, job_values) = prepare_job(&job_options, windows_backend())?;
        assert_eq!(job_values.len(), 1);
        Ok(())
    }

    #[test]
    fn pseudoconsole_preparation_uses_the_terminal_only_path() -> Result<()> {
        configure_fault(None);
        let owner = ();
        let terminal =
            TerminalMode::PseudoConsole(crate::handles::borrowed_pseudoconsole_for_test(&owner));
        let command = Command::new("cmd.exe");
        let plan = ValidatedPlan::running(
            &command,
            SpawnOptions::new().terminal(terminal),
            IoMode::Spawn,
        )?;
        let prepared = PreparedSpawn::<Running, SelectedTable<'_>>::prepare(plan, fault_backend())?;
        assert!(matches!(
            prepared.resources.stdio_values,
            sys::StartupStdio::PseudoConsole
        ));
        assert!(prepared.resources.stdio.stdin.is_none());
        assert!(prepared.resources.stdio.stdout.is_none());
        assert!(prepared.resources.stdio.stderr.is_none());
        assert!(matches!(
            process_inheritability(&[]),
            Inheritability::Private
        ));
        let inherited = [ChildHandleValue::<SelectedTable<'static>>::INVALID];
        assert!(matches!(
            process_inheritability(&inherited),
            Inheritability::Inheritable
        ));
        Ok(())
    }

    #[test]
    fn prepared_stdio_and_attributes_propagate_each_backend_failure() -> io::Result<()> {
        let inherited = Stdio::inherit();
        configure_missing_standard_handle();
        let mut missing_transfer = HandleTransfer::new(None, fault_backend());
        let prepared = prepare_stdio(
            StdioSpec::Configured(&inherited),
            StandardStream::Input,
            &mut missing_transfer,
        )?;
        assert_eq!(prepared.child.as_raw(), -1);
        assert!(prepared.parent.is_none());

        configure_fault(None);
        let piped = Stdio::piped();
        let mut piped_transfer = HandleTransfer::new(None, fault_backend());
        let piped = prepare_stdio(
            StdioSpec::Configured(&piped),
            StandardStream::Input,
            &mut piped_transfer,
        )?;
        assert!(piped.parent.is_some());

        configure_fault(Some(0));
        let mut fault_transfer = HandleTransfer::new(None, fault_backend());
        assert!(prepare_stdio(
            StdioSpec::Inherit,
            StandardStream::Input,
            &mut fault_transfer,
        )
        .is_err());
        let owned = Stdio::from(
            File::open("NUL")
                .map_err(|error| Error::windows(Phase::Preparation, Operation::OpenFile, error))?,
        );
        configure_fault(Some(0));
        let mut failed_transfer = HandleTransfer::new(None, fault_backend());
        assert!(prepare_stdio(
            StdioSpec::Configured(&owned),
            StandardStream::Input,
            &mut failed_transfer,
        )
        .is_err());
        configure_fault(None);
        let mut owned_transfer = HandleTransfer::new(None, fault_backend());
        let prepared_owned = prepare_stdio(
            StdioSpec::Configured(&owned),
            StandardStream::Input,
            &mut owned_transfer,
        )?;
        assert!(prepared_owned.parent.is_none());

        configure_fault(Some(1));
        let pseudo_owner = ();
        let pseudoconsole = crate::handles::borrowed_pseudoconsole_for_test(&pseudo_owner);
        let backing = AttributeBacking {
            inherited_values: Vec::new().into_boxed_slice(),
            parent_value: None,
            mitigation_value: None,
            job_values: Vec::new().into_boxed_slice(),
            pseudoconsole: Some(pseudoconsole),
        };
        assert!(
            ProcessAttributeList::<SelectedTable<'static>>::new(backing, fault_backend()).is_err()
        );
        Ok(())
    }

    #[test]
    fn output_pipeline_runs_through_the_backend_state_machine() -> Result<()> {
        configure_fault(None);
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let output = output_with_backend::<FaultBackend>(&command, SpawnOptions::new())?;
        assert!(output.status.success());
        Ok(())
    }

    #[test]
    fn dynamic_builders_preserve_all_typed_failure_categories() -> Result<()> {
        let allocation = Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err();
        assert!(matches!(
            command_line_error(CommandLineError::TooLong),
            Error::Validation(crate::ValidationError::CommandLineTooLong)
        ));
        let allocation_error = command_line_error(CommandLineError::Allocation(allocation));
        let Error::Windows(windows) = allocation_error else {
            return Err(Error::Validation(crate::ValidationError::SizeOverflow));
        };
        assert_eq!(windows.operation(), Operation::BuildCommandLine);

        let source = File::open("NUL")
            .map_err(|error| Error::windows(Phase::Preparation, Operation::OpenFile, error))?;
        let mut argument = Command::new("program.exe");
        argument.arg_handle(&source)?;
        configure_fault(Some(0));
        let mut transfer = HandleTransfer::new(None, fault_backend());
        assert!(build_command_line(&argument, &mut transfer).is_err());

        let mut environment = Command::new("program.exe");
        environment.env_clear();
        environment.env_handle("HANDLE", &source)?;
        configure_fault(Some(0));
        let mut transfer = HandleTransfer::new(None, fault_backend());
        assert!(build_environment(&environment, &mut transfer).is_err());
        configure_fault(None);
        let mut transfer = HandleTransfer::new(None, fault_backend());
        let lowered_environment =
            build_environment(&environment, &mut transfer).map_err(|error| {
                Error::windows(Phase::Preparation, Operation::ReadEnvironment, error)
            })?;
        assert!(lowered_environment.block.is_some());

        let oversized = vec![u16::from(b'x'); crate::core_logic::MAX_COMMAND_LINE_UNITS];
        let oversized_text = OsString::from_wide(&oversized);
        let oversized_program = Command::new(&oversized_text);
        let mut transfer = HandleTransfer::new(None, windows_backend());
        assert!(build_command_line(&oversized_program, &mut transfer).is_err());

        let mut regular = Command::new("program.exe");
        regular.arg(&oversized_text);
        let mut transfer = HandleTransfer::new(None, windows_backend());
        assert!(build_command_line(&regular, &mut transfer).is_err());

        let mut raw = Command::new("program.exe");
        raw.raw_arg(&oversized_text);
        let mut transfer = HandleTransfer::new(None, windows_backend());
        assert!(build_command_line(&raw, &mut transfer).is_err());

        let largest_program = OsString::from_wide(&vec![
            u16::from(b'p');
            crate::core_logic::MAX_COMMAND_LINE_UNITS
                - 3
        ]);
        let mut handle_at_limit = Command::new(largest_program);
        handle_at_limit.arg_handle(&source)?;
        let mut transfer = HandleTransfer::new(None, windows_backend());
        assert!(build_command_line(&handle_at_limit, &mut transfer).is_err());
        Ok(())
    }

    fn decode(value: &[u16]) -> String {
        String::from_utf16_lossy(value.strip_suffix(&[0]).unwrap_or(value))
    }

    fn run_local_fault_case(fail_at: Option<usize>) -> Result<()> {
        configure_fault(fail_at);
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command.env("WINDOWS_SPAWN_FAULT_BACKEND", "1");
        let options = SpawnOptions::new()
            .mitigation(MitigationPolicy::new().dep(DepPolicy::Enable))
            .job_close_policy(JobClosePolicy::TerminateProcesses);
        let mut child =
            spawn_running_with_backend::<FaultBackend>(&command, options, IoMode::Output)?;
        wait_for_fault_child(&mut child)?;
        child.cleanup().map(drop)
    }

    fn run_suspended_fault_case(fail_at: Option<usize>) -> Result<()> {
        configure_fault(fail_at);
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let suspended =
            spawn_suspended_with_backend::<FaultBackend>(&command, SpawnOptions::new())?;
        drop(suspended);
        Ok(())
    }

    fn run_remote_fault_case(fail_at: Option<usize>) -> Result<()> {
        configure_fault(fail_at);
        execute_remote_fault_case()
    }

    fn execute_remote_fault_case() -> Result<()> {
        let parent = ParentProcess::open(std::process::id())?;
        let source = File::open("NUL")
            .map_err(|error| Error::windows(Phase::Preparation, Operation::OpenFile, error))?;
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        command.arg_handle(&source)?;
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let options = SpawnOptions::new().parent_process(&parent);
        let mut child =
            spawn_running_with_backend::<FaultBackend>(&command, options, IoMode::Spawn)?;
        wait_for_fault_child(&mut child)?;
        child.cleanup().map(drop)
    }

    fn wait_for_fault_child(child: &mut Child) -> Result<()> {
        let exited = sys::wait_process_for_test(child.process_handle(), 5_000)
            .map_err(|error| Error::windows(Phase::Runtime, Operation::WaitProcess, error))?;
        if !exited {
            sys::cleanup_process_for_test(child.process_handle());
            return Err(Error::windows(
                Phase::Runtime,
                Operation::WaitProcess,
                io::Error::new(io::ErrorKind::TimedOut, "fault-backend child did not exit"),
            ));
        }
        let _status = child.wait()?;
        Ok(())
    }

    fn assert_faults_release_every_handle(
        run_case: fn(Option<usize>) -> Result<()>,
    ) -> io::Result<Vec<BackendCall>> {
        run_case(None).map_err(io::Error::from)?;
        let baseline = fault_calls();
        assert!(!baseline.is_empty());
        for index in 0..baseline.len() {
            let before = sys::current_process_handle_count()?;
            let result = run_case(Some(index));
            assert!(result.is_err(), "fault {index} was not observed");
            let observed = fault_calls();
            assert_eq!(observed.get(index), baseline.get(index), "fault {index}");
            assert_eq!(&observed[..=index], &baseline[..=index], "fault {index}");
            let after = sys::current_process_handle_count()?;
            assert_eq!(after, before, "fault {index} leaked a handle");
        }
        Ok(baseline)
    }

    struct ProcessExitGuard {
        process: OwnedHandle,
        armed: bool,
    }

    impl ProcessExitGuard {
        fn new(process: OwnedHandle) -> Self {
            Self {
                process,
                armed: true,
            }
        }

        #[track_caller]
        fn assert_exited(&mut self, message: &str) {
            match sys::wait_process_for_test(self.process.as_handle(), 5_000) {
                Ok(true) => self.armed = false,
                Ok(false) => panic!("{message}"),
                Err(error) => panic!("{message}: {error}"),
            }
        }
    }

    impl Drop for ProcessExitGuard {
        fn drop(&mut self) {
            if self.armed {
                sys::cleanup_process_for_test(self.process.as_handle());
            }
        }
    }

    #[test]
    fn fault_backend_exhausts_local_spawn_calls_without_leaks() -> io::Result<()> {
        let calls = assert_faults_release_every_handle(run_local_fault_case)?;
        for required in [
            BackendCall::OpenNull,
            BackendCall::CreatePipe,
            BackendCall::DuplicateLocal,
            BackendCall::CreateJob,
            BackendCall::ConfigureJob,
            BackendCall::ReadEnvironment,
            BackendCall::SetHandleList,
            BackendCall::SetMitigation,
            BackendCall::SetJobs,
            BackendCall::CreateProcess,
            BackendCall::ResumeThread,
        ] {
            assert!(calls.contains(&required), "missing {required:?}");
        }
        Ok(())
    }

    #[test]
    fn fault_backend_exhausts_remote_spawn_calls_without_leaks() -> io::Result<()> {
        let calls = assert_faults_release_every_handle(run_remote_fault_case)?;
        for required in [
            BackendCall::DuplicateRemote,
            BackendCall::SetParent,
            BackendCall::ReclaimRemote,
        ] {
            assert!(calls.contains(&required), "missing {required:?}");
        }
        Ok(())
    }

    #[test]
    fn fault_backend_exhausts_suspended_spawn_calls_without_leaks() -> io::Result<()> {
        let calls = assert_faults_release_every_handle(run_suspended_fault_case)?;
        assert!(calls.contains(&BackendCall::StandardHandle));
        let invalid = Command::new("");
        assert!(
            spawn_suspended_with_backend::<FaultBackend>(&invalid, SpawnOptions::new(),).is_err()
        );
        assert!(spawn_running_with_backend::<FaultBackend>(
            &invalid,
            SpawnOptions::new(),
            IoMode::Spawn,
        )
        .is_err());
        assert!(output_with_backend::<FaultBackend>(&invalid, SpawnOptions::new()).is_err());
        Ok(())
    }

    #[test]
    fn primary_and_cleanup_failures_are_both_preserved() -> io::Result<()> {
        run_remote_fault_case(None).map_err(io::Error::from)?;
        let calls = fault_calls();
        let create = calls
            .iter()
            .position(|call| *call == BackendCall::CreateProcess)
            .ok_or_else(|| io::Error::other("create call was not observed"))?;
        let reclaim: Vec<usize> = calls
            .iter()
            .enumerate()
            .filter_map(|(index, call)| (*call == BackendCall::ReclaimRemote).then_some(index))
            .collect();
        let mut failures = Vec::with_capacity(reclaim.len() + 1);
        failures.push(create);
        failures.extend(reclaim.iter().copied());
        configure_faults(&failures);
        let before = sys::current_process_handle_count()?;
        let result = execute_remote_fault_case();
        let after = sys::current_process_handle_count()?;
        assert_eq!(after, before);
        let Error::Cleanup(cleanup) = result.expect_err("combined failure expected") else {
            return Err(io::Error::other("combined failure lost its cleanup type"));
        };
        let primary = cleanup
            .primary()
            .ok_or_else(|| io::Error::other("primary failure was not retained"))?;
        assert_eq!(primary.operation(), Operation::CreateProcess);
        assert_eq!(cleanup.failures().count(), reclaim.len());
        Ok(())
    }

    #[test]
    fn quotes_regular_and_preserves_raw_arguments() {
        let mut command = Command::new("program.exe");
        command.arg("a b").arg("a\"b").raw_arg("x&&y");
        let mut transfer = HandleTransfer::new(None, windows_backend());
        let line = build_command_line(&command, &mut transfer).unwrap();
        assert_eq!(decode(&line), r#""program.exe" "a b" "a\"b" x&&y"#);
    }

    #[test]
    fn executable_search_finds_system_command_without_current_directory() {
        let command = resolve_executable(OsStr::new("cmd"), None, windows_backend()).unwrap();
        assert!(decode(&command).to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn cleared_environment_is_double_nul() {
        let mut command = Command::new("cmd.exe");
        command.env_clear();
        let mut transfer = HandleTransfer::new(None, windows_backend());
        let environment = build_environment(&command, &mut transfer).unwrap();
        assert_eq!(environment.block.unwrap(), vec![0, 0]);
    }

    #[test]
    fn environment_merges_case_insensitively_and_removes_entries() {
        let mut command = Command::new("cmd.exe");
        command
            .env("Path", "first")
            .env("PATH", "second")
            .env("REMOVE_ME", "value")
            .env_remove("remove_me");
        let mut transfer = HandleTransfer::new(None, windows_backend());
        let environment = build_environment(&command, &mut transfer).unwrap();
        assert_eq!(environment.path, Some(OsString::from("second")));
        let block = environment.block.unwrap();
        let text = String::from_utf16_lossy(&block);
        assert!(!text.contains("first"));
        assert!(!text.contains("REMOVE_ME"));

        let lower = EnvKey::new(OsString::from("alpha"));
        let upper = EnvKey::new(OsString::from("ALPHA"));
        assert_eq!(lower, upper);
        assert_eq!(lower.partial_cmp(&upper), Some(Ordering::Equal));
        assert_ne!(lower, EnvKey::new(OsString::from("beta")));
    }

    #[test]
    fn environment_preserves_windows_ordinal_distinctions() {
        let mut command = Command::new("cmd.exe");
        command
            .env_clear()
            .env("S", "latin-s")
            .env("ſ", "long-s")
            .env("Μ", "greek-mu")
            .env("µ", "micro-sign");
        let mut transfer = HandleTransfer::new(None, windows_backend());
        let block = build_environment(&command, &mut transfer)
            .unwrap()
            .block
            .unwrap();
        let entries: Vec<String> = block
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .map(String::from_utf16_lossy)
            .collect();

        assert_eq!(entries.len(), 4, "Windows-distinct keys were overwritten");
        for expected in ["S=latin-s", "ſ=long-s", "Μ=greek-mu", "µ=micro-sign"] {
            assert!(entries.iter().any(|entry| entry == expected));
        }
        assert_ne!(
            EnvKey::new(OsString::from("S")),
            EnvKey::new(OsString::from("ſ"))
        );
        assert_ne!(
            EnvKey::new(OsString::from("Μ")),
            EnvKey::new(OsString::from("µ"))
        );
    }

    #[test]
    fn quoting_covers_empty_and_trailing_backslashes() {
        let mut command = Command::new("program.exe");
        command.arg("").arg(r"C:\path with spaces\");
        let mut transfer = HandleTransfer::new(None, windows_backend());
        let line = decode(&build_command_line(&command, &mut transfer).unwrap());
        assert_eq!(line, r#""program.exe" "" "C:\path with spaces\\""#);
    }

    #[test]
    fn executable_resolution_covers_explicit_paths_and_not_found() {
        let system = sys::system_directory().unwrap().into_path_buf();
        let executable = system.join("cmd.exe");
        assert_eq!(
            decode(&resolve_executable(executable.as_os_str(), None, windows_backend()).unwrap(),),
            executable.to_string_lossy()
        );
        let without_extension = system.join("cmd");
        assert_eq!(
            decode(
                &resolve_executable(without_extension.as_os_str(), None, windows_backend())
                    .unwrap(),
            ),
            executable.to_string_lossy()
        );
        assert!(decode(
            &resolve_executable(
                OsStr::new("cmd.exe"),
                Some(system.as_os_str()),
                windows_backend(),
            )
            .unwrap()
        )
        .to_ascii_lowercase()
        .ends_with("cmd.exe"));

        let missing = format!("windows-spawn-missing-{}.exe", std::process::id());
        assert_eq!(
            resolve_executable(
                OsStr::new(&missing),
                Some(OsStr::new("")),
                windows_backend(),
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        let empty_path_probe = format!("windows-spawn-empty-path-{}.exe", std::process::id());
        let empty_path_probe_path = env::current_dir().unwrap().join(&empty_path_probe);
        let _probe = ProbeFile::create(empty_path_probe_path);
        assert_eq!(
            resolve_executable(
                OsStr::new(&empty_path_probe),
                Some(OsStr::new(";")),
                windows_backend(),
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        let nul = OsString::from_wide(&[u16::from(b'x'), 0]);
        assert_eq!(
            wide_nul(&nul).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn executable_resolution_searches_process_system_and_environment_paths() {
        let explicit_missing = env::temp_dir().join(format!(
            "windows-spawn-explicit-missing-{}",
            std::process::id()
        ));
        assert_eq!(
            decode(
                &resolve_executable_with(
                    explicit_missing.as_os_str(),
                    None,
                    Err(io::Error::other("current application unavailable")),
                    None,
                    windows_backend(),
                )
                .unwrap(),
            ),
            explicit_missing.to_string_lossy()
        );

        let current = env::current_exe().unwrap();
        let current_name = current.file_name().unwrap();
        assert_eq!(
            decode(
                &resolve_executable_with(
                    current_name,
                    None,
                    Ok(current.clone()),
                    None,
                    windows_backend(),
                )
                .unwrap(),
            ),
            current.to_string_lossy()
        );

        let explorer = resolve_executable_with(
            OsStr::new("explorer.exe"),
            None,
            Err(io::Error::other("current application unavailable")),
            None,
            windows_backend(),
        )
        .unwrap();
        assert!(decode(&explorer)
            .to_ascii_lowercase()
            .ends_with("explorer.exe"));

        let path_probe = format!("windows-spawn-path-probe-{}.exe", std::process::id());
        let path_probe_file = env::temp_dir().join(&path_probe);
        let probe = ProbeFile::create(path_probe_file);
        assert_eq!(
            decode(
                &resolve_executable_with(
                    OsStr::new(&path_probe),
                    None,
                    Err(io::Error::other("current application unavailable")),
                    Some(env::temp_dir().as_os_str()),
                    windows_backend(),
                )
                .unwrap(),
            ),
            probe.path().to_string_lossy()
        );

        let missing = format!("windows-spawn-missing-{}.exe", std::process::id());
        configure_fault(Some(1));
        assert!(resolve_executable_with(
            OsStr::new(&missing),
            None,
            Err(io::Error::other("current application unavailable")),
            None,
            fault_backend(),
        )
        .is_err());
        configure_fault(None);

        let nul_path = OsString::from_wide(&[u16::from(b'X'), 0]);
        assert!(resolve_executable_with(
            OsStr::new(&missing),
            Some(nul_path.as_os_str()),
            Err(io::Error::other("current application unavailable")),
            None,
            windows_backend(),
        )
        .is_err());
        let nul_program = OsString::from_wide(&[
            u16::from(b'C'),
            u16::from(b':'),
            u16::from(b'\\'),
            u16::from(b'X'),
            0,
        ]);
        assert!(resolve_executable_with(
            nul_program.as_os_str(),
            None,
            Err(io::Error::other("current application unavailable")),
            None,
            windows_backend(),
        )
        .is_err());
    }

    #[test]
    fn uncommitted_running_and_suspended_transactions_roll_back() {
        let mut running_command = Command::new("cmd.exe");
        running_command.args(["/D", "/C", "ping -n 10 127.0.0.1 >nul"]);
        let running =
            ValidatedPlan::running(&running_command, SpawnOptions::new(), IoMode::Spawn).unwrap();
        let running_transaction = PreparedSpawn::prepare(running, windows_backend())
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut running_process = ProcessExitGuard::new(
            sys::duplicate_local(
                running_transaction.resources.process.process_handle(),
                Inheritability::Private,
            )
            .unwrap(),
        );
        drop(running_transaction);
        running_process.assert_exited("rollback did not terminate its running process");

        let mut suspended_command = Command::new("cmd.exe");
        suspended_command.args(["/D", "/C", "ping -n 10 127.0.0.1 >nul"]);
        let suspended =
            ValidatedPlan::suspended(&suspended_command, SpawnOptions::new(), IoMode::Spawn)
                .unwrap();
        let suspended_transaction = PreparedSpawn::prepare(suspended, windows_backend())
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut suspended_process = ProcessExitGuard::new(
            sys::duplicate_local(
                suspended_transaction.resources.process.process_handle(),
                Inheritability::Private,
            )
            .unwrap(),
        );
        drop(suspended_transaction);
        suspended_process.assert_exited("rollback did not terminate its suspended process");
    }

    #[test]
    fn suspended_child_drop_terminates_the_process() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 0"]);
        let plan = ValidatedPlan::suspended(&command, SpawnOptions::new(), IoMode::Spawn).unwrap();
        let transaction = PreparedSpawn::prepare(plan, windows_backend())
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut process = ProcessExitGuard::new(
            sys::duplicate_local(
                transaction.resources.process.process_handle(),
                Inheritability::Private,
            )
            .unwrap(),
        );
        drop(transaction.reclaim().unwrap().into_suspended());
        process.assert_exited("dropping SuspendedChild did not terminate the process");
    }
}
