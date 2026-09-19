use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use crate::backend::WindowsBackend;
use crate::child::{Child, SuspendedChild};
use crate::error::{Error, Operation, Phase, Result};
use crate::handles::Stdio;
use crate::options::SpawnOptions;
use crate::plan::IoMode;
use crate::sys;
use crate::trace::{self, ResourceKind};
use crate::transaction::{
    output_with_backend, spawn_running_with_backend, spawn_suspended_with_backend,
};

fn duplicate_command_handle_with(
    source: BorrowedHandle<'_>,
    duplicate: fn(BorrowedHandle<'_>) -> io::Result<OwnedHandle>,
) -> Result<OwnedHandle> {
    trace::io(
        Phase::Preparation,
        Operation::DuplicateLocalHandle,
        ResourceKind::Handle,
        || duplicate(source),
    )
    .map_err(|error| Error::windows(Phase::Preparation, Operation::DuplicateLocalHandle, error))
}

#[derive(Debug)]
pub(crate) enum Arg {
    Text(OsString),
    Raw(OsString),
    Handle(OwnedHandle),
}

#[derive(Debug)]
pub(crate) enum EnvOp {
    Set(OsString, EnvValue),
    Remove(OsString),
}

#[derive(Debug)]
pub(crate) enum EnvValue {
    Text(OsString),
    Handle(OwnedHandle),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentBase {
    Inherit,
    Empty,
}

/// A reusable description of a Windows process launch.
///
/// Handles embedded by [`Self::arg_handle`] and [`Self::env_handle`] are
/// privately duplicated when configured. Each spawn duplicates those handles
/// again into the actual parent process and only then lowers their numeric
/// values to decimal text.
///
/// # Examples
///
/// Run a command to completion and capture what it wrote, terminating any
/// descendants it leaves behind:
///
/// ```
/// use windows_spawn::{Command, JobClosePolicy, SpawnOptions};
///
/// // `.bat` and `.cmd` are rejected, so a shell boundary is always explicit.
/// let shell = std::env::var_os("COMSPEC").expect("COMSPEC is set on Windows");
/// let mut command = Command::new(shell);
/// command.args(["/D", "/S", "/C"]).raw_arg("echo hello");
///
/// let output = command.output_with(
///     SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses),
/// )?;
///
/// assert!(output.status.success());
/// assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
/// # Ok::<(), windows_spawn::Error>(())
/// ```
#[derive(Debug)]
pub struct Command {
    pub(crate) program: OsString,
    pub(crate) args: Vec<Arg>,
    pub(crate) environment_base: EnvironmentBase,
    pub(crate) env_ops: Vec<EnvOp>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) stdin: Option<Stdio>,
    pub(crate) stdout: Option<Stdio>,
    pub(crate) stderr: Option<Stdio>,
}

impl Command {
    /// Creates a command which will execute `program`.
    #[must_use]
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            environment_base: EnvironmentBase::Inherit,
            env_ops: Vec::new(),
            cwd: None,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Appends a normally quoted argument.
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.args.push(Arg::Text(arg.as_ref().to_os_string()));
        self
    }

    /// Appends multiple normally quoted arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    /// Appends text verbatim to the Windows command line.
    ///
    /// The text is separated from the preceding element by one space but is
    /// otherwise neither quoted nor escaped.
    pub fn raw_arg<S: AsRef<OsStr>>(&mut self, text: S) -> &mut Self {
        self.args.push(Arg::Raw(text.as_ref().to_os_string()));
        self
    }

    /// Appends a handle argument whose child-table value is lowered at spawn.
    ///
    /// # Errors
    ///
    /// Returns an error if the source handle cannot be duplicated.
    pub fn arg_handle<T: AsHandle>(&mut self, handle: &T) -> Result<&mut Self> {
        self.arg_handle_with(handle, |source| {
            sys::duplicate_local(source, sys::Inheritability::Private)
        })
    }

    fn arg_handle_with<T: AsHandle>(
        &mut self,
        handle: &T,
        duplicate: fn(BorrowedHandle<'_>) -> io::Result<OwnedHandle>,
    ) -> Result<&mut Self> {
        duplicate_command_handle_with(handle.as_handle(), duplicate).map(|handle| {
            self.args.push(Arg::Handle(handle));
            self
        })
    }

    /// Sets one environment variable.
    pub fn env<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.env_ops.push(EnvOp::Set(
            key.as_ref().to_os_string(),
            EnvValue::Text(value.as_ref().to_os_string()),
        ));
        self
    }

    /// Sets multiple environment variables.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (key, value) in vars {
            self.env(key, value);
        }
        self
    }

    /// Sets an environment variable to a handle's child-table numeric value.
    ///
    /// # Errors
    ///
    /// Returns an error if the source handle cannot be duplicated.
    pub fn env_handle<K: AsRef<OsStr>, T: AsHandle>(
        &mut self,
        key: K,
        handle: &T,
    ) -> Result<&mut Self> {
        self.env_handle_with(key, handle, |source| {
            sys::duplicate_local(source, sys::Inheritability::Private)
        })
    }

    fn env_handle_with<K: AsRef<OsStr>, T: AsHandle>(
        &mut self,
        key: K,
        handle: &T,
        duplicate: fn(BorrowedHandle<'_>) -> io::Result<OwnedHandle>,
    ) -> Result<&mut Self> {
        duplicate_command_handle_with(handle.as_handle(), duplicate).map(|handle| {
            self.env_ops.push(EnvOp::Set(
                key.as_ref().to_os_string(),
                EnvValue::Handle(handle),
            ));
            self
        })
    }

    /// Removes an environment variable case-insensitively.
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.env_ops
            .push(EnvOp::Remove(key.as_ref().to_os_string()));
        self
    }

    /// Clears the inherited environment and prior recorded modifications.
    pub fn env_clear(&mut self) -> &mut Self {
        self.environment_base = EnvironmentBase::Empty;
        self.env_ops.clear();
        self
    }

    /// Sets the child working directory.
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Configures standard input.
    pub fn stdin<T: Into<Stdio>>(&mut self, stdio: T) -> &mut Self {
        self.stdin = Some(stdio.into());
        self
    }

    /// Configures standard output.
    pub fn stdout<T: Into<Stdio>>(&mut self, stdio: T) -> &mut Self {
        self.stdout = Some(stdio.into());
        self
    }

    /// Configures standard error.
    pub fn stderr<T: Into<Stdio>>(&mut self, stdio: T) -> &mut Self {
        self.stderr = Some(stdio.into());
        self
    }

    /// Returns the originally configured program.
    #[must_use]
    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    /// Returns the configured working directory.
    #[must_use]
    pub fn get_current_dir(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Spawns with default options.
    ///
    /// # Errors
    ///
    /// Returns validation, resource-acquisition, or process-creation errors.
    pub fn spawn(&mut self) -> Result<Child> {
        self.spawn_with(SpawnOptions::new())
    }

    /// Spawns using one operation's borrowed capabilities and policy.
    ///
    /// # Errors
    ///
    /// Returns validation, resource-acquisition, or process-creation errors.
    pub fn spawn_with(&mut self, options: SpawnOptions<'_>) -> Result<Child> {
        spawn_running_with_backend::<WindowsBackend>(self, options, IoMode::Spawn)
    }

    /// Spawns in the suspended type state with default options.
    ///
    /// # Errors
    ///
    /// Returns validation, resource-acquisition, or process-creation errors.
    pub fn spawn_suspended(&mut self) -> Result<SuspendedChild> {
        self.spawn_suspended_with(SpawnOptions::new())
    }

    /// Spawns in the suspended type state using explicit options.
    ///
    /// # Errors
    ///
    /// Returns validation, resource-acquisition, or process-creation errors.
    pub fn spawn_suspended_with(&mut self, options: SpawnOptions<'_>) -> Result<SuspendedChild> {
        spawn_suspended_with_backend::<WindowsBackend>(self, options)
    }

    /// Runs the process and waits for its status using default options.
    ///
    /// # Errors
    ///
    /// Returns an error from spawning, waiting, or retrieving the exit code.
    pub fn status(&mut self) -> Result<ExitStatus> {
        self.status_with(SpawnOptions::new())
    }

    /// Runs the process and waits for its status using explicit options.
    ///
    /// # Errors
    ///
    /// Returns an error from spawning, waiting, or retrieving the exit code.
    pub fn status_with(&mut self, options: SpawnOptions<'_>) -> Result<ExitStatus> {
        self.spawn_with(options).and_then(|mut child| child.wait())
    }

    /// Runs the process and captures output using default options.
    ///
    /// # Errors
    ///
    /// Returns an error from spawning, waiting, reading, or Job termination.
    pub fn output(&mut self) -> Result<Output> {
        self.output_with(SpawnOptions::new())
    }

    /// Runs the process and captures output using explicit options.
    ///
    /// # Errors
    ///
    /// Returns an error from spawning, waiting, reading, or Job termination.
    pub fn output_with(&mut self, options: SpawnOptions<'_>) -> Result<Output> {
        output_with_backend::<WindowsBackend>(self, options)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn bulk_environment_builder_records_each_pair() {
        let mut command = Command::new("program.exe");
        command.envs([("A", "1"), ("B", "2")]);
        assert_eq!(command.env_ops.len(), 2);
    }

    #[test]
    fn duplicate_failures_keep_the_typed_operation() {
        let file = File::open("NUL").unwrap();
        let error = duplicate_command_handle_with(file.as_handle(), |_| {
            Err(io::Error::from_raw_os_error(5))
        })
        .unwrap_err();
        let Error::Windows(error) = error else {
            panic!("Windows error expected");
        };
        assert_eq!(error.operation(), Operation::DuplicateLocalHandle);

        let mut command = Command::new("program.exe");
        assert!(command
            .arg_handle_with(&file, |_| Err(io::Error::from_raw_os_error(5)))
            .is_err());
        assert!(command
            .env_handle_with("HANDLE", &file, |_| Err(io::Error::from_raw_os_error(5)))
            .is_err());
        command
            .arg_handle_with(&file, |source| {
                sys::duplicate_local(source, sys::Inheritability::Private)
            })
            .unwrap();
        command
            .env_handle_with("HANDLE", &file, |source| {
                sys::duplicate_local(source, sys::Inheritability::Private)
            })
            .unwrap();

        let mut invalid = Command::new("");
        assert!(invalid.status_with(SpawnOptions::new()).is_err());
        let mut valid = Command::new("cmd.exe");
        valid.args(["/D", "/C", "exit /b 0"]);
        assert!(valid.status_with(SpawnOptions::new()).unwrap().success());
    }
}
