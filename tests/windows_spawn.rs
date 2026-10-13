#![cfg_attr(not(windows), allow(missing_docs))]
#![cfg(windows)]
#![allow(clippy::as_conversions, clippy::expect_used, clippy::unwrap_in_result)]

//! End-to-end Windows process creation tests.

#[cfg(windows_spawn_vectors)]
mod lifecycle;
mod support;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::mem::size_of_val;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use support::{
    local_duplicate, suspend_count, tempt_resume, ProbeCommand, ProcessExitGuard, Role, TempDir,
    EXIT_RELEASED, GRANDCHILD_MARKER, SELF_EXIT_CODES,
};

use windows_spawn::{
    AsPseudoConsole, BorrowedPseudoConsole, Command, ConsoleMode, Dep, Job, Mitigation,
    MitigationPolicy, Operation, ParentProcess, PriorityClass, SpawnError, SpawnOptions, Stdio,
    ValidationError,
};
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, GetFileType, ReadFile, WriteFile, BY_HANDLE_FILE_INFORMATION,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_UNKNOWN,
};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, GetConsoleCP, GetConsoleScreenBufferInfo,
    GetStdHandle, CONSOLE_SCREEN_BUFFER_INFO, COORD, HPCON, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, GetProcessId, GetProcessMitigationPolicy,
    GetThreadId, ProcessExtensionPointDisablePolicy, SuspendThread,
};

const PCON_ISOLATION_HELPER: &str = "WINDOWS_SPAWN_PCON_ISOLATION_HELPER";
const PCON_STDIO_PROBE: &str = "WINDOWS_SPAWN_PCON_STDIO_PROBE";
const PCON_STDIN_MARKER: &[u8] = b"windows-spawn-pcon-stdin";
const PCON_STDOUT_MARKER: &[u8] = b"windows-spawn-pcon-stdout";
const PCON_STDERR_MARKER: &[u8] = b"windows-spawn-pcon-stderr";
const CALLER_STDERR_HELPER: &str = "WINDOWS_SPAWN_CALLER_STDERR_HELPER";
const CALLER_STDERR_MARKER: &str = "windows-spawn-caller-stderr";
/// An unusual pseudoconsole size, so a probe can tell it is attached to the test's pseudoconsole.
const PCON_SIZE: COORD = COORD { X: 97, Y: 31 };

/// Returns true if this process's console has the test pseudoconsole's size.
fn attached_to_test_pseudoconsole() -> bool {
    let Ok(output) = File::options().read(true).write(true).open("CONOUT$") else {
        return false;
    };
    let mut information = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: `output` is an open console handle and `information` is writable.
    let queried =
        unsafe { GetConsoleScreenBufferInfo(output.as_raw_handle() as HANDLE, &mut information) };
    queried != 0 && information.dwSize.X == PCON_SIZE.X && information.dwSize.Y == PCON_SIZE.Y
}

fn cmd(script: &str) -> Command {
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/S", "/C"]).raw_arg(script);
    command
}

fn inheritable_duplicate<T: AsHandle>(source: &T) -> io::Result<OwnedHandle> {
    local_duplicate(source, true)
}

fn file_identity(handle: HANDLE) -> io::Result<(u32, u64)> {
    // SAFETY: the all-zero value is a valid output buffer initialization.
    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    // SAFETY: the handle is valid and the output buffer is writable for the call.
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        let index =
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
        Ok((information.dwVolumeSerialNumber, index))
    }
}

#[test]
fn probe() {
    support::run_probe_if_requested();
}

#[test]
fn args_environment_cwd_and_wait_cache_work() -> io::Result<()> {
    let directory = TempDir::new("cwd")?;

    let mut command = cmd("echo [%WINDOWS_SPAWN_VALUE%]&cd&exit /b 37");
    command
        .env("WINDOWS_SPAWN_VALUE", "hello world")
        .current_dir(directory.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let first = child.wait()?;
    let second = child.wait()?;
    assert_eq!(first, second);
    assert_eq!(first.code(), Some(37));
    Ok(())
}

#[test]
fn output_drains_stdout_and_stderr_beyond_pipe_capacity() -> io::Result<()> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$x='x'*131072; [Console]::Out.Write($x); [Console]::Error.Write($x); exit 23",
    ]);
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(23));
    assert_eq!(output.stdout.len(), 131_072);
    assert_eq!(output.stderr.len(), 131_072);
    Ok(())
}

#[test]
fn null_and_owned_file_stdio_work() -> io::Result<()> {
    let directory = TempDir::new("stdout")?;
    let path = directory.join("stdout");
    let file = File::create(&path)?;
    let mut command = cmd("echo file-output");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::null());
    assert!(command.status()?.success());
    let contents = fs::read_to_string(&path)?;
    assert!(contents.contains("file-output"));
    Ok(())
}

#[test]
fn suspended_child_resumes_once_into_normal_state() -> io::Result<()> {
    let (probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    let suspended = probe.spawn_suspended()?;
    let mut process = ProcessExitGuard::watch(&suspended)?;
    let thread = local_duplicate(&suspended.main_thread_handle(), false)?;
    let pid = suspended.id();
    let process_handle = suspended.as_handle().as_raw_handle();
    let thread_handle = suspended.main_thread_handle().as_raw_handle();
    // SAFETY: SuspendedChild owns both handles for these read-only queries.
    assert_eq!(unsafe { GetProcessId(process_handle) }, pid);
    // SAFETY: the main thread handle is valid until resume consumes it.
    assert_ne!(unsafe { GetThreadId(thread_handle) }, 0);

    let mut child = suspended.resume()?;
    assert_eq!(suspend_count(thread.as_handle())?, 0);
    assert_eq!(child.id(), pid);
    assert_eq!(child.as_handle().as_raw_handle(), process_handle);
    report.expect(b"r")?;
    assert!(child.try_wait()?.is_none());
    gate.open_with(19)?;
    assert_eq!(process.exit_code()?, 19);
    assert_eq!(child.try_wait()?.and_then(|status| status.code()), Some(19));
    Ok(())
}

#[test]
fn resume_rejects_an_externally_changed_suspend_count() -> io::Result<()> {
    let (probe, _gate, report) = ProbeCommand::new(Role::Ran)?;
    let suspended = probe.spawn_suspended()?;
    let mut process = ProcessExitGuard::watch(&suspended)?;
    let thread = local_duplicate(&suspended.main_thread_handle(), false)?;
    // SAFETY: the duplicate has THREAD_SUSPEND_RESUME access.
    assert_eq!(unsafe { SuspendThread(thread.as_raw_handle()) }, 1);

    let refused = suspended
        .resume()
        .expect_err("a suspend count of two is refused");
    assert!(matches!(
        &refused,
        SpawnError::Windows(error) if error.operation() == Operation::ResumeThread
    ));
    assert_eq!(refused.kind(), io::ErrorKind::InvalidData);
    assert!(refused.to_string().contains("suspend count was 2"));
    tempt_resume(thread.as_handle());
    assert_eq!(
        process.exit_code()?,
        1,
        "the rejected resume did not roll back"
    );
    assert!(report.rest()?.is_empty(), "the rolled-back process ran");
    Ok(())
}

#[test]
fn inherited_and_null_standard_handles_are_usable() -> io::Result<()> {
    let test_binary = std::env::current_exe()?;
    let mut inherited = Command::new(&test_binary);
    inherited
        .args(["--exact", "native_standard_handle_probe", "--nocapture"])
        .env("WINDOWS_SPAWN_STDIO_PROBE", "inherit");
    inherited.stdin(Stdio::null());
    assert!(inherited.status()?.success());

    let mut null = Command::new(test_binary);
    null.args(["--exact", "native_standard_handle_probe", "--nocapture"])
        .env("WINDOWS_SPAWN_STDIO_PROBE", "null");
    null.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    assert!(null.status()?.success());
    Ok(())
}

#[test]
fn native_standard_handle_probe() {
    let Ok(mode) = std::env::var("WINDOWS_SPAWN_STDIO_PROBE") else {
        return;
    };
    // SAFETY: this only reads the standard input slot.
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    // SAFETY: this only reads the standard output slot.
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    // SAFETY: this only reads the standard error slot.
    let error = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    let valid = |handle: HANDLE| {
        !handle.is_null()
            && handle != INVALID_HANDLE_VALUE
            // SAFETY: the query does not take ownership, and FILE_TYPE_UNKNOWN reports an invalid value.
            && unsafe { GetFileType(handle) } != FILE_TYPE_UNKNOWN
    };
    assert!(valid(output));
    assert!(valid(error));

    if mode == "null" {
        assert!(valid(input));
        let mut byte = [0_u8; 1];
        let mut read = 0_u32;
        // SAFETY: the buffers and byte counts are valid for synchronous I/O.
        let read_succeeded =
            unsafe { ReadFile(input, byte.as_mut_ptr(), 1, &mut read, std::ptr::null_mut()) };
        assert_ne!(read_succeeded, 0);
        assert_eq!(read, 0);
        let mut written = 0_u32;
        // SAFETY: the one-byte buffer and byte count are valid.
        let wrote_stdout =
            unsafe { WriteFile(output, byte.as_ptr(), 1, &mut written, std::ptr::null_mut()) };
        assert_ne!(wrote_stdout, 0);
        assert_eq!(written, 1);
        // SAFETY: the one-byte buffer and byte count are valid.
        let wrote_stderr =
            unsafe { WriteFile(error, byte.as_ptr(), 1, &mut written, std::ptr::null_mut()) };
        assert_ne!(wrote_stderr, 0);
        assert_eq!(written, 1);
    }
}

#[test]
fn direct_child_pipe_reads_reach_eof() -> io::Result<()> {
    let mut command = cmd("echo stdout-line&echo stderr-line 1>&2");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    assert!(child.wait()?.success());

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut buffer = [0_u8; 128];
    let stdout_length = stdout.read(&mut buffer)?;
    assert!(String::from_utf8_lossy(&buffer[..stdout_length]).contains("stdout-line"));
    assert_eq!(stdout.read(&mut buffer)?, 0);

    let stderr_length = stderr.read(&mut buffer)?;
    assert!(String::from_utf8_lossy(&buffer[..stderr_length]).contains("stderr-line"));
    assert_eq!(stderr.read(&mut buffer)?, 0);
    Ok(())
}

#[test]
fn explicit_job_attachment_runs_the_child() -> io::Result<()> {
    let outer_job = Job::create()?;
    let inner_job = Job::create()?;
    let mut ordinary = cmd("exit /b 0");
    let options = SpawnOptions::new().job(&outer_job).job(&inner_job);
    assert!(ordinary.status_with(options)?.success());
    Ok(())
}

/// The grandchild holds the stdout writer and blocks on a gate the test opens only afterwards, so returning at all proves it was terminated.
#[test]
fn kill_tree_output_terminates_a_descendant_holding_stdout() -> io::Result<()> {
    let (mut probe, mut gate, _report) = ProbeCommand::new(Role::TreeExit)?;
    probe
        .command_mut()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .own_process_tree();
    let output = probe.output_with(SpawnOptions::new())?;
    gate.release();
    assert!(output.status.success());
    assert!(output
        .stdout
        .windows(GRANDCHILD_MARKER.len())
        .any(|window| window == GRANDCHILD_MARKER));
    Ok(())
}

#[test]
fn requested_mitigation_is_visible_on_the_real_process() -> io::Result<()> {
    let (mut probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    probe
        .command_mut()
        .mitigation(MitigationPolicy::new().disable_extension_points(Mitigation::AlwaysOn));
    let mut child = probe.spawn_with(SpawnOptions::new())?;
    let mut process = ProcessExitGuard::watch(&child)?;
    report.expect(b"r")?;
    let mut flags = 0_u32;
    // SAFETY: the child's process handle is valid, and `flags` is writable DWORD storage as this query requires.
    let queried = unsafe {
        GetProcessMitigationPolicy(
            child.as_handle().as_raw_handle(),
            ProcessExtensionPointDisablePolicy,
            std::ptr::addr_of_mut!(flags).cast(),
            size_of_val(&flags),
        )
    };
    let query_error = io::Error::last_os_error();
    child.kill()?;
    gate.release();
    assert_eq!(process.exit_code()?, 1);
    assert_eq!(child.wait()?.code(), Some(1));
    if queried == 0 {
        return Err(query_error);
    }
    assert_eq!(flags & 1, 1);
    Ok(())
}

/// Returns the number of handles this process holds.
fn handle_count() -> io::Result<u32> {
    let mut count = 0;
    // SAFETY: the pseudo-handle is valid and `count` is writable DWORD storage.
    let success = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count)
    }
}

#[test]
fn failed_transactions_do_not_leak_handles() -> io::Result<()> {
    fn fail_after_acquisition(missing_directory: &Path) {
        let mut command = cmd("exit /b 0");
        command
            .current_dir(missing_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .own_process_tree();
        let error = command
            .spawn()
            .expect_err("a missing current directory must fail after resources are acquired");
        assert!(
            matches!(&error, SpawnError::Windows(failure) if failure.operation() == Operation::CreateProcess),
            "{error}"
        );
    }

    if !support::isolated("failed_transactions_do_not_leak_handles")? {
        return Ok(());
    }

    let directory = TempDir::new("missing-cwd")?;
    let missing_directory = directory.join("missing");
    fail_after_acquisition(&missing_directory);
    let before = handle_count()?;
    for _ in 0..64 {
        fail_after_acquisition(&missing_directory);
    }
    let after = handle_count()?;
    assert!(
        after <= before.saturating_add(8),
        "handle count grew from {before} to {after}"
    );
    Ok(())
}

#[test]
fn validation_happens_before_process_creation() {
    let mut batch = Command::new("script.CMD");
    let error = batch.spawn().unwrap_err();
    assert!(matches!(
        error,
        SpawnError::Validation(ValidationError::BatchFile)
    ));
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(io::Error::from(error).kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn argument_and_environment_handles_are_lowered_after_duplication() -> io::Result<()> {
    let directory = TempDir::new("handles")?;
    let argument_path = directory.join("argument");
    let environment_path = directory.join("environment");
    let argument_file = File::create(&argument_path)?;
    let environment_file = File::create(&environment_path)?;

    let script = concat!(
        "& { param($argumentHandle)",
        "$a=[IntPtr]::new([Int64]$argumentHandle);",
        "$as=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($a,$false);",
        "$af=[System.IO.FileStream]::new($as,[System.IO.FileAccess]::Write);",
        "$ab=[Text.Encoding]::UTF8.GetBytes('argument');$af.Write($ab,0,$ab.Length);$af.Flush();",
        "$e=[IntPtr]::new([Int64]$env:WINDOWS_SPAWN_HANDLE);",
        "$es=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($e,$false);",
        "$ef=[System.IO.FileStream]::new($es,[System.IO.FileAccess]::Write);",
        "$eb=[Text.Encoding]::UTF8.GetBytes('environment');$ef.Write($eb,0,$eb.Length);$ef.Flush()",
        "}"
    );
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    command.arg_handle(&argument_file)?;
    command.env_handle("WINDOWS_SPAWN_HANDLE", &environment_file)?;
    drop(argument_file);
    drop(environment_file);
    assert!(command.status()?.success());
    assert!(command.status()?.success());
    assert_eq!(
        fs::read_to_string(&argument_path)?,
        "argumentargument",
        "the reusable Command must re-transfer its private handle"
    );
    assert_eq!(
        fs::read_to_string(&environment_path)?,
        "environmentenvironment",
        "the reusable Command must re-transfer its private handle"
    );
    Ok(())
}

#[test]
fn alternate_parent_receives_remote_stdio_duplicates() -> io::Result<()> {
    let (host_probe, mut host_gate, mut host_report) = ProbeCommand::new(Role::Gate)?;
    let host = host_probe.spawn_with(SpawnOptions::new())?;
    let mut host_process = ProcessExitGuard::watch(&host)?;
    host_report.expect(b"r")?;
    let parent = ParentProcess::open(host.id())?;

    let mut command = cmd("echo alternate-parent");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = command.output_with(SpawnOptions::new().parent_process(&parent));
    host_gate.release();
    assert_eq!(host_process.exit_code()?, EXIT_RELEASED);

    let output = result?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("alternate-parent"));
    Ok(())
}

#[test]
fn alternate_parent_receives_remote_argument_and_environment_handles() -> io::Result<()> {
    let directory = TempDir::new("remote-handles")?;
    let argument_path = directory.join("argument");
    let environment_path = directory.join("environment");
    let argument_file = File::create(&argument_path)?;
    let environment_file = File::create(&environment_path)?;
    let (host_probe, mut host_gate, mut host_report) = ProbeCommand::new(Role::Gate)?;
    let host = host_probe.spawn_with(SpawnOptions::new())?;
    let mut host_process = ProcessExitGuard::watch(&host)?;
    host_report.expect(b"r")?;
    let parent = ParentProcess::open(host.id())?;

    let script = concat!(
        "& { param($argumentHandle)",
        "$a=[IntPtr]::new([Int64]$argumentHandle);",
        "$as=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($a,$false);",
        "$af=[System.IO.FileStream]::new($as,[System.IO.FileAccess]::Write);",
        "$ab=[Text.Encoding]::UTF8.GetBytes('argument');$af.Write($ab,0,$ab.Length);$af.Flush();",
        "$e=[IntPtr]::new([Int64]$env:WINDOWS_SPAWN_HANDLE);",
        "$es=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($e,$false);",
        "$ef=[System.IO.FileStream]::new($es,[System.IO.FileAccess]::Write);",
        "$eb=[Text.Encoding]::UTF8.GetBytes('environment');$ef.Write($eb,0,$eb.Length);$ef.Flush()",
        "}"
    );
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    command
        .arg_handle(&argument_file)?
        .env_handle("WINDOWS_SPAWN_HANDLE", &environment_file)?
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    drop((argument_file, environment_file));

    let result = command.status_with(SpawnOptions::new().parent_process(&parent));
    host_gate.release();
    assert_eq!(host_process.exit_code()?, EXIT_RELEASED);

    assert!(result?.success());
    assert_eq!(fs::read_to_string(&argument_path)?, "argument");
    assert_eq!(fs::read_to_string(&environment_path)?, "environment");
    Ok(())
}

#[test]
fn child_pipes_try_wait_and_cached_lifecycle_work() -> io::Result<()> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$line=[Console]::In.ReadLine(); [Console]::Out.Write('out:'+ $line); [Console]::Error.Write('err')",
    ]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    assert!(child.id() > 1);
    assert!(child.try_wait()?.is_none());
    let _ = child.as_handle();
    let mut stdin = child.stdin.take().expect("piped stdin");
    let _ = stdin.as_handle();
    stdin.write_all(b"hello\n")?;
    stdin.flush()?;
    drop(stdin);
    let _ = child.stdout.as_ref().expect("piped stdout").as_handle();
    let _ = child.stderr.as_ref().expect("piped stderr").as_handle();
    let piped = child.wait_with_output()?;
    assert!(piped.status.success());
    assert_eq!(piped.stdout, b"out:hello");
    assert_eq!(piped.stderr, b"err");

    let mut exited = cmd("exit /b 0").spawn()?;
    assert!(exited.wait()?.success());
    assert!(exited.try_wait()?.expect("cached exit").success());
    exited.kill()?;

    let (probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    let mut polled = probe.spawn_with(SpawnOptions::new())?;
    let mut process = ProcessExitGuard::watch(&polled)?;
    report.expect(b"r")?;
    assert!(polled.try_wait()?.is_none());
    gate.open_with(42)?;
    assert_eq!(process.exit_code()?, 42);
    assert_eq!(
        polled.try_wait()?.and_then(|status| status.code()),
        Some(42)
    );

    let mut silent = cmd("exit /b 0");
    silent.stdout(Stdio::null()).stderr(Stdio::null());
    let silenced = silent.spawn()?.wait_with_output()?;
    assert!(silenced.stdout.is_empty() && silenced.stderr.is_empty());
    Ok(())
}

/// A pseudoconsole whose output a detached thread drains from creation on.
struct TestPseudoConsole {
    value: HPCON,
    input_writer: Option<OwnedHandle>,
    output: mpsc::Receiver<Vec<u8>>,
}

impl TestPseudoConsole {
    fn create() -> io::Result<Self> {
        fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
            let mut read = std::ptr::null_mut();
            let mut write = std::ptr::null_mut();
            // SAFETY: both output pointers are writable; on success each handle is adopted once.
            if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: CreatePipe succeeded and returned two distinct handles.
            Ok(unsafe {
                (
                    OwnedHandle::from_raw_handle(read as RawHandle),
                    OwnedHandle::from_raw_handle(write as RawHandle),
                )
            })
        }

        let (input_reader, input_writer) = pipe()?;
        let (output_reader, output_writer) = pipe()?;
        let mut value = 0;
        // SAFETY: the pipe handles are valid for the call and `value` is writable; this type owns the HPCON.
        let result = unsafe {
            CreatePseudoConsole(
                PCON_SIZE,
                input_reader.as_raw_handle() as HANDLE,
                output_writer.as_raw_handle() as HANDLE,
                0,
                &mut value,
            )
        };
        if result < 0 {
            return Err(io::Error::other(format!(
                "CreatePseudoConsole failed with HRESULT {result:#x}"
            )));
        }
        drop((input_reader, output_writer));
        let (sender, output) = mpsc::channel();
        let mut output_reader = File::from(output_reader);
        drop(thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(read) = output_reader.read(&mut buffer) {
                if read == 0
                    || sender
                        .send(buffer.get(..read).unwrap_or_default().to_vec())
                        .is_err()
                {
                    break;
                }
            }
        }));
        Ok(Self {
            value,
            input_writer: Some(input_writer),
            output,
        })
    }

    fn write_input(&self, input: &[u8]) -> io::Result<()> {
        let writer = self
            .input_writer
            .as_ref()
            .expect("a live pseudoconsole retains its input writer");
        let mut written = 0_u32;
        let length = u32::try_from(input.len())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        // SAFETY: the buffer and byte count are valid for the synchronous write, and `self` owns the writer.
        if unsafe {
            WriteFile(
                writer.as_raw_handle(),
                input.as_ptr(),
                length,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if written != length {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "the pseudoconsole input write was incomplete",
            ));
        }
        Ok(())
    }

    /// Returns true once every marker has appeared in the output, or false if the output ends first.
    fn wait_for_output_markers(&self, expected: &[&[u8]]) -> bool {
        let mut received = Vec::new();
        loop {
            if expected.iter().all(|marker| contains(&received, marker)) {
                return true;
            }
            match self.output.recv() {
                Ok(chunk) => received.extend_from_slice(&chunk),
                Err(_) => return false,
            }
        }
    }
}

/// The close runs on a detached thread because some Windows Server 2022 builds block in it.
impl Drop for TestPseudoConsole {
    fn drop(&mut self) {
        drop(self.input_writer.take());
        let value = self.value;
        drop(thread::spawn(move || {
            // SAFETY: this type uniquely owns the HPCON.
            unsafe { ClosePseudoConsole(value) };
        }));
    }
}

impl AsPseudoConsole for TestPseudoConsole {
    fn as_pseudo_console(&self) -> BorrowedPseudoConsole<'_> {
        // SAFETY: TestPseudoConsole owns the HPCON until it drops, which the borrow of `self` outlives.
        unsafe { BorrowedPseudoConsole::borrow_raw(self.value) }
    }
}

#[test]
fn pseudoconsole_attribute_connects_the_child_console() -> io::Result<()> {
    let pseudoconsole = TestPseudoConsole::create()?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", "pseudoconsole_child_probe", "--nocapture"])
        .env("WINDOWS_SPAWN_PCON_PROBE", "1");
    let child = command.spawn_with(SpawnOptions::new().pseudo_console(&pseudoconsole))?;
    assert_eq!(ProcessExitGuard::watch(&child)?.exit_code()?, 0);
    assert!(pseudoconsole.wait_for_output_markers(&[b"windows-spawn-pcon-attached"]));
    Ok(())
}

/// The console's size proves the child is attached to the test pseudoconsole, so a missing attribute fails before any output wait.
#[test]
fn pseudoconsole_child_probe() {
    if std::env::var_os("WINDOWS_SPAWN_PCON_PROBE").is_none() {
        return;
    }
    // SAFETY: GetConsoleCP has no pointer preconditions.
    assert_ne!(unsafe { GetConsoleCP() }, 0);
    assert!(attached_to_test_pseudoconsole());
    let mut output = File::options().write(true).open("CONOUT$").unwrap();
    output.write_all(b"windows-spawn-pcon-attached").unwrap();
}

/// Streams the helper's output and stops the helper as soon as a pseudoconsole marker leaks into it.
#[test]
fn pseudoconsole_regular_stdio_stays_off_parent_pipes() -> io::Result<()> {
    let mut helper = Command::new(std::env::current_exe()?);
    helper
        .args([
            "--exact",
            "pseudoconsole_stdio_isolation_helper",
            "--nocapture",
        ])
        .env(PCON_ISOLATION_HELPER, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .own_process_tree();
    let mut child = helper.spawn()?;
    let (sender, events) = mpsc::channel();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    forward(stdout, 0, sender.clone());
    forward(stderr, 1, sender);

    let mut received = [Vec::new(), Vec::new()];
    for (stream, chunk) in events {
        received[stream].extend_from_slice(&chunk);
        let leaked = [PCON_STDOUT_MARKER, PCON_STDERR_MARKER]
            .iter()
            .any(|marker| contains(&received[stream], marker));
        assert!(
            !leaked,
            "ConPTY child output leaked to its parent's {}",
            ["stdout", "stderr"][stream]
        );
    }
    let status = child.wait()?;
    assert!(
        status.success(),
        "isolated ConPTY stdio helper failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&received[0]),
        String::from_utf8_lossy(&received[1])
    );
    Ok(())
}

/// Sends each chunk read from `stream` as `(index, chunk)` until EOF.
fn forward<R: Read + Send + 'static>(
    mut stream: R,
    index: usize,
    sender: mpsc::Sender<(usize, Vec<u8>)>,
) {
    drop(thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = stream.read(&mut buffer) {
            if read == 0
                || sender
                    .send((index, buffer.get(..read).unwrap_or_default().to_vec()))
                    .is_err()
            {
                break;
            }
        }
    }));
}

/// The helper's standard error is a pipe this test reads, so the marker arrives there only if the grandchild wrote its standard output into it.
#[test]
fn a_child_writes_its_output_to_the_callers_standard_error() -> io::Result<()> {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", "caller_stderr_helper", "--nocapture"])
        .env(CALLER_STDERR_HELPER, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    assert!(output.status.success());
    assert!(contains(&output.stderr, CALLER_STDERR_MARKER.as_bytes()));
    Ok(())
}

#[test]
fn caller_stderr_helper() -> io::Result<()> {
    if std::env::var_os(CALLER_STDERR_HELPER).is_none() {
        return Ok(());
    }
    let status = cmd(&format!("echo {CALLER_STDERR_MARKER}"))
        .stdin(Stdio::null())
        .stdout(io::stderr())
        .stderr(Stdio::null())
        .status()?;
    assert!(status.success());
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn pseudoconsole_stdio_isolation_helper() -> io::Result<()> {
    if std::env::var_os(PCON_ISOLATION_HELPER).is_none() {
        return Ok(());
    }

    let pseudoconsole = TestPseudoConsole::create()?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "pseudoconsole_regular_stdio_probe",
            "--nocapture",
        ])
        .env(PCON_STDIO_PROBE, "1");
    let child = command.spawn_with(SpawnOptions::new().pseudo_console(&pseudoconsole))?;
    let mut input = PCON_STDIN_MARKER.to_vec();
    input.extend_from_slice(b"\r\n");
    pseudoconsole.write_input(&input)?;
    assert_eq!(ProcessExitGuard::watch(&child)?.exit_code()?, 0);
    assert!(pseudoconsole.wait_for_output_markers(&[PCON_STDOUT_MARKER, PCON_STDERR_MARKER]));
    Ok(())
}

#[test]
fn pseudoconsole_regular_stdio_probe() -> io::Result<()> {
    if std::env::var_os(PCON_STDIO_PROBE).is_none() {
        return Ok(());
    }
    if !attached_to_test_pseudoconsole() {
        return Err(io::Error::other("not attached to the test pseudoconsole"));
    }

    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    if !line
        .as_bytes()
        .windows(PCON_STDIN_MARKER.len())
        .any(|window| window == PCON_STDIN_MARKER)
    {
        return Err(io::Error::other(
            "standard input did not arrive through ConPTY",
        ));
    }
    io::stdout().write_all(PCON_STDOUT_MARKER)?;
    io::stdout().flush()?;
    io::stderr().write_all(PCON_STDERR_MARKER)?;
    io::stderr().flush()
}

#[test]
fn capability_wrappers_and_all_option_builders_are_exercised() -> io::Result<()> {
    let borrowed_stdio = Stdio::from_borrowed(&File::open("NUL")?)?;
    assert!(format!("{borrowed_stdio:?}").contains("Owned"));
    assert!(format!("{:?}", Stdio::inherit()).contains("Inherit"));
    assert!(format!("{:?}", Stdio::null()).contains("Null"));
    assert!(format!("{:?}", Stdio::piped()).contains("Piped"));

    let parent = ParentProcess::open(std::process::id())?;
    let _ = parent.as_handle();
    assert!(format!("{parent:?}").contains("ParentProcess"));
    let job = Job::create()?;
    job.set_kill_on_close(true)?;
    job.set_kill_on_close(false)?;
    let duplicate_job = job.duplicate()?;
    let _ = duplicate_job.as_handle();
    assert!(format!("{duplicate_job:?}").contains("Job"));

    // SAFETY: only this validation test borrows the value, and validation rejects every spawn that names it before it reaches Win32.
    let pseudoconsole = unsafe { BorrowedPseudoConsole::borrow_raw(1) };
    let options = SpawnOptions::new()
        .job(&job)
        .parent_process(&parent)
        .pseudo_console(pseudoconsole);
    assert!(format!("{options:?}").contains("BorrowedPseudoConsole"));

    let mut invalid = cmd("exit /b 0");
    invalid
        .mitigation(MitigationPolicy::new().dep(Dep::Enable))
        .priority(PriorityClass::BelowNormal)
        .new_process_group()
        .inherit_parent_affinity()
        .breakaway_from_job()
        .preserve_code_authorization()
        .default_error_mode()
        .own_process_tree()
        .stdout(Stdio::null());
    let rendered = format!("{invalid:#?}");
    assert!(rendered.contains("BelowNormal") && rendered.contains("owns_tree: true"));
    assert!(matches!(
        invalid.spawn_with(options.clone()),
        Err(SpawnError::Validation(
            ValidationError::PseudoConsoleWithStdio
        ))
    ));
    invalid.console(ConsoleMode::NoWindow);
    assert!(matches!(
        invalid.spawn_with(options.clone()),
        Err(SpawnError::Validation(
            ValidationError::PseudoConsoleWithConsoleMode
        ))
    ));
    let without_console = SpawnOptions::new().parent_process(&parent);
    assert!(matches!(
        invalid.spawn_with(without_console),
        Err(SpawnError::Validation(
            ValidationError::AlternateParentNeedsStdio
        ))
    ));

    let assigned_job = Job::create()?;
    let (probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    let mut child = probe.spawn_with(SpawnOptions::new())?;
    let mut process = ProcessExitGuard::watch(&child)?;
    report.expect(b"r")?;
    assigned_job.assign(&child)?;
    assigned_job.terminate(31)?;
    gate.release();
    assert_eq!(process.exit_code()?, 31);
    assert_eq!(child.wait()?.code(), Some(31));
    Ok(())
}

#[test]
fn handle_list_excludes_an_unlisted_inheritable_handle() -> io::Result<()> {
    const PROBE: &str = "WINDOWS_SPAWN_EXCLUDED_HANDLE_PROBE";
    if let Some(value) = std::env::var_os(PROBE) {
        let value = value.to_string_lossy();
        let mut fields = value.split(':');
        let handle = fields
            .next()
            .expect("probe handle is present")
            .parse::<isize>()
            .expect("probe handle must be an integer");
        let volume = fields
            .next()
            .expect("probe volume is present")
            .parse::<u32>()
            .expect("probe volume must be an integer");
        let index = fields
            .next()
            .expect("probe file index is present")
            .parse::<u64>()
            .expect("probe file index must be an integer");
        assert!(fields.next().is_none(), "probe has exactly three fields");
        if let Ok(identity) = file_identity(handle as HANDLE) {
            assert_ne!(
                identity,
                (volume, index),
                "unlisted inheritable file handle reached the child"
            );
        }
        return Ok(());
    }

    let directory = TempDir::new("excluded-handle")?;
    let excluded_path = directory.join("excluded");
    let listed_file = File::open("NUL")?;
    let excluded_file = File::create(&excluded_path)?;
    let excluded = inheritable_duplicate(&excluded_file)?;
    let excluded_value = excluded.as_handle().as_raw_handle() as isize;
    let (excluded_volume, excluded_index) = file_identity(excluded.as_handle().as_raw_handle())?;
    let probe = format!("{excluded_value}:{excluded_volume}:{excluded_index}");

    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "handle_list_excludes_an_unlisted_inheritable_handle",
            "--test-threads=1",
        ])
        .env(PROBE, probe);
    command.arg_handle(&listed_file)?;
    drop(listed_file);
    let status = command.status()?;
    assert!(status.success());

    drop((excluded_file, excluded));
    Ok(())
}

#[test]
fn dropping_a_tree_owner_terminates_the_tree() -> io::Result<()> {
    let (mut probe, mut gate, mut report) = ProbeCommand::new(Role::TreeHold)?;
    probe.command_mut().own_process_tree();
    let child = probe.spawn_with(SpawnOptions::new())?;
    let mut root = ProcessExitGuard::watch(&child)?;
    report.expect(b"rr")?;
    drop(child);
    gate.release();
    assert!(
        report.rest()?.is_empty(),
        "a process in the tree outlived the drop"
    );
    assert!(!SELF_EXIT_CODES.contains(&root.exit_code()?));
    Ok(())
}

#[test]
fn killing_a_tree_owner_ends_every_process_in_the_tree() -> io::Result<()> {
    let (mut probe, mut gate, mut report) = ProbeCommand::new(Role::TreeHold)?;
    probe.command_mut().own_process_tree();
    let mut child = probe.spawn_with(SpawnOptions::new())?;
    let mut root = ProcessExitGuard::watch(&child)?;
    report.expect(b"rr")?;
    child.kill()?;
    assert_eq!(child.wait_tree()?.code(), Some(1));
    gate.release();
    assert!(
        report.rest()?.is_empty(),
        "a process in the tree outlived the kill"
    );
    assert_eq!(root.exit_code()?, 1);
    child.kill()?;
    Ok(())
}

/// Both probes hold a file that refuses deletion while any handle to it is open, so deleting it right after `wait_tree` shows that no process of the tree still held it.
#[test]
fn a_killed_tree_releases_its_files_once_wait_tree_returns() -> io::Result<()> {
    let directory = TempDir::new("wait-tree")?;
    let held = directory.join("held");
    let file = File::options()
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&held)?;
    let (mut probe, mut gate, mut report) = ProbeCommand::new(Role::TreeHold)?;
    probe.command_mut().stdout(file).own_process_tree();
    let mut child = probe.spawn_with(SpawnOptions::new())?;
    report.expect(b"rr")?;
    child.kill()?;
    assert_eq!(child.wait_tree()?.code(), Some(1));
    fs::remove_file(&held)?;
    gate.release();
    assert!(
        report.rest()?.is_empty(),
        "a process in the tree outlived the kill"
    );
    Ok(())
}

#[test]
fn dropping_a_detached_child_leaves_it_running() -> io::Result<()> {
    let (probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    let child = probe.spawn_with(SpawnOptions::new())?;
    let mut process = ProcessExitGuard::watch(&child)?;
    report.expect(b"r")?;
    drop(child);
    gate.release();
    assert_eq!(process.exit_code()?, EXIT_RELEASED);
    Ok(())
}

#[test]
fn public_job_kill_on_close_terminates_assigned_process() -> io::Result<()> {
    let job = Job::create()?;
    job.set_kill_on_close(true)?;
    let (probe, mut gate, mut report) = ProbeCommand::new(Role::Gate)?;
    let child = probe.spawn_with(SpawnOptions::new())?;
    let mut process = ProcessExitGuard::watch(&child)?;
    report.expect(b"r")?;
    job.assign(&child)?;
    drop(job);
    gate.release();
    assert!(
        report.rest()?.is_empty(),
        "the assigned process outlived the Job"
    );
    assert!(!SELF_EXIT_CODES.contains(&process.exit_code()?));
    Ok(())
}

#[test]
fn reusable_command_environment_and_accessors_work() -> io::Result<()> {
    let directory = TempDir::new("builder-cwd")?;
    let mut command = cmd("echo %WINDOWS_SPAWN_ONE%-%WINDOWS_SPAWN_TWO%");
    command
        .envs([("WINDOWS_SPAWN_ONE", "one"), ("WINDOWS_SPAWN_TWO", "old")])
        .env_remove("windows_spawn_two")
        .env("WINDOWS_SPAWN_TWO", "two")
        .current_dir(directory.path());
    assert_eq!(command.get_program(), "cmd.exe");
    assert_eq!(command.get_current_dir(), Some(directory.path()));
    let modified = command.output()?;
    assert!(String::from_utf8_lossy(&modified.stdout).contains("one-two"));

    command.env_clear().env("WINDOWS_SPAWN_ONE", "clear");
    let cleared = command.output()?;
    assert!(String::from_utf8_lossy(&cleared.stdout).contains("clear-"));
    Ok(())
}

/// A way a spawn can go, which [`spawn_cleanup_stress`] takes in turn.
#[derive(Clone, Copy, Debug)]
enum StressMode {
    Wait,
    Resume,
    DropSuspended,
    Status,
    Output,
    Input,
    KillTree,
    TreeOutput,
    Handles,
    Job,
    Parent,
    FailedCreation,
    FailedSearch,
}

impl StressMode {
    const ALL: [Self; 13] = [
        Self::Wait,
        Self::Resume,
        Self::DropSuspended,
        Self::Status,
        Self::Output,
        Self::Input,
        Self::KillTree,
        Self::TreeOutput,
        Self::Handles,
        Self::Job,
        Self::Parent,
        Self::FailedCreation,
        Self::FailedSearch,
    ];
}

/// The exit code of the stress children, which no termination gives.
const STRESS_EXIT: i32 = 3;

/// Requires `status` to carry the stress children's exit code.
fn stress_exit(status: std::process::ExitStatus) -> io::Result<()> {
    if status.code() == Some(STRESS_EXIT) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "exited with {status:?}, not {STRESS_EXIT}"
        )))
    }
}

/// Requires `spawned` to fail in `operation`.
fn stress_failure<T>(spawned: windows_spawn::Result<T>, operation: Operation) -> io::Result<()> {
    match spawned {
        Err(SpawnError::Windows(failure)) if failure.operation() == operation => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "failed with {error}, not in {operation}"
        ))),
        Ok(_) => Err(io::Error::other(format!(
            "succeeded, not failed in {operation}"
        ))),
    }
}

/// What the stress spawns borrow, created before the count starts.
struct Stress {
    nul: File,
    job: Job,
    parent: ParentProcess,
    directory: TempDir,
}

impl Stress {
    fn new() -> io::Result<Self> {
        Ok(Self {
            nul: File::options().read(true).write(true).open("NUL")?,
            job: Job::create()?,
            parent: ParentProcess::open(std::process::id())?,
            directory: TempDir::new("stress")?,
        })
    }

    fn run(&self, mode: StressMode) -> io::Result<()> {
        let mut command = cmd("exit /b 3");
        match mode {
            StressMode::Wait => stress_exit(command.spawn()?.wait()?),
            StressMode::Resume => stress_exit(command.spawn_suspended()?.resume()?.wait()?),
            StressMode::DropSuspended => {
                drop(command.spawn_suspended()?);
                Ok(())
            }
            StressMode::Status => stress_exit(command.status()?),
            StressMode::Output => {
                let output = cmd("echo out& echo err 1>&2").output()?;
                if output.stdout.starts_with(b"out") && output.stderr.starts_with(b"err") {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("read {output:?}")))
                }
            }
            StressMode::Input => {
                let mut sort = cmd("sort");
                let mut child = sort.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
                let mut input = child
                    .stdin
                    .take()
                    .ok_or_else(|| io::Error::other("stdin is piped"))?;
                input.write_all(b"typed\r\n")?;
                drop(input);
                let output = child.wait_with_output()?;
                if output.stdout.starts_with(b"typed") {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("read {output:?}")))
                }
            }
            StressMode::KillTree => {
                let mut waiting = cmd("set /p x=");
                let mut child = waiting.stdin(Stdio::piped()).own_process_tree().spawn()?;
                child.kill()?;
                match child.wait_tree()?.code() {
                    Some(1) => Ok(()),
                    code => Err(io::Error::other(format!(
                        "a killed tree exited with {code:?}"
                    ))),
                }
            }
            StressMode::TreeOutput => {
                let output = cmd("echo tree").own_process_tree().output()?;
                if output.stdout.starts_with(b"tree") {
                    Ok(())
                } else {
                    Err(io::Error::other(format!("read {output:?}")))
                }
            }
            StressMode::Handles => {
                command
                    .arg_handle(&self.nul)?
                    .env_handle("WINDOWS_SPAWN_HANDLE", &self.nul)?;
                stress_exit(command.status()?)
            }
            StressMode::Job => {
                stress_exit(command.status_with(SpawnOptions::new().job(&self.job))?)
            }
            StressMode::Parent => {
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                stress_exit(command.status_with(SpawnOptions::new().parent_process(&self.parent))?)
            }
            StressMode::FailedCreation => {
                command
                    .current_dir(self.directory.join("missing"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .own_process_tree();
                stress_failure(command.spawn(), Operation::CreateProcess)
            }
            StressMode::FailedSearch => {
                let mut missing = Command::new("windows-spawn-missing-program");
                missing
                    .env_clear()
                    .stdout(Stdio::piped())
                    .own_process_tree();
                stress_failure(missing.spawn(), Operation::ResolveExecutable)
            }
        }
    }
}

/// Spawns children in every [`StressMode`], `WINDOWS_SPAWN_STRESS_CASES` times in all, and requires this process's handle count not to grow.
///
/// Each mode runs twice before the count starts, so what the system creates once, such as a thread-pool worker, is already there.
/// It runs only on request, and alone, because the count belongs to the process:
/// `cargo test --test windows_spawn spawn_cleanup_stress -- --ignored --exact`.
#[test]
#[ignore = "set WINDOWS_SPAWN_STRESS_CASES, and run it alone"]
fn spawn_cleanup_stress() -> io::Result<()> {
    let cases: usize = std::env::var("WINDOWS_SPAWN_STRESS_CASES")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let stress = Stress::new()?;
    for mode in StressMode::ALL.into_iter().chain(StressMode::ALL) {
        stress.run(mode)?;
    }
    let before = handle_count()?;
    for (case, mode) in StressMode::ALL.into_iter().cycle().take(cases).enumerate() {
        stress
            .run(mode)
            .map_err(|error| io::Error::other(format!("case {case}, {mode:?}: {error}")))?;
    }
    let after = handle_count()?;
    assert!(
        after <= before,
        "the handle count grew from {before} to {after} over {cases} spawns"
    );
    println!(
        "{cases} spawns in {} modes: the handle count went from {before} to {after}",
        StressMode::ALL.len()
    );
    Ok(())
}

#[test]
fn dropping_suspended_child_prevents_execution() -> io::Result<()> {
    let (probe, _gate, report) = ProbeCommand::new(Role::Ran)?;
    let suspended = probe.spawn_suspended()?;
    let mut process = ProcessExitGuard::watch(&suspended)?;
    let thread = local_duplicate(&suspended.main_thread_handle(), false)?;
    drop(suspended);
    tempt_resume(thread.as_handle());
    assert_eq!(
        process.exit_code()?,
        1,
        "dropping SuspendedChild must terminate the process"
    );
    assert!(report.rest()?.is_empty(), "the suspended process ran");
    Ok(())
}
