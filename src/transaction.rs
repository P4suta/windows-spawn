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

use crate::backend::{AttributeAddress, SpawnBackend, WindowsBackend};
use crate::child::{Child, JobOwnership, ProcessOwner, SuspendedChild};
use crate::command::{Arg, Command, EnvOp, EnvValue, EnvironmentBase};
use crate::core_logic::{CommandLine, CommandLineError};
use crate::error::{CleanupError, Error, Operation, Phase, Result, WindowsError};
use crate::handles::StdioInner;
use crate::options::{ConsoleMode, JobClosePolicy, SpawnOptions, TerminalMode};
use crate::plan::{
    DesiredState, Running, StandardHandles, StandardIo, StdioSpec, Suspended, ValidatedPlan,
};
use crate::resource::{ChildHandleValue, CurrentTable, SelectedTable};
use crate::sys::{self, Inheritability, InitialState, NullAccess, PipeDirection, StandardStream};
use crate::trace::{self, ResourceKind};

const MAX_ATTRIBUTE_COUNT: u32 = 5;

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
    fn new<Backend: SpawnBackend>(backing: AttributeBacking<'a, Table>) -> Result<Self> {
        let backing = Box::pin(backing);
        let values = backing.as_ref().get_ref();
        let attribute_count = u32::from(!values.inherited_values.is_empty())
            + u32::from(values.parent_value.is_some())
            + u32::from(values.mitigation_value.is_some())
            + u32::from(!values.job_values.is_empty())
            + u32::from(values.pseudoconsole.is_some());
        if attribute_count > MAX_ATTRIBUTE_COUNT {
            return Err(Error::Validation(crate::ValidationError::SizeOverflow));
        }
        let mut list = if attribute_count == 0 {
            None
        } else {
            Some(Backend::create_attributes(attribute_count).map_err(attribute_error)?)
        };
        if let Some(attributes) = &mut list {
            if !values.inherited_values.is_empty() {
                Backend::set_handle_list(attributes, &values.inherited_values)
                    .map_err(attribute_error)?;
            }
            if let Some(parent) = &values.parent_value {
                Backend::set_parent(attributes, AttributeAddress::new(parent.as_ref()))
                    .map_err(attribute_error)?;
            }
            if let Some(mitigation) = &values.mitigation_value {
                Backend::set_mitigation(attributes, mitigation).map_err(attribute_error)?;
            }
            if !values.job_values.is_empty() {
                Backend::set_jobs(attributes, &values.job_values).map_err(attribute_error)?;
            }
            if let Some(pseudoconsole) = values.pseudoconsole {
                Backend::set_pseudoconsole(attributes, pseudoconsole).map_err(attribute_error)?;
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

pub(crate) struct PreparedSpawn<'options, State, Table, Backend = WindowsBackend> {
    application: Vec<u16>,
    command_line: Vec<u16>,
    environment: Environment,
    current_dir: Option<Vec<u16>>,
    stdio_values: sys::StartupStdio<Table>,
    stdio: StandardHandles<Option<OwnedHandle>>,
    job: JobOwnership,
    transfer: HandleTransfer<'options, Backend>,
    attributes: ProcessAttributeList<'options, Table>,
    creation_flags: u32,
    state: PhantomData<(State, Backend)>,
}

impl<'options, State: DesiredState>
    PreparedSpawn<'options, State, SelectedTable<'options>, WindowsBackend>
{
    pub(crate) fn prepare(plan: ValidatedPlan<'_, 'options, State>) -> Result<Self> {
        Self::prepare_with_backend(plan)
    }
}

impl<'options, State: DesiredState, Backend: SpawnBackend>
    PreparedSpawn<'options, State, SelectedTable<'options>, Backend>
{
    fn prepare_with_backend(plan: ValidatedPlan<'_, 'options, State>) -> Result<Self> {
        let (command, options, stdio_plan) = plan.into_parts();
        let parent: Option<BorrowedHandle<'options>> = options.parent.map(AsHandle::as_handle);
        let mut transfer = HandleTransfer::<Backend>::new(parent);
        let (stdio_values, stdio) = prepare_standard_io::<Backend>(&stdio_plan, &mut transfer)?;
        let (job, job_values) = prepare_job::<Backend>(&options)?;

        let command_line = build_command_line::<Backend>(command, &mut transfer)?;
        let environment =
            build_environment::<Backend>(command, &mut transfer).map_err(|error| {
                Error::windows(Phase::Preparation, Operation::ReadEnvironment, error)
            })?;
        let child_path = environment.path.as_deref();
        let application =
            resolve_executable::<Backend>(&command.program, child_path).map_err(|error| {
                Error::windows(Phase::Preparation, Operation::ResolveExecutable, error)
            })?;
        let current_dir = command
            .cwd
            .as_ref()
            .map(|path| wide_nul(path.as_os_str()))
            .transpose()
            .map_err(|error| {
                Error::windows(Phase::Preparation, Operation::ResolveExecutable, error)
            })?;

        let attributes = prepare_attributes::<Backend>(&options, &transfer, job_values)?;
        let console_flags = match options.terminal {
            TerminalMode::Console(mode) => mode.creation_bits(),
            TerminalMode::PseudoConsole(_) => ConsoleMode::Inherit.creation_bits(),
        };
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
            creation_flags: options.creation.bits() | console_flags,
            state: PhantomData,
        })
    }

    pub(crate) fn create_suspended(
        mut self,
    ) -> Result<CreatedSuspended<'options, State, SelectedTable<'options>, Backend>> {
        let mut request = sys::ProcessRequest {
            application: &self.application,
            command_line: &mut self.command_line,
            environment: self.environment.block.as_deref(),
            current_dir: self.current_dir.as_deref(),
            stdio: self.stdio_values,
            inheritability: if self.transfer.inherited_values().is_empty() {
                Inheritability::Private
            } else {
                Inheritability::Inheritable
            },
            creation_flags: self.creation_flags,
            initial_state: InitialState::Suspended,
            attributes: self.attributes.as_list(),
        };
        let created = trace::io(
            Phase::Creation,
            Operation::CreateProcess,
            ResourceKind::Process,
            || Backend::create_process(&mut request),
        );
        match created {
            Ok(created) => Ok(CreatedSuspended {
                process: ProcessOwner::new(created),
                job: self.job,
                stdio: self.stdio,
                transfer: self.transfer,
                attributes: self.attributes,
                state: PhantomData,
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

fn prepare_standard_io<'options, Backend: SpawnBackend>(
    plan: &StandardIo<'_>,
    transfer: &mut HandleTransfer<'options, Backend>,
) -> Result<(
    sys::StartupStdio<SelectedTable<'options>>,
    StandardHandles<Option<OwnedHandle>>,
)> {
    match plan {
        StandardIo::Ordinary(specs) => {
            let prepared =
                prepare_standard_handles::<Backend>(specs, transfer).map_err(|error| {
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

fn prepare_job<Backend: SpawnBackend>(
    options: &SpawnOptions<'_>,
) -> Result<(JobOwnership, Vec<ChildHandleValue<CurrentTable>>)> {
    let job = match options.job_close {
        JobClosePolicy::PreserveProcesses => JobOwnership::Preserve,
        JobClosePolicy::TerminateProcesses => {
            let job = trace::io(
                Phase::Preparation,
                Operation::CreateJob,
                ResourceKind::Job,
                Backend::create_job,
            )
            .map_err(|error| Error::windows(Phase::Preparation, Operation::CreateJob, error))?;
            trace::io(
                Phase::Preparation,
                Operation::ConfigureJob,
                ResourceKind::Job,
                || Backend::configure_job(&job, JobClosePolicy::TerminateProcesses),
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

fn prepare_attributes<'options, Backend: SpawnBackend>(
    options: &SpawnOptions<'options>,
    transfer: &HandleTransfer<'options, Backend>,
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
    ProcessAttributeList::new::<Backend>(AttributeBacking {
        inherited_values,
        parent_value,
        mitigation_value,
        job_values: job_values.into_boxed_slice(),
        pseudoconsole,
    })
}

pub(crate) struct CreatedSuspended<'options, State, Table, Backend = WindowsBackend> {
    process: ProcessOwner,
    job: JobOwnership,
    stdio: StandardHandles<Option<OwnedHandle>>,
    transfer: HandleTransfer<'options, Backend>,
    attributes: ProcessAttributeList<'options, Table>,
    state: PhantomData<(State, Backend)>,
}

impl<State, Table, Backend: SpawnBackend> CreatedSuspended<'_, State, Table, Backend> {
    pub(crate) fn reclaim(self) -> Result<Reclaimed<State, Table, Backend>> {
        let Self {
            process,
            job,
            stdio,
            transfer,
            attributes,
            state: _,
        } = self;
        drop(attributes);
        transfer.reclaim().map_err(Error::Cleanup)?;
        Ok(Reclaimed {
            process,
            job,
            stdio,
            state: PhantomData,
        })
    }
}

pub(crate) struct Reclaimed<State, Table, Backend = WindowsBackend> {
    process: ProcessOwner,
    job: JobOwnership,
    stdio: StandardHandles<Option<OwnedHandle>>,
    state: PhantomData<(State, Table, Backend)>,
}

impl<Table, Backend: SpawnBackend> Reclaimed<Running, Table, Backend> {
    pub(crate) fn resume(self) -> Result<Child> {
        let mut child = Child::new(
            self.process,
            self.job,
            self.stdio.stdin,
            self.stdio.stdout,
            self.stdio.stderr,
        );
        child.resume_initial_with(Backend::resume_thread)?;
        Ok(child)
    }
}

impl<Table> Reclaimed<Suspended, Table, WindowsBackend> {
    pub(crate) fn into_suspended(self) -> SuspendedChild {
        SuspendedChild::new(Child::new(
            self.process,
            self.job,
            self.stdio.stdin,
            self.stdio.stdout,
            self.stdio.stderr,
        ))
    }
}

struct PreparedStdio<'parent> {
    child: ChildHandleValue<SelectedTable<'parent>>,
    parent: Option<OwnedHandle>,
}

fn prepare_standard_handles<'parent, Backend: SpawnBackend>(
    specs: &StandardHandles<StdioSpec<'_>>,
    transfer: &mut HandleTransfer<'parent, Backend>,
) -> io::Result<StandardHandles<PreparedStdio<'parent>>> {
    Ok(StandardHandles {
        stdin: prepare_stdio::<Backend>(specs.stdin, StandardStream::Input, transfer)?,
        stdout: prepare_stdio::<Backend>(specs.stdout, StandardStream::Output, transfer)?,
        stderr: prepare_stdio::<Backend>(specs.stderr, StandardStream::Error, transfer)?,
    })
}

fn prepare_stdio<'parent, Backend: SpawnBackend>(
    spec: StdioSpec<'_>,
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent, Backend>,
) -> io::Result<PreparedStdio<'parent>> {
    match spec {
        StdioSpec::Inherit => prepare_inherit::<Backend>(stream, transfer),
        StdioSpec::Null => prepare_null::<Backend>(stream, transfer),
        StdioSpec::Piped => prepare_pipe::<Backend>(stream, transfer),
        StdioSpec::Configured(stdio) => match &stdio.inner {
            StdioInner::Inherit => prepare_inherit::<Backend>(stream, transfer),
            StdioInner::Null => prepare_null::<Backend>(stream, transfer),
            StdioInner::Piped => prepare_pipe::<Backend>(stream, transfer),
            StdioInner::Owned(handle) => Ok(PreparedStdio {
                child: transfer.lower(handle.as_handle())?,
                parent: None,
            }),
        },
    }
}

fn prepare_inherit<'parent, Backend: SpawnBackend>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent, Backend>,
) -> io::Result<PreparedStdio<'parent>> {
    match Backend::standard_handle(stream)? {
        Some(handle) => Ok(PreparedStdio {
            child: transfer.lower(handle.as_handle())?,
            parent: None,
        }),
        None => Ok(PreparedStdio {
            child: sys::invalid_child_handle(),
            parent: None,
        }),
    }
}

fn prepare_null<'parent, Backend: SpawnBackend>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent, Backend>,
) -> io::Result<PreparedStdio<'parent>> {
    let access = match stream {
        StandardStream::Input => NullAccess::Read,
        StandardStream::Output | StandardStream::Error => NullAccess::Write,
    };
    let handle = Backend::null_handle(access)?;
    Ok(PreparedStdio {
        child: transfer.lower(handle.as_handle())?,
        parent: None,
    })
}

fn prepare_pipe<'parent, Backend: SpawnBackend>(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'parent, Backend>,
) -> io::Result<PreparedStdio<'parent>> {
    let direction = match stream {
        StandardStream::Input => PipeDirection::ParentWrites,
        StandardStream::Output | StandardStream::Error => PipeDirection::ParentReads,
    };
    let pipe = trace::io(
        Phase::Preparation,
        Operation::CreatePipe,
        ResourceKind::Pipe,
        || Backend::create_pipe(direction),
    )?;
    let child = transfer.lower(pipe.child.as_handle())?;
    Ok(PreparedStdio {
        child,
        parent: Some(pipe.parent),
    })
}

struct HandleTransfer<'a, Backend = WindowsBackend> {
    parent: Option<BorrowedHandle<'a>>,
    local: Vec<OwnedHandle>,
    remote: Vec<sys::RemoteHandle<'a>>,
    inherited: Vec<ChildHandleValue<SelectedTable<'a>>>,
    backend: PhantomData<Backend>,
}

impl<'a, Backend: SpawnBackend> HandleTransfer<'a, Backend> {
    fn new(parent: Option<BorrowedHandle<'a>>) -> Self {
        Self {
            parent,
            local: Vec::new(),
            remote: Vec::new(),
            inherited: Vec::new(),
            backend: PhantomData,
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
                || Backend::duplicate_remote(source, parent, Inheritability::Inheritable),
            )?;
            let value = handle.value();
            self.remote.push(handle);
            value
        } else {
            let handle = trace::io(
                Phase::Preparation,
                Operation::DuplicateLocalHandle,
                ResourceKind::Handle,
                || Backend::duplicate_local(source, Inheritability::Inheritable),
            )?;
            let value = sys::child_handle_value(handle.as_handle());
            self.local.push(handle);
            value
        };
        if !self.inherited.contains(&value) {
            self.inherited.push(value);
        }
        Ok(value)
    }

    fn parent(&self) -> Option<BorrowedHandle<'a>> {
        self.parent
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
                || Backend::reclaim_remote(handle),
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

fn build_environment<Backend: SpawnBackend>(
    command: &Command,
    transfer: &mut HandleTransfer<'_, Backend>,
) -> io::Result<Environment> {
    if command.environment_base == EnvironmentBase::Inherit && command.env_ops.is_empty() {
        return Ok(Environment {
            block: None,
            path: None,
        });
    }

    let mut map: BTreeMap<EnvKey, OsString> = BTreeMap::new();
    if command.environment_base == EnvironmentBase::Inherit {
        for (key, value) in Backend::environment_strings()? {
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

fn build_command_line<Backend: SpawnBackend>(
    command: &Command,
    transfer: &mut HandleTransfer<'_, Backend>,
) -> Result<Vec<u16>> {
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
    result.finish().map_err(command_line_error)
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

fn resolve_executable<Backend: SpawnBackend>(
    program: &OsStr,
    child_path: Option<&OsStr>,
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
    if let Ok(mut application) = env::current_exe() {
        application.pop();
        if let Some(found) = search(application) {
            return Ok(found);
        }
    }
    if let Some(found) = search(PathBuf::from(Backend::system_directory()?)) {
        return Ok(found);
    }
    if let Some(found) = search(PathBuf::from(Backend::windows_directory()?)) {
        return Ok(found);
    }
    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths).filter(|path| !path.as_os_str().is_empty()) {
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
        configure_fault, configure_faults, fault_calls, BackendCall, FaultBackend,
    };
    use crate::plan::IoMode;
    use crate::{DepPolicy, MitigationPolicy, ParentProcess, Stdio};

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
        let plan = ValidatedPlan::running(&command, options, IoMode::Output)?;
        let mut child =
            PreparedSpawn::<Running, SelectedTable<'_>, FaultBackend>::prepare_with_backend(plan)?
                .create_suspended()?
                .reclaim()?
                .resume()?;
        let _status = child.wait()?;
        child.cleanup()
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
        let plan = ValidatedPlan::running(&command, options, IoMode::Spawn)?;
        let mut child =
            PreparedSpawn::<Running, SelectedTable<'_>, FaultBackend>::prepare_with_backend(plan)?
                .create_suspended()?
                .reclaim()?
                .resume()?;
        let _status = child.wait()?;
        child.cleanup()
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
        let mut transfer = HandleTransfer::<WindowsBackend>::new(None);
        let line = build_command_line(&command, &mut transfer).unwrap();
        assert_eq!(decode(&line), r#""program.exe" "a b" "a\"b" x&&y"#);
    }

    #[test]
    fn executable_search_finds_system_command_without_current_directory() {
        let command = resolve_executable::<WindowsBackend>(OsStr::new("cmd"), None).unwrap();
        assert!(decode(&command).to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn cleared_environment_is_double_nul() {
        let mut command = Command::new("cmd.exe");
        command.env_clear();
        let mut transfer = HandleTransfer::<WindowsBackend>::new(None);
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
        let mut transfer = HandleTransfer::<WindowsBackend>::new(None);
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
        let mut transfer = HandleTransfer::<WindowsBackend>::new(None);
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
        let mut transfer = HandleTransfer::<WindowsBackend>::new(None);
        let line = decode(&build_command_line(&command, &mut transfer).unwrap());
        assert_eq!(line, r#""program.exe" "" "C:\path with spaces\\""#);
    }

    #[test]
    fn executable_resolution_covers_explicit_child_path_and_not_found() {
        let system = PathBuf::from(sys::system_directory().unwrap());
        let executable = system.join("cmd.exe");
        assert_eq!(
            decode(&resolve_executable::<WindowsBackend>(executable.as_os_str(), None).unwrap()),
            executable.to_string_lossy()
        );
        let without_extension = system.join("cmd");
        assert_eq!(
            decode(
                &resolve_executable::<WindowsBackend>(without_extension.as_os_str(), None).unwrap(),
            ),
            executable.to_string_lossy()
        );
        assert!(decode(
            &resolve_executable::<WindowsBackend>(OsStr::new("cmd.exe"), Some(system.as_os_str()),)
                .unwrap()
        )
        .to_ascii_lowercase()
        .ends_with("cmd.exe"));

        let missing = format!("windows-spawn-missing-{}.exe", std::process::id());
        assert_eq!(
            resolve_executable::<WindowsBackend>(OsStr::new(&missing), Some(OsStr::new("")))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        let empty_path_probe = format!("windows-spawn-empty-path-{}.exe", std::process::id());
        let empty_path_probe_path = env::current_dir().unwrap().join(&empty_path_probe);
        let probe = File::create(&empty_path_probe_path).unwrap();
        assert_eq!(
            resolve_executable::<WindowsBackend>(
                OsStr::new(&empty_path_probe),
                Some(OsStr::new(";")),
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        drop(probe);
        std::fs::remove_file(empty_path_probe_path).unwrap();
        let nul = OsString::from_wide(&[u16::from(b'x'), 0]);
        assert_eq!(
            wide_nul(&nul).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn uncommitted_running_and_suspended_transactions_roll_back() {
        let mut running_command = Command::new("cmd.exe");
        running_command.args(["/D", "/C", "ping -n 10 127.0.0.1 >nul"]);
        let running =
            ValidatedPlan::running(&running_command, SpawnOptions::new(), IoMode::Spawn).unwrap();
        let running_transaction = PreparedSpawn::prepare(running)
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut running_process = ProcessExitGuard::new(
            sys::duplicate_local(
                running_transaction.process.process_handle(),
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
        let suspended_transaction = PreparedSpawn::prepare(suspended)
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut suspended_process = ProcessExitGuard::new(
            sys::duplicate_local(
                suspended_transaction.process.process_handle(),
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
        let transaction = PreparedSpawn::prepare(plan)
            .unwrap()
            .create_suspended()
            .unwrap();
        let mut process = ProcessExitGuard::new(
            sys::duplicate_local(
                transaction.process.process_handle(),
                Inheritability::Private,
            )
            .unwrap(),
        );
        drop(transaction.reclaim().unwrap().into_suspended());
        process.assert_exited("dropping SuspendedChild did not terminate the process");
    }
}
