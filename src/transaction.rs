//! Resource-acquiring spawn transaction and rollback.

use std::cmp::Ordering;
use std::collections::btree_map::{BTreeMap, Entry};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::marker::PhantomData;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle};
use std::path::{Path, PathBuf};

use crate::child::{Child, SuspendedChild};
use crate::command::{Arg, Command, EnvOp, EnvValue};
use crate::domain::attribute_rules;
use crate::domain::command_line_rules::{append_argument, append_program, NUL, QUOTE, SPACE};
use crate::domain::environment_rules;
use crate::handles::{Job, Stdio, StdioInner};
use crate::options::DropPolicy;
use crate::plan::{
    validate_command, Running, Source, SpawnPlan, SpawnState, StandardIo, Streams, Suspended,
};
use crate::sys::{self, NullAccess, StandardStream};

/// Owns the process between `CreateProcessW` success and an explicit commit.
/// Dropping before commit terminates the partially-created process.
pub(crate) struct SpawnTransaction<M> {
    created: Option<sys::CreatedProcess>,
    kill_job: Option<Job>,
    stdio: Streams<Option<OwnedHandle>>,
    state: PhantomData<M>,
}

impl<M> SpawnTransaction<M> {
    /// Borrows the created process's primary thread.
    #[cfg(test)]
    pub(crate) fn primary_thread(&self) -> BorrowedHandle<'_> {
        self.created
            .as_ref()
            .expect("an uncommitted transaction owns its process")
            .thread
            .as_handle()
    }
}

impl<M: SpawnState> SpawnTransaction<M> {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new<'options>(plan: &SpawnPlan<'_, 'options, M>) -> io::Result<Self> {
        let parent: Option<BorrowedHandle<'options>> = plan.options.parent.map(AsHandle::as_handle);
        let mut transfer = HandleTransfer::new(parent);
        let (stdio_values, stdio) = match &plan.stdio {
            StandardIo::Ordinary(specs) => {
                let prepared = prepare_standard_handles(specs, &mut transfer)?;
                let values = sys::StartupStdio::Ordinary(sys::StandardHandles {
                    stdin: prepared.stdin.child,
                    stdout: prepared.stdout.child,
                    stderr: prepared.stderr.child,
                });
                let owners = Streams {
                    stdin: prepared.stdin.parent,
                    stdout: prepared.stdout.parent,
                    stderr: prepared.stderr.parent,
                };
                (values, owners)
            }
            StandardIo::PseudoConsole => (
                sys::StartupStdio::PseudoConsole,
                Streams {
                    stdin: None,
                    stdout: None,
                    stderr: None,
                },
            ),
        };
        let kill_job = if plan.options.drop_policy == DropPolicy::KillTree {
            let job = Job::create()?;
            job.set_kill_on_close(true)?;
            Some(job)
        } else {
            None
        };
        let mut job_values: Vec<isize> = plan
            .options
            .jobs
            .iter()
            .map(|job| sys::handle_value(job.as_handle().as_raw_handle()))
            .collect();
        if let Some(job) = &kill_job {
            job_values.push(sys::handle_value(job.as_handle().as_raw_handle()));
        }

        let command_line = build_command_line(plan.command, &mut transfer)?;
        let environment = build_environment(plan.command, &mut transfer)?;
        let child_path = environment.path.as_deref();
        let application = resolve_executable(&plan.command.program, child_path)?;
        let current_dir = plan
            .command
            .cwd
            .as_ref()
            .map(|path| wide_nul(path.as_os_str()))
            .transpose()?;

        let inherited_values = transfer.inherited_values().to_vec().into_boxed_slice();
        let parent_value = transfer
            .parent()
            .map(|handle| Box::new(sys::handle_value(handle.as_raw_handle())));
        let mitigation_words = plan.options.mitigation.words();
        let mitigation_value = (mitigation_words != [0, 0]).then(|| Box::new(mitigation_words));
        let job_values = job_values.into_boxed_slice();
        let pseudoconsole = plan.options.pseudoconsole_raw();

        let attribute_count = attribute_rules::count(&[
            !inherited_values.is_empty(),
            parent_value.is_some(),
            mitigation_value.is_some(),
            !job_values.is_empty(),
            pseudoconsole.is_some(),
        ]);
        let mut attributes = if attribute_count == 0 {
            None
        } else {
            Some(sys::AttributeList::new(attribute_count)?)
        };
        if let Some(list) = &mut attributes {
            if !inherited_values.is_empty() {
                list.set_handle_list(&inherited_values)?;
            }
            if let Some(value) = &parent_value {
                list.set_parent(value)?;
            }
            if let Some(value) = &mitigation_value {
                list.set_mitigation(value)?;
            }
            if !job_values.is_empty() {
                list.set_jobs(&job_values)?;
            }
            if let Some(value) = pseudoconsole {
                list.set_pseudoconsole(value)?;
            }
        }

        let mut command_line = command_line;
        let mut request = sys::ProcessRequest {
            application: &application,
            command_line: &mut command_line,
            environment: environment.block.as_deref(),
            current_dir: current_dir.as_deref(),
            stdio: stdio_values,
            inherit_handles: !inherited_values.is_empty(),
            creation_flags: plan.options.creation_flags.bits(),
            suspended: M::SUSPENDED,
            attributes: attributes.as_ref(),
        };
        let created = sys::create_process(&mut request)?;

        drop(attributes);
        drop(transfer);

        Ok(Self {
            created: Some(created),
            kill_job,
            stdio,
            state: PhantomData,
        })
    }

    #[allow(clippy::expect_used)]
    fn commit_parts(mut self) -> (Child, OwnedHandle) {
        let created = self
            .created
            .take()
            .expect("an uncommitted transaction owns its process");
        let child = Child::new(
            created.process,
            created.pid,
            self.kill_job.take(),
            self.stdio.stdin.take(),
            self.stdio.stdout.take(),
            self.stdio.stderr.take(),
        );
        (child, created.thread)
    }
}

impl SpawnTransaction<Running> {
    pub(crate) fn commit_child(self) -> Child {
        let (child, thread) = self.commit_parts();
        drop(thread);
        child
    }
}

impl SpawnTransaction<Suspended> {
    pub(crate) fn commit_suspended(self) -> SuspendedChild {
        let (child, thread) = self.commit_parts();
        SuspendedChild::new(child, thread)
    }
}

impl<M> Drop for SpawnTransaction<M> {
    fn drop(&mut self) {
        if let Some(created) = &self.created {
            drop(sys::terminate_process(created.process.as_handle(), 1));
        }
        drop(self.kill_job.take());
    }
}

struct PreparedStdio {
    child: isize,
    parent: Option<OwnedHandle>,
}

fn prepare_standard_handles(
    specs: &Streams<Source<&Stdio>>,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<Streams<PreparedStdio>> {
    Ok(Streams {
        stdin: prepare_stdio(specs.stdin, StandardStream::Input, transfer)?,
        stdout: prepare_stdio(specs.stdout, StandardStream::Output, transfer)?,
        stderr: prepare_stdio(specs.stderr, StandardStream::Error, transfer)?,
    })
}

fn prepare_stdio(
    spec: Source<&Stdio>,
    stream: StandardStream,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<PreparedStdio> {
    match spec {
        Source::Inherit => prepare_inherit(stream, transfer),
        Source::Null => prepare_null(stream, transfer),
        Source::Piped => prepare_pipe(stream, transfer),
        Source::Configured(stdio) => match &stdio.inner {
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

fn prepare_inherit(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<PreparedStdio> {
    match sys::standard_handle(stream)? {
        Some(handle) => Ok(PreparedStdio {
            child: transfer.lower(handle.as_handle())?,
            parent: None,
        }),
        None => Ok(PreparedStdio {
            child: sys::INVALID_RAW_HANDLE,
            parent: None,
        }),
    }
}

fn prepare_null(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<PreparedStdio> {
    let access = match stream {
        StandardStream::Input => NullAccess::Read,
        StandardStream::Output | StandardStream::Error => NullAccess::Write,
    };
    let handle = sys::null_handle(access)?;
    Ok(PreparedStdio {
        child: transfer.lower(handle.as_handle())?,
        parent: None,
    })
}

fn prepare_pipe(
    stream: StandardStream,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<PreparedStdio> {
    let pipe = sys::create_pipe(!matches!(stream, StandardStream::Input))?;
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
    inherited: Vec<isize>,
}

impl<'a> HandleTransfer<'a> {
    const fn new(parent: Option<BorrowedHandle<'a>>) -> Self {
        Self {
            parent,
            local: Vec::new(),
            remote: Vec::new(),
            inherited: Vec::new(),
        }
    }

    fn lower(&mut self, source: BorrowedHandle<'_>) -> io::Result<isize> {
        let value = if let Some(parent) = self.parent {
            let handle = sys::duplicate_remote(source, parent, true)?;
            let value = handle.value();
            self.remote.push(handle);
            value
        } else {
            let handle = sys::duplicate_local(source, true)?;
            let value = sys::handle_value(handle.as_raw_handle());
            self.local.push(handle);
            value
        };
        self.inherited.push(value);
        Ok(value)
    }

    const fn parent(&self) -> Option<BorrowedHandle<'a>> {
        self.parent
    }

    fn inherited_values(&self) -> &[isize] {
        &self.inherited
    }
}

struct Environment {
    block: Option<Vec<u16>>,
    path: Option<OsString>,
}

#[derive(Debug)]
struct EnvKey {
    wide: Vec<u16>,
}

impl EnvKey {
    fn new(text: &OsStr) -> Self {
        Self { wide: wide(text) }
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
    if !command.env_clear && command.env_ops.is_empty() {
        return Ok(Environment {
            block: None,
            path: None,
        });
    }

    let mut map: BTreeMap<EnvKey, OsString> = BTreeMap::new();
    if !command.env_clear {
        for (key, value) in sys::environment_strings()? {
            map.insert(EnvKey::new(&key), value);
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
                match map.entry(EnvKey::new(key)) {
                    Entry::Occupied(mut entry) => {
                        entry.insert(value);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            }
            EnvOp::Remove(key) => {
                map.remove(&EnvKey::new(key));
            }
        }
    }

    let path = map.get(&EnvKey::new(OsStr::new("PATH"))).cloned();
    let mut block = Vec::new();
    for (key, value) in map {
        let value: Vec<u16> = value.encode_wide().collect();
        environment_rules::append_entry(&mut block, &key.wide, &value);
    }
    environment_rules::finish(&mut block);
    Ok(Environment {
        block: Some(block),
        path,
    })
}

/// Returns the `PATH` that [`build_environment`] would produce, without lowering handles.
///
/// A `PATH` that a later operation replaces or removes does not matter, so only a final value set from a handle, which exists only inside a spawn, is an error.
fn child_path(command: &Command) -> io::Result<Option<OsString>> {
    if !command.env_clear && command.env_ops.is_empty() {
        return Ok(None);
    }
    let path_key = EnvKey::new(OsStr::new("PATH"));
    let mut path: Option<Result<OsString, ()>> = None;
    if !command.env_clear {
        for (key, value) in sys::environment_strings()? {
            if EnvKey::new(&key) == path_key {
                path = Some(Ok(value));
            }
        }
    }
    for operation in &command.env_ops {
        match operation {
            EnvOp::Set(key, value) => {
                if EnvKey::new(key) == path_key {
                    path = Some(match value {
                        EnvValue::Text(value) => Ok(value.clone()),
                        EnvValue::Handle(_) => Err(()),
                    });
                }
            }
            EnvOp::Remove(key) => {
                if EnvKey::new(key) == path_key {
                    path = None;
                }
            }
        }
    }
    path.transpose().map_err(|()| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a PATH set from a handle has a value only inside a spawn",
        )
    })
}

/// Implements [`Command::to_command_line`].
pub(crate) fn broker_command_line(command: &Command) -> io::Result<OsString> {
    validate_command(command)?;
    let mut arguments = Vec::new();
    for argument in &command.args {
        arguments.push(SPACE);
        match argument {
            Arg::Text(text) => append_argument(&mut arguments, &wide(text)),
            Arg::Raw(text) => arguments.extend(text.encode_wide()),
            Arg::Handle(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a handle argument has a value only inside a spawn",
                ))
            }
        }
    }
    let path = child_path(command)?;
    let application = resolve_executable(&command.program, path.as_deref())?;
    let absolute = sys::full_path(&application)?;
    if absolute.contains(&QUOTE) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resolved program path contains a double quote",
        ));
    }
    let mut line = Vec::new();
    append_program(&mut line, &absolute);
    line.extend(arguments);
    Ok(OsString::from_wide(&line))
}

fn build_command_line(
    command: &Command,
    transfer: &mut HandleTransfer<'_>,
) -> io::Result<Vec<u16>> {
    let mut result = Vec::new();
    append_program(&mut result, &wide(&command.program));
    for argument in &command.args {
        result.push(SPACE);
        match argument {
            Arg::Text(text) => append_argument(&mut result, &wide(text)),
            Arg::Raw(text) => result.extend(text.encode_wide()),
            Arg::Handle(handle) => {
                let value = transfer.lower(handle.as_handle())?.to_string();
                append_argument(&mut result, &wide(OsStr::new(&value)));
            }
        }
    }
    result.push(NUL);
    Ok(result)
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().collect()
}

/// What executable resolution reads from the system, so a test can stand in for the file system and the process.
trait SearchHost {
    /// Returns true if a file or directory exists at `path`, a NUL-terminated wide path.
    fn exists(&self, path: &[u16]) -> bool;
    /// The directory of this process's executable.
    fn application_directory(&self) -> Option<PathBuf>;
    fn system_directory(&self) -> io::Result<OsString>;
    fn windows_directory(&self) -> io::Result<OsString>;
    /// This process's PATH.
    fn parent_path(&self) -> Option<OsString>;
}

/// The running system.
struct System;

impl SearchHost for System {
    fn exists(&self, path: &[u16]) -> bool {
        sys::program_exists(path)
    }

    fn application_directory(&self) -> Option<PathBuf> {
        env::current_exe().ok().map(|mut application| {
            application.pop();
            application
        })
    }

    fn system_directory(&self) -> io::Result<OsString> {
        sys::system_directory()
    }

    fn windows_directory(&self) -> io::Result<OsString> {
        sys::windows_directory()
    }

    fn parent_path(&self) -> Option<OsString> {
        env::var_os("PATH")
    }
}

fn resolve_executable(program: &OsStr, child_path: Option<&OsStr>) -> io::Result<Vec<u16>> {
    resolve_executable_in(&System, program, child_path)
}

/// The extension a program found by name gains, and that a path keeps as it is.
const EXE_SUFFIX: &[u8] = b".exe";

/// Finds `program` as Rust's standard library does on Windows; `proofs/prolog` states that search.
fn resolve_executable_in(
    host: &impl SearchHost,
    program: &OsStr,
    child_path: Option<&OsStr>,
) -> io::Result<Vec<u16>> {
    let program_path = Path::new(program);
    let has_exe_suffix = program
        .as_encoded_bytes()
        .get(program.len().saturating_sub(EXE_SUFFIX.len())..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(EXE_SUFFIX));
    let is_file_name = program_path.file_name() == Some(program);

    if !is_file_name {
        if has_exe_suffix {
            return wide_nul(program);
        }
        let mut appended = program.to_os_string();
        appended.push(".exe");
        let appended_wide = wide_nul(&appended)?;
        if host.exists(&appended_wide) {
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
        host.exists(&wide).then_some(wide)
    };

    if let Some(paths) = child_path {
        for directory in env::split_paths(paths).filter(|entry| !entry.as_os_str().is_empty()) {
            if let Some(found) = search(directory) {
                return Ok(found);
            }
        }
    }
    if let Some(application) = host.application_directory() {
        if let Some(found) = search(application) {
            return Ok(found);
        }
    }
    if let Some(found) = search(PathBuf::from(host.system_directory()?)) {
        return Ok(found);
    }
    if let Some(found) = search(PathBuf::from(host.windows_directory()?)) {
        return Ok(found);
    }
    if let Some(paths) = host.parent_path() {
        for directory in env::split_paths(&paths).filter(|entry| !entry.as_os_str().is_empty()) {
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
mod tests {
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::ffi::OsStringExt;

    use super::*;
    use crate::handles::Stdio;
    use crate::sys::test_support::{
        duplicate, pipe, suspend_count, tempt_resume, ProcessExitGuard,
    };

    fn decode(value: &[u16]) -> String {
        String::from_utf16_lossy(value.strip_suffix(&[0]).unwrap_or(value))
    }

    /// A file system and process that hold only what a vector row lists.
    #[cfg(windows_spawn_vectors)]
    struct Scenario {
        files: Vec<String>,
        application: PathBuf,
        system: OsString,
        windows: OsString,
        parent: Option<OsString>,
    }

    /// A path as the file system matches it: in any case, and with a slash read as a backslash.
    #[cfg(windows_spawn_vectors)]
    fn normal(path: &str) -> String {
        path.to_ascii_lowercase().replace('/', "\\")
    }

    #[cfg(windows_spawn_vectors)]
    impl SearchHost for Scenario {
        fn exists(&self, path: &[u16]) -> bool {
            self.files.contains(&normal(&decode(path)))
        }

        fn application_directory(&self) -> Option<PathBuf> {
            Some(self.application.clone())
        }

        fn system_directory(&self) -> io::Result<OsString> {
            Ok(self.system.clone())
        }

        fn windows_directory(&self) -> io::Result<OsString> {
            Ok(self.windows.clone())
        }

        fn parent_path(&self) -> Option<OsString> {
            self.parent.clone()
        }
    }

    /// Reads a PATH field of the vectors: `-` when unset, `=` and the text when set.
    #[cfg(windows_spawn_vectors)]
    fn path_field(field: &str) -> Option<OsString> {
        field.strip_prefix('=').map(OsString::from)
    }

    /// `proofs/prolog/vectors/executable_search.tsv` holds the search Rust's standard library documents,
    /// which `proofs/prolog/executable_search.pl` states apart from this code.
    #[cfg(windows_spawn_vectors)]
    #[test]
    fn executable_search_follows_the_prolog_model() {
        const VECTORS: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/proofs/prolog/vectors/executable_search.tsv"
        ));
        const ROWS: usize = 4672;
        let mut rows = VECTORS.lines();
        let host: Vec<&str> = rows.next().unwrap().split('\t').collect();
        let ["#host", application, system, windows] = host[..] else {
            panic!("the vectors start with the host row: {host:?}");
        };
        let mut checked = 0;
        for row in rows {
            let fields: Vec<&str> = row.split('\t').collect();
            let [program, child, parent, files, expected] = fields[..] else {
                panic!("a row has five fields: {row:?}");
            };
            assert!(child == "-" || child.starts_with('='), "{row:?}");
            assert!(parent == "-" || parent.starts_with('='), "{row:?}");
            let scenario = Scenario {
                files: files
                    .split('|')
                    .filter(|file| !file.is_empty())
                    .map(normal)
                    .collect(),
                application: PathBuf::from(application),
                system: OsString::from(system),
                windows: OsString::from(windows),
                parent: path_field(parent),
            };
            let child = path_field(child);
            let actual =
                match resolve_executable_in(&scenario, OsStr::new(program), child.as_deref()) {
                    Ok(path) => format!("path={}", decode(&path)),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => "not_found".to_owned(),
                    Err(error) => panic!("{row:?}: {error}"),
                };
            assert_eq!(actual, expected, "{row:?}");
            checked += 1;
        }
        assert_eq!(checked, ROWS);
    }

    #[test]
    fn quotes_regular_and_preserves_raw_arguments() {
        let mut command = Command::new("program.exe");
        command.arg("a b").arg("a\"b").raw_arg("x&&y");
        let mut transfer = HandleTransfer::new(None);
        let line = build_command_line(&command, &mut transfer).unwrap();
        assert_eq!(decode(&line), r#""program.exe" "a b" "a\"b" x&&y"#);
    }

    #[test]
    fn executable_search_finds_system_command_without_current_directory() {
        let command = resolve_executable(OsStr::new("cmd"), None).unwrap();
        assert!(decode(&command).to_ascii_lowercase().ends_with("cmd.exe"));
    }

    #[test]
    fn cleared_environment_is_double_nul() {
        let mut command = Command::new("cmd.exe");
        command.env_clear();
        let mut transfer = HandleTransfer::new(None);
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
        let mut transfer = HandleTransfer::new(None);
        let environment = build_environment(&command, &mut transfer).unwrap();
        assert_eq!(environment.path, Some(OsString::from("second")));
        let block = environment.block.unwrap();
        let text = String::from_utf16_lossy(&block);
        assert!(!text.contains("first"));
        assert!(!text.contains("REMOVE_ME"));

        let lower = EnvKey::new(OsStr::new("alpha"));
        let upper = EnvKey::new(OsStr::new("ALPHA"));
        assert_eq!(lower, upper);
        assert_eq!(lower.partial_cmp(&upper), Some(Ordering::Equal));
        assert_ne!(lower, EnvKey::new(OsStr::new("beta")));
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
        let mut transfer = HandleTransfer::new(None);
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
        assert_ne!(EnvKey::new(OsStr::new("S")), EnvKey::new(OsStr::new("ſ")));
        assert_ne!(EnvKey::new(OsStr::new("Μ")), EnvKey::new(OsStr::new("µ")));
    }

    #[test]
    fn quoting_covers_empty_and_trailing_backslashes() {
        let mut command = Command::new("program.exe");
        command.arg("").arg(r"C:\path with spaces\");
        let mut transfer = HandleTransfer::new(None);
        let line = decode(&build_command_line(&command, &mut transfer).unwrap());
        assert_eq!(line, r#""program.exe" "" "C:\path with spaces\\""#);
    }

    #[test]
    fn broker_command_line_names_the_resolved_program_and_encodes_like_a_spawn() {
        let mut command = Command::new("cmd");
        command
            .arg("")
            .arg(r"C:\path with spaces\")
            .arg("embedded\"quote")
            .raw_arg("/D /C");
        let line = broker_command_line(&command).unwrap();
        let program = PathBuf::from(sys::system_directory().unwrap()).join("cmd.exe");

        let mut transfer = HandleTransfer::new(None);
        let spawned = decode(&build_command_line(&command, &mut transfer).unwrap());
        let arguments = spawned.strip_prefix(r#""cmd""#).unwrap();
        assert_eq!(
            line,
            OsString::from(format!("\"{}\"{arguments}", program.display()))
        );
    }

    #[test]
    fn broker_command_line_makes_a_relative_program_absolute() {
        let command = Command::new(r"nested\tool.exe");
        let expected = env::current_dir().unwrap().join(r"nested\tool.exe");
        assert_eq!(
            broker_command_line(&command).unwrap(),
            OsString::from(format!("\"{}\"", expected.display()))
        );
    }

    #[test]
    fn broker_command_line_rejects_what_only_a_spawn_can_carry() {
        let file = File::open("NUL").unwrap();

        let mut handle_argument = Command::new("cmd.exe");
        handle_argument.arg_handle(&file).unwrap();
        let mut handle_path = Command::new("cmd.exe");
        handle_path.env_handle("Path", &file).unwrap();
        let mut batch = Command::new("script.cmd");
        batch.arg("value");
        for command in [handle_argument, handle_path, batch] {
            assert_eq!(
                broker_command_line(&command).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }

        let mut other_handle = Command::new("cmd.exe");
        other_handle
            .env_handle("WINDOWS_SPAWN_HANDLE", &file)
            .unwrap();
        assert!(broker_command_line(&other_handle).is_ok());

        let mut missing = Command::new("windows-spawn-missing-program");
        missing.env_clear();
        assert_eq!(
            broker_command_line(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn child_path_reads_path_exactly_as_the_environment_block_does() {
        let mut inherited = Command::new("cmd.exe");
        inherited.env("WINDOWS_SPAWN_OTHER", "value");
        let mut replaced = Command::new("cmd.exe");
        replaced.env("Path", "first").env("PATH", "second");
        let mut removed = Command::new("cmd.exe");
        removed.env("PATH", "value").env_remove("path");
        let mut cleared = Command::new("cmd.exe");
        cleared.env("PATH", "before").env_clear();
        let mut cleared_then_set = Command::new("cmd.exe");
        cleared_then_set.env_clear().env("pAtH", "after");
        let mut set_then_other_removed = Command::new("cmd.exe");
        set_then_other_removed
            .env("PATH", "kept")
            .env_remove("WINDOWS_SPAWN_OTHER");
        let file = File::open("NUL").unwrap();
        let mut handle_then_replaced = Command::new("cmd.exe");
        handle_then_replaced
            .env_handle("PATH", &file)
            .unwrap()
            .env("Path", "replaced");
        let mut handle_then_removed = Command::new("cmd.exe");
        handle_then_removed
            .env_handle("PATH", &file)
            .unwrap()
            .env_remove("path");

        assert_eq!(child_path(&Command::new("cmd.exe")).unwrap(), None);
        assert_eq!(
            child_path(&set_then_other_removed).unwrap(),
            Some(OsString::from("kept"))
        );
        assert_eq!(
            child_path(&handle_then_replaced).unwrap(),
            Some(OsString::from("replaced"))
        );
        for command in [
            inherited,
            replaced,
            removed,
            cleared,
            cleared_then_set,
            set_then_other_removed,
            handle_then_replaced,
            handle_then_removed,
        ] {
            let mut transfer = HandleTransfer::new(None);
            let environment = build_environment(&command, &mut transfer).unwrap();
            assert_eq!(child_path(&command).unwrap(), environment.path);
        }
    }

    #[test]
    fn executable_resolution_rejects_or_skips_nul_paths() {
        assert_eq!(
            resolve_executable(OsStr::new("dir\\a\0b"), None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let system = PathBuf::from(sys::system_directory().unwrap());
        assert_eq!(
            decode(&resolve_executable(OsStr::new("cmd"), Some(OsStr::new("a\0b"))).unwrap())
                .to_lowercase(),
            system.join("cmd.exe").to_string_lossy().to_lowercase()
        );
    }

    #[test]
    fn executable_resolution_searches_the_application_directory() {
        let mut directory = env::current_exe().unwrap();
        directory.pop();
        let name = format!("windows-spawn-app-dir-{}.exe", std::process::id());
        let path = directory.join(&name);
        File::create(&path).unwrap();
        let found = resolve_executable(OsStr::new(&name), None);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(decode(&found.unwrap()), path.to_string_lossy());
    }

    #[test]
    fn executable_resolution_keeps_other_extensions_and_missing_paths() {
        let system = PathBuf::from(sys::system_directory().unwrap());
        assert_eq!(
            decode(&resolve_executable(OsStr::new("more.com"), None).unwrap()).to_lowercase(),
            system.join("more.com").to_string_lossy().to_lowercase()
        );
        assert_eq!(
            decode(&resolve_executable(OsStr::new(r"nested\tool"), None).unwrap()),
            r"nested\tool"
        );
        let mut system_only = Command::new("cmd");
        system_only.env_clear();
        assert_eq!(
            broker_command_line(&system_only).unwrap(),
            OsString::from(format!("\"{}\"", system.join("cmd.exe").display()))
        );
    }

    #[test]
    fn executable_resolution_covers_explicit_child_path_and_not_found() {
        let system = PathBuf::from(sys::system_directory().unwrap());
        let executable = system.join("cmd.exe");
        assert_eq!(
            decode(&resolve_executable(executable.as_os_str(), None).unwrap()),
            executable.to_string_lossy()
        );
        let without_extension = system.join("cmd");
        assert_eq!(
            decode(&resolve_executable(without_extension.as_os_str(), None).unwrap()),
            executable.to_string_lossy()
        );
        assert!(decode(
            &resolve_executable(OsStr::new("cmd.exe"), Some(system.as_os_str())).unwrap()
        )
        .to_ascii_lowercase()
        .ends_with("cmd.exe"));

        let missing = format!("windows-spawn-missing-{}.exe", std::process::id());
        assert_eq!(
            resolve_executable(OsStr::new(&missing), Some(OsStr::new("")))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        let empty_path_probe = format!("windows-spawn-empty-path-{}.exe", std::process::id());
        let empty_path_probe_path = env::current_dir().unwrap().join(&empty_path_probe);
        let probe = File::create(&empty_path_probe_path).unwrap();
        assert_eq!(
            resolve_executable(OsStr::new(&empty_path_probe), Some(OsStr::new(";")))
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
    fn uncommitted_running_transaction_rolls_back() {
        let (gate_reader, gate_writer) = pipe();
        let (ready_reader, ready_writer) = pipe();
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/S", "/C"])
            .raw_arg("echo r&set /p x=&exit 77")
            .stdin(Stdio::from(gate_reader))
            .stdout(Stdio::from(ready_writer))
            .stderr(Stdio::null());
        let plan = SpawnPlan::new_running(
            &command,
            crate::SpawnOptions::new(),
            crate::plan::IoMode::Spawn,
        )
        .unwrap();
        let transaction = SpawnTransaction::new(&plan).unwrap();
        let created = transaction.created.as_ref().unwrap();
        let mut process = ProcessExitGuard::watch(created.process.as_handle());
        assert_eq!(
            suspend_count(created.thread.as_handle()),
            0,
            "a running spawn must not start suspended"
        );
        drop(plan);
        drop(command);

        let mut ready = [0_u8; 1];
        File::from(ready_reader).read_exact(&mut ready).unwrap();
        assert_eq!(&ready, b"r");
        drop(transaction);
        drop(gate_writer);
        assert_eq!(
            process.exit_code(),
            1,
            "rollback did not terminate its running process"
        );
    }

    #[test]
    fn uncommitted_suspended_transaction_rolls_back() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 77"]);
        let plan = SpawnPlan::new_suspended(
            &command,
            crate::SpawnOptions::new(),
            crate::plan::IoMode::Spawn,
        )
        .unwrap();
        let transaction = SpawnTransaction::new(&plan).unwrap();
        let created = transaction.created.as_ref().unwrap();
        let mut process = ProcessExitGuard::watch(created.process.as_handle());
        let thread = duplicate(created.thread.as_handle());
        drop(transaction);
        tempt_resume(thread.as_handle());
        assert_eq!(
            process.exit_code(),
            1,
            "rollback did not terminate its suspended process"
        );
    }

    #[test]
    fn suspended_child_drop_terminates_the_process() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit /b 77"]);
        let plan = SpawnPlan::new_suspended(
            &command,
            crate::SpawnOptions::new(),
            crate::plan::IoMode::Spawn,
        )
        .unwrap();
        let transaction = SpawnTransaction::new(&plan).unwrap();
        let created = transaction.created.as_ref().unwrap();
        let mut process = ProcessExitGuard::watch(created.process.as_handle());
        let thread = duplicate(created.thread.as_handle());
        drop(transaction.commit_suspended());
        tempt_resume(thread.as_handle());
        assert_eq!(
            process.exit_code(),
            1,
            "dropping SuspendedChild did not terminate the process"
        );
    }
}
