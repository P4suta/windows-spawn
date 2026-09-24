//! Event-based helpers: every wait ends on a process exit, a pipe byte, or a pipe EOF.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::windows::io::{
    AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle,
};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_spawn::{Child, Command, SpawnOptions, Stdio, SuspendedChild};
use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_PIPE};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, ResumeThread, SuspendThread, TerminateProcess,
    WaitForSingleObject, INFINITE,
};

/// Exit code of a probe released by gate EOF.
pub(crate) const EXIT_RELEASED: u32 = 0x0057_0001;
/// Exit code of a `Ran` probe, which must never run.
pub(crate) const EXIT_RAN: u32 = 0x0057_0002;
/// Exit code of a probe whose handoff failed before it reported ready.
pub(crate) const EXIT_SETUP_FAILED: u32 = 0x0057_0003;
/// Exit codes a probe can produce by itself.
pub(crate) const SELF_EXIT_CODES: [u32; 3] = [EXIT_RELEASED, EXIT_RAN, EXIT_SETUP_FAILED];
/// Written to standard output by an announcing probe before it reports ready.
pub(crate) const GRANDCHILD_MARKER: &[u8] = b"windows-spawn-grandchild";

const ROLE: &str = "WINDOWS_SPAWN_PROBE_ROLE";
const GATE: &str = "WINDOWS_SPAWN_PROBE_GATE";
const REPORT: &str = "WINDOWS_SPAWN_PROBE_REPORT";
const ANNOUNCE: &str = "WINDOWS_SPAWN_PROBE_ANNOUNCE";

/// Returns a non-inheritable pipe as (reader, writer).
pub(crate) fn pipe() -> io::Result<(File, File)> {
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    // SAFETY: both output pointers are writable, and null security attributes make both handles non-inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreatePipe succeeded, so both handles are new and distinct.
    let (read, write) = unsafe {
        (
            OwnedHandle::from_raw_handle(read as RawHandle),
            OwnedHandle::from_raw_handle(write as RawHandle),
        )
    };
    Ok((File::from(read), File::from(write)))
}

/// Returns a non-inheritable duplicate with the same access.
pub(crate) fn local_duplicate<T: AsHandle>(
    source: &T,
    inheritable: bool,
) -> io::Result<OwnedHandle> {
    let mut duplicate = ptr::null_mut();
    // SAFETY: both pseudo-handles are valid, the source is borrowed for the call, and `duplicate` is writable.
    let success = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source.as_handle().as_raw_handle(),
            GetCurrentProcess(),
            &mut duplicate,
            0,
            i32::from(inheritable),
            DUPLICATE_SAME_ACCESS,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle returned a new, uniquely owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(duplicate as RawHandle) })
}

/// The test's end of a probe's gate pipe.
///
/// A probe blocks on the gate after reporting ready; dropping or releasing the gate lets it exit with [`EXIT_RELEASED`].
pub(crate) struct Gate(Option<File>);

impl Gate {
    /// Makes one probe exit with `code`.
    pub(crate) fn open_with(&mut self, code: u8) -> io::Result<()> {
        let mut writer = self.0.take().expect("the gate is still closed");
        writer.write_all(&[code])
    }

    /// Closes the gate; every probe still blocked on it sees EOF.
    pub(crate) fn release(&mut self) {
        drop(self.0.take());
    }
}

/// The test's end of a probe's report pipe.
///
/// Probes write `r` when ready and `x` when they see gate EOF.
pub(crate) struct Report(File);

impl Report {
    /// Reads exactly `expected`; EOF means a probe ended before reporting.
    pub(crate) fn expect(&mut self, expected: &[u8]) -> io::Result<()> {
        let mut received = vec![0_u8; expected.len()];
        self.0.read_exact(&mut received)?;
        assert_eq!(received, expected, "unexpected probe report");
        Ok(())
    }

    /// Reads until every report writer has closed.
    pub(crate) fn rest(mut self) -> io::Result<Vec<u8>> {
        let mut rest = Vec::new();
        self.0.read_to_end(&mut rest)?;
        Ok(rest)
    }
}

/// What a probe does.
#[derive(Clone, Copy)]
pub(crate) enum Role {
    /// Reports ready, then waits on the gate.
    Gate,
    /// Must never run: reports `!` and exits with [`EXIT_RAN`].
    Ran,
    /// Starts a `Gate` grandchild on the same gate and report, then acts as `Gate`.
    TreeHold,
    /// Starts an announcing `Gate` grandchild that inherits standard output, waits until it is ready, and exits 0.
    TreeExit,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Gate => "gate",
            Self::Ran => "ran",
            Self::TreeHold => "tree-hold",
            Self::TreeExit => "tree-exit",
        }
    }
}

/// A launch of this test binary as a probe.
///
/// Spawning consumes the value, so the command's private handle copies close before the test waits for EOF.
pub(crate) struct ProbeCommand {
    command: Command,
}

impl ProbeCommand {
    /// Creates a probe launch with null standard I/O.
    pub(crate) fn new(role: Role) -> io::Result<(Self, Gate, Report)> {
        let (gate_reader, gate_writer) = pipe()?;
        let (report_reader, report_writer) = pipe()?;
        let command = probe_command(role, &gate_reader, &report_writer)?;
        Ok((
            Self { command },
            Gate(Some(gate_writer)),
            Report(report_reader),
        ))
    }

    pub(crate) fn command_mut(&mut self) -> &mut Command {
        &mut self.command
    }

    pub(crate) fn spawn_with(mut self, options: SpawnOptions<'_>) -> io::Result<Child> {
        self.command.spawn_with(options)
    }

    pub(crate) fn spawn_suspended(mut self) -> io::Result<SuspendedChild> {
        self.command.spawn_suspended()
    }

    pub(crate) fn output_with(mut self, options: SpawnOptions<'_>) -> io::Result<Output> {
        self.command.output_with(options)
    }
}

fn probe_command(role: Role, gate: &File, report: &File) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "probe", "--nocapture"])
        .env(ROLE, role.name())
        .env_remove(ANNOUNCE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.env_handle(GATE, gate)?.env_handle(REPORT, report)?;
    Ok(command)
}

/// Runs the probe role selected by the environment; returns when there is none.
pub(crate) fn run_probe_if_requested() {
    let Some(role) = std::env::var_os(ROLE) else {
        return;
    };
    let code = match run_role(&role) {
        Ok(code) => code,
        Err(_) => EXIT_SETUP_FAILED,
    };
    std::process::exit(i32::from_ne_bytes(code.to_ne_bytes()));
}

fn run_role(role: &OsString) -> io::Result<u32> {
    let role = role.to_str().unwrap_or_default();
    if role == "ran" {
        let mut report = adopt_pipe(REPORT)?;
        let _ = report.write_all(b"!");
        return Ok(EXIT_RAN);
    }
    let gate = adopt_pipe(GATE)?;
    let mut report = adopt_pipe(REPORT)?;
    match role {
        "gate" => {
            if std::env::var_os(ANNOUNCE).is_some() {
                let mut stdout = io::stdout();
                stdout.write_all(GRANDCHILD_MARKER)?;
                stdout.flush()?;
            }
        }
        "tree-hold" => {
            let mut grandchild = probe_command(Role::Gate, &gate, &report)?;
            drop(grandchild.spawn()?);
        }
        "tree-exit" => {
            let (mut ready_reader, ready_writer) = pipe()?;
            let mut grandchild = probe_command(Role::Gate, &gate, &ready_writer)?;
            grandchild
                .env(ANNOUNCE, "1")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            drop(grandchild.spawn()?);
            drop(ready_writer);
            let mut ready = [0_u8; 1];
            ready_reader.read_exact(&mut ready)?;
            return Ok(0);
        }
        _ => return Ok(EXIT_SETUP_FAILED),
    }
    report.write_all(b"r")?;
    Ok(wait_on_gate(gate, report))
}

/// Blocks on the gate: a byte is the exit code, EOF reports `x` and releases.
fn wait_on_gate(mut gate: File, mut report: File) -> u32 {
    let mut byte = [0_u8; 1];
    match gate.read(&mut byte) {
        Ok(1) => u32::from(byte[0]),
        Ok(_) => {
            let _ = report.write_all(b"x");
            EXIT_RELEASED
        }
        Err(_) => loop {
            std::thread::park();
        },
    }
}

fn adopt_pipe(variable: &str) -> io::Result<File> {
    let value: isize = std::env::var(variable)
        .map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    // SAFETY: the parent listed this inheritable handle in PROC_THREAD_ATTRIBUTE_HANDLE_LIST and published its value only in `variable`.
    // This process adopts it exactly once.
    let handle = unsafe { OwnedHandle::from_raw_handle(value as RawHandle) };
    // SAFETY: the owned handle is valid for the query.
    if unsafe { GetFileType(handle.as_raw_handle()) } != FILE_TYPE_PIPE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a pipe"));
    }
    Ok(File::from(handle))
}

/// Owns a duplicated process handle and terminates the process on drop unless it was observed to exit.
///
/// It calls Win32 directly so mutants of the crate cannot disable cleanup.
pub(crate) struct ProcessExitGuard {
    process: OwnedHandle,
    armed: bool,
}

impl ProcessExitGuard {
    pub(crate) fn watch<T: AsHandle>(process: &T) -> io::Result<Self> {
        Ok(Self {
            process: local_duplicate(process, false)?,
            armed: true,
        })
    }

    /// Waits for exit and returns the exit code.
    pub(crate) fn exit_code(&mut self) -> io::Result<u32> {
        // SAFETY: the owned process handle is valid for the wait.
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        self.armed = false;
        let mut code = 0_u32;
        // SAFETY: the owned process handle is valid and `code` is writable.
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
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

/// Resumes a thread once, ignoring the result.
///
/// Run after a termination under test: a terminated thread never runs again, while a surviving one runs and exits with its own code.
pub(crate) fn tempt_resume(thread: BorrowedHandle<'_>) {
    // SAFETY: the thread handle is valid for the call.
    let _ = unsafe { ResumeThread(thread.as_raw_handle()) };
}

/// Returns a thread's suspend count, leaving it unchanged.
pub(crate) fn suspend_count(thread: BorrowedHandle<'_>) -> io::Result<u32> {
    // SAFETY: the thread handle is valid and has THREAD_SUSPEND_RESUME access.
    let previous = unsafe { SuspendThread(thread.as_raw_handle()) };
    if previous == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the same handle is valid; this undoes the suspension above.
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(previous)
}

/// A new directory under the temporary directory, removed on drop.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let parent = std::env::temp_dir();
        loop {
            let index = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "windows-spawn-{label}-{}-{index}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
