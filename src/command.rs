//! Reusable process-launch intent.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::windows::io::AsHandle;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use crate::child::{Child, SuspendedChild};
use crate::domain::creation_flag_rules as flags;
use crate::error::{Operation, Phase, Result, SpawnError};
use crate::handles::Stdio;
use crate::mitigation::MitigationPolicy;
use crate::options::{ConsoleMode, Creation, PriorityClass, SpawnOptions};
use crate::plan::{IoMode, SpawnPlan};
use crate::resource::{Borrowed, Foreign, Owned};
use crate::sys;
use crate::transaction::SpawnTransaction;

#[derive(Debug)]
pub(crate) enum Arg {
    Text(OsString),
    Raw(OsString),
    Handle(Owned<Foreign>),
}

#[derive(Debug)]
pub(crate) enum EnvOp {
    Set(OsString, EnvValue),
    Remove(OsString),
}

#[derive(Debug)]
pub(crate) enum EnvValue {
    Text(OsString),
    Handle(Owned<Foreign>),
}

/// A reusable description of a Windows process launch, with the API of [`std::process::Command`].
///
/// Beyond std, it owns the choices of `CreateProcessW` that need no borrowed capability:
/// [`Self::own_process_tree`], [`Self::console`], [`Self::priority`], [`Self::mitigation`], and the creation-flag setters.
/// Capabilities that one spawn borrows (Jobs, an alternate parent, a pseudoconsole) go in [`SpawnOptions`],
/// which the `*_with` methods take.
///
/// Handles given to [`Self::arg_handle`] and [`Self::env_handle`] are duplicated privately when configured.
/// Each spawn duplicates them into the effective parent and only then lowers their values to decimal text.
///
/// # Examples
///
/// Capture a tool's output, and end every process it starts once it exits:
///
/// ```
/// use windows_spawn::Command;
///
/// let output = Command::new("cmd.exe")
///     .args(["/D", "/C", "echo hello"])
///     .own_process_tree()
///     .output()?;
///
/// assert!(output.status.success());
/// assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
/// # Ok::<(), windows_spawn::SpawnError>(())
/// ```
pub struct Command {
    pub(crate) program: OsString,
    pub(crate) args: Vec<Arg>,
    pub(crate) env_clear: bool,
    pub(crate) env_ops: Vec<EnvOp>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) stdin: Option<Stdio>,
    pub(crate) stdout: Option<Stdio>,
    pub(crate) stderr: Option<Stdio>,
    pub(crate) creation: Creation,
}

/// Prints the program and its arguments as [`std::process::Command`] does: `"cmd.exe" "/D" echo hi`.
///
/// A raw argument prints as written, and a handle argument as `<handle>`.
/// The alternate form, `{:#?}`, prints every setting.
impl fmt::Debug for Command {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if formatter.alternate() {
            return formatter
                .debug_struct("Command")
                .field("program", &self.program)
                .field("args", &self.args)
                .field("env_clear", &self.env_clear)
                .field("env_ops", &self.env_ops)
                .field("cwd", &self.cwd)
                .field("stdin", &self.stdin)
                .field("stdout", &self.stdout)
                .field("stderr", &self.stderr)
                .field("creation", &self.creation)
                .finish();
        }
        write!(formatter, "{:?}", self.program)?;
        for arg in &self.args {
            match arg {
                Arg::Text(text) => write!(formatter, " {text:?}")?,
                Arg::Raw(text) => write!(formatter, " {}", text.to_string_lossy())?,
                Arg::Handle(_) => formatter.write_str(" <handle>")?,
            }
        }
        Ok(())
    }
}

impl Command {
    /// Creates a command that runs `program`.
    #[must_use]
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            env_clear: false,
            env_ops: Vec::new(),
            cwd: None,
            stdin: None,
            stdout: None,
            stderr: None,
            creation: Creation::new(),
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
    /// The text follows one space and is neither quoted nor escaped.
    pub fn raw_arg<S: AsRef<OsStr>>(&mut self, text: S) -> &mut Self {
        self.args.push(Arg::Raw(text.as_ref().to_os_string()));
        self
    }

    /// Appends a handle argument whose child-table value is lowered at spawn.
    ///
    /// # Errors
    ///
    /// Returns a Windows error if the source handle cannot be duplicated.
    pub fn arg_handle<T: AsHandle>(&mut self, handle: &T) -> Result<&mut Self> {
        self.args.push(Arg::Handle(private_duplicate(handle)?));
        Ok(self)
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
    /// Returns a Windows error if the source handle cannot be duplicated.
    pub fn env_handle<K: AsRef<OsStr>, T: AsHandle>(
        &mut self,
        key: K,
        handle: &T,
    ) -> Result<&mut Self> {
        self.env_ops.push(EnvOp::Set(
            key.as_ref().to_os_string(),
            EnvValue::Handle(private_duplicate(handle)?),
        ));
        Ok(self)
    }

    /// Removes an environment variable case-insensitively.
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.env_ops
            .push(EnvOp::Remove(key.as_ref().to_os_string()));
        self
    }

    /// Clears the inherited environment and prior recorded modifications.
    pub fn env_clear(&mut self) -> &mut Self {
        self.env_clear = true;
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

    /// Makes the returned [`Child`] own the child's whole process tree: the child and every process it starts.
    ///
    /// The spawn puts the child in a private Job, innermost after any Jobs of [`SpawnOptions::job`], and the `Child` owns that Job:
    ///
    /// - [`Child::kill`] terminates every process in the tree, not only the child.
    /// - Dropping the `Child`, or the exit of the process that holds it, terminates every process still in the tree.
    /// - [`Child::wait_with_output`] and [`Self::output`] terminate the descendants once the child exits,
    ///   so a descendant that inherited an output pipe can't keep the call from returning.
    /// - [`Child::wait_tree`] waits until every process in the tree has exited.
    ///
    /// The Job allows no breakaway, so a descendant that asks for `CREATE_BREAKAWAY_FROM_JOB` fails to start.
    /// A daemon that a build starts and expects to outlive it, such as `mspdbsrv.exe` or an `sccache` server, ends with the tree too.
    /// A [`SuspendedChild`] already runs in the Job, and dropping it terminates the child before it runs.
    pub fn own_process_tree(&mut self) -> &mut Self {
        self.creation.owns_tree = true;
        self
    }

    /// Chooses the child's console.
    ///
    /// A pseudoconsole from [`SpawnOptions::pseudo_console`] is the child's console, so it combines only with [`ConsoleMode::Inherit`].
    pub fn console(&mut self, mode: ConsoleMode) -> &mut Self {
        self.creation.console = mode;
        self
    }

    /// Chooses the child's scheduling priority class.
    ///
    /// Without a choice, the child gets the normal class, or the caller's when that is idle or below normal.
    pub fn priority(&mut self, class: PriorityClass) -> &mut Self {
        self.creation.set_priority(class);
        self
    }

    /// Applies a process-creation mitigation policy: `PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`.
    pub fn mitigation(&mut self, policy: MitigationPolicy) -> &mut Self {
        self.creation.mitigation = policy;
        self
    }

    /// Makes the child the root of a new process group, which `GenerateConsoleCtrlEvent` can signal: `CREATE_NEW_PROCESS_GROUP`.
    pub fn new_process_group(&mut self) -> &mut Self {
        self.creation.set(flags::NEW_PROCESS_GROUP);
        self
    }

    /// Gives the child its logical parent's processor affinity: `INHERIT_PARENT_AFFINITY`.
    pub fn inherit_parent_affinity(&mut self) -> &mut Self {
        self.creation.set(flags::INHERIT_PARENT_AFFINITY);
        self
    }

    /// Lets the child leave the caller's Job where that Job allows it: `CREATE_BREAKAWAY_FROM_JOB`.
    pub fn breakaway_from_job(&mut self) -> &mut Self {
        self.creation.set(flags::BREAKAWAY_FROM_JOB);
        self
    }

    /// Keeps the caller's code-authorization level for the child: `CREATE_PRESERVE_CODE_AUTHZ_LEVEL`.
    pub fn preserve_code_authorization(&mut self) -> &mut Self {
        self.creation.set(flags::PRESERVE_CODE_AUTHZ_LEVEL);
        self
    }

    /// Starts the child with the default hard-error mode instead of the caller's: `CREATE_DEFAULT_ERROR_MODE`.
    pub fn default_error_mode(&mut self) -> &mut Self {
        self.creation.set(flags::DEFAULT_ERROR_MODE);
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

    /// Renders the command line for a launcher that accepts only a command line, such as WMI `Win32_Process.Create`.
    ///
    /// The first token is the quoted absolute path of the executable [`Self::spawn`] would run.
    /// Arguments are encoded as for a spawn.
    /// The environment, working directory, standard I/O, creation choices, and [`SpawnOptions`] are not represented.
    ///
    /// # Examples
    ///
    /// ```
    /// use windows_spawn::Command;
    ///
    /// let mut command = Command::new("cmd.exe");
    /// command.args(["/D", "/C", "echo two words"]);
    ///
    /// let line = command.to_command_line()?;
    /// let line = line.to_string_lossy();
    /// assert!(line.starts_with('"'));
    /// assert!(line.ends_with(r#"\cmd.exe" /D /C "echo two words""#));
    /// # Ok::<(), windows_spawn::SpawnError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a validation error for input a spawn rejects, for a handle argument, and for a `PATH` set from a handle.
    /// Returns a Windows error of kind [`std::io::ErrorKind::NotFound`] if the program is not found.
    pub fn to_command_line(&self) -> Result<OsString> {
        crate::transaction::broker_command_line(self)
    }

    /// Starts the process, as [`std::process::Command::spawn`] does.
    ///
    /// Standard streams the command doesn't configure are inherited.
    ///
    /// # Errors
    ///
    /// Returns a validation error for rejected input, or the Windows error of the step that failed;
    /// a program the search didn't find is [`std::io::ErrorKind::NotFound`].
    pub fn spawn(&mut self) -> Result<Child> {
        self.spawn_with(SpawnOptions::new())
    }

    /// Starts the process with the capabilities that `options` lends to this spawn.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`], and a validation error for options that conflict with the command.
    pub fn spawn_with(&mut self, options: SpawnOptions<'_>) -> Result<Child> {
        let plan = SpawnPlan::new_running(self, options, IoMode::Spawn)?;
        SpawnTransaction::new(&plan)?.commit_child()
    }

    /// Creates the process with its main thread still suspended, for inspection before it runs any code.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`].
    pub fn spawn_suspended(&mut self) -> Result<SuspendedChild> {
        self.spawn_suspended_with(SpawnOptions::new())
    }

    /// Creates the process suspended, with the capabilities that `options` lends to this spawn.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn_with`].
    pub fn spawn_suspended_with(&mut self, options: SpawnOptions<'_>) -> Result<SuspendedChild> {
        let plan = SpawnPlan::new_suspended(self, options, IoMode::Spawn)?;
        Ok(SpawnTransaction::new(&plan)?.commit_suspended())
    }

    /// Runs the process to completion and returns its status, as [`std::process::Command::status`] does.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`], or the Windows error of waiting for the process.
    pub fn status(&mut self) -> Result<ExitStatus> {
        self.status_with(SpawnOptions::new())
    }

    /// Runs the process to completion with the capabilities that `options` lends, and returns its status.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn_with`], or the Windows error of waiting for the process.
    pub fn status_with(&mut self, options: SpawnOptions<'_>) -> Result<ExitStatus> {
        self.spawn_with(options)?.wait()
    }

    /// Runs the process to completion and collects its output, as [`std::process::Command::output`] does.
    ///
    /// Standard input defaults to the null device, and standard output and error to pipes.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`], or the Windows error of waiting, reading the pipes, or ending the owned tree.
    pub fn output(&mut self) -> Result<Output> {
        self.output_with(SpawnOptions::new())
    }

    /// Runs the process to completion with the capabilities that `options` lends, and collects its output.
    ///
    /// # Errors
    ///
    /// As [`Self::output`], and a validation error for options that conflict with the command.
    pub fn output_with(&mut self, options: SpawnOptions<'_>) -> Result<Output> {
        let plan = SpawnPlan::new_running(self, options, IoMode::Output)?;
        SpawnTransaction::new(&plan)?
            .commit_child()?
            .wait_with_output()
    }
}

/// A private, non-inheritable duplicate that the command owns until a spawn lowers it.
fn private_duplicate<T: AsHandle>(handle: &T) -> Result<Owned<Foreign>> {
    sys::duplicate_local(Borrowed::foreign(handle), false).map_err(|error| {
        SpawnError::windows(Phase::Preparation, Operation::DuplicateLocalHandle, error)
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;
    use crate::error::ValidationError;
    use crate::sys::test_support::is_inheritable;

    #[test]
    fn validation_errors_propagate_from_every_entry_point() {
        let mut batch = Command::new("script.cmd");
        for result in [
            batch.spawn().map(drop),
            batch.spawn_with(SpawnOptions::new()).map(drop),
            batch.spawn_suspended().map(drop),
            batch.spawn_suspended_with(SpawnOptions::new()).map(drop),
            batch.status().map(drop),
            batch.status_with(SpawnOptions::new()).map(drop),
            batch.output().map(drop),
            batch.output_with(SpawnOptions::new()).map(drop),
        ] {
            assert!(matches!(
                result,
                Err(SpawnError::Validation(ValidationError::BatchFile))
            ));
        }
    }

    #[test]
    fn stored_handle_duplicates_are_private() -> std::io::Result<()> {
        let nul = File::open("NUL")?;
        let mut command = Command::new("cmd.exe");
        command
            .arg_handle(&nul)?
            .env_handle("WINDOWS_SPAWN_HANDLE", &nul)?;
        let Some(Arg::Handle(argument)) = command.args.first() else {
            panic!("arg_handle stores a handle argument");
        };
        assert!(!is_inheritable(argument.as_handle()));
        let Some(EnvOp::Set(_, EnvValue::Handle(value))) = command.env_ops.first() else {
            panic!("env_handle stores a handle value");
        };
        assert!(!is_inheritable(value.as_handle()));
        Ok(())
    }
}
