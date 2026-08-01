//! The spawn builder.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::attributes::{Job, ParentProcess, Pcon, RawHandleRef};
use crate::child::Child;
use crate::error::Result;
use crate::mitigation::MitigationPolicy;

/// What a child's standard handle should be connected to.
///
/// Note the interaction with
/// [`inherit_handles`](WindowsCommand::inherit_handles): when a handle list is
/// set, `Inherit` and `Handle` only work if those handles are *in the list*.
/// Windows does not fall back — a standard handle missing from the list arrives
/// in the child as an invalid handle, which is one of the classic
/// `STARTUPINFOEX` bugs.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub enum Stdio<'a> {
    /// Pass the parent's corresponding standard handle through.
    #[default]
    Inherit,
    /// Connect to `NUL`.
    Null,
    /// Create an anonymous pipe; the parent end is available from
    /// [`Child::take_stdin`] and friends.
    Piped,
    /// Connect to a specific handle — a file, a pipe end, a socket.
    Handle(RawHandleRef<'a>),
}

/// Builds a `CreateProcessW` call.
///
/// The lifetime `'a` is not decoration: it is the lifetime of every value the
/// eventual `PROC_THREAD_ATTRIBUTE_LIST` will point at. A `WindowsCommand<'a>`
/// therefore cannot outlive the handle slice, parent process, job or
/// pseudoconsole it was configured with, and the compiler rejects the
/// use-after-free that the C API happily performs.
///
/// The builder deliberately mirrors [`std::process::Command`] where the
/// semantics match, so that moving a call site over is mechanical. It is not a
/// drop-in replacement and does not try to be — see the non-goals in
/// `README.md`.
///
/// ```ignore
/// use spawnkit::{RawHandleRef, WindowsCommand};
///
/// let log = std::fs::File::create("child.log")?;
/// let inherited = [RawHandleRef::borrow(&log)];
///
/// let mut child = WindowsCommand::new("cargo")
///     .args(["build", "--release"])
///     .inherit_handles(&inherited)
///     .kill_tree_on_drop()
///     .spawn()?;
///
/// let status = child.wait()?;
/// ```
#[derive(Debug)]
pub struct WindowsCommand<'a> {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
    inherit_env: bool,
    current_dir: Option<PathBuf>,
    stdin: Stdio<'a>,
    stdout: Stdio<'a>,
    stderr: Stdio<'a>,
    inherited: Option<&'a [RawHandleRef<'a>]>,
    parent: Option<&'a ParentProcess>,
    mitigation: Option<MitigationPolicy>,
    job: Option<&'a Job>,
    pcon: Option<&'a Pcon>,
    suspended: bool,
    kill_tree_on_drop: bool,
}

impl<'a> WindowsCommand<'a> {
    /// Start building a spawn of `program`.
    ///
    /// `program` is passed to `CreateProcessW` as `lpApplicationName` when it
    /// looks like a path, so the notorious `lpCommandLine`-only search order
    /// (which will happily run `C:\Program.exe`) is avoided.
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        todo!("initialise the builder with defaults")
    }

    /// Append one argument.
    ///
    /// Arguments are quoted per the `CommandLineToArgvW` rules on the way into
    /// the single command-line string Windows actually takes.
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        todo!("push one argument")
    }

    /// Append several arguments.
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        todo!("push each argument")
    }

    /// Set one environment variable for the child.
    pub fn env<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        todo!("record the variable")
    }

    /// Set several environment variables.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        todo!("record each variable")
    }

    /// Start from an empty environment instead of the parent's.
    pub fn env_clear(&mut self) -> &mut Self {
        todo!("drop the inherited environment")
    }

    /// Set the child's working directory.
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        todo!("record the working directory")
    }

    /// Configure the child's standard input.
    pub fn stdin(&mut self, cfg: Stdio<'a>) -> &mut Self {
        todo!("record the stdin configuration")
    }

    /// Configure the child's standard output.
    pub fn stdout(&mut self, cfg: Stdio<'a>) -> &mut Self {
        todo!("record the stdout configuration")
    }

    /// Configure the child's standard error.
    pub fn stderr(&mut self, cfg: Stdio<'a>) -> &mut Self {
        todo!("record the stderr configuration")
    }

    /// Restrict inheritance to exactly these handles
    /// (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`).
    ///
    /// This is the whole reason the crate exists. Without it, spawning with
    /// inheritance enabled hands the child every inheritable handle in the
    /// process — including ones another thread opened a microsecond ago.
    ///
    /// The slice is borrowed until the spawn happens; that borrow is what `'a`
    /// tracks.
    pub fn inherit_handles(&mut self, handles: &'a [RawHandleRef<'a>]) -> &mut Self {
        todo!("record the handle list")
    }

    /// Re-parent the child (`PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`).
    pub fn parent_process(&mut self, parent: &'a ParentProcess) -> &mut Self {
        todo!("record the parent process")
    }

    /// Apply process creation mitigations
    /// (`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`).
    pub fn mitigation(&mut self, policy: MitigationPolicy) -> &mut Self {
        todo!("record the mitigation policy")
    }

    /// Create the child directly inside `job` (`PROC_THREAD_ATTRIBUTE_JOB_LIST`).
    ///
    /// Atomic with creation, unlike `AssignProcessToJobObject`. See
    /// `docs/adr/0004-job-attachment.md`.
    pub fn attach_to_job(&mut self, job: &'a Job) -> &mut Self {
        todo!("record the job")
    }

    /// Attach the child to a pseudoconsole
    /// (`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`).
    pub fn pseudoconsole(&mut self, pcon: &'a Pcon) -> &mut Self {
        todo!("record the pseudoconsole")
    }

    /// Create the child suspended (`CREATE_SUSPENDED`).
    ///
    /// The primary thread stays suspended until [`Child::resume`] is called, so
    /// the caller can inspect or modify the process first. Note that a
    /// suspended process is *not* a process that has run no code from the
    /// kernel's point of view — see `docs/adr/0004-job-attachment.md`.
    pub fn suspended(&mut self) -> &mut Self {
        todo!("set CREATE_SUSPENDED")
    }

    /// Kill the child, and everything it spawned, when the [`Child`] is
    /// dropped.
    ///
    /// Implemented with a job object owned by the `Child` and
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, not by walking the process tree:
    /// a PID-walking killer races against PID reuse and misses grandchildren
    /// that re-parented themselves.
    pub fn kill_tree_on_drop(&mut self) -> &mut Self {
        todo!("arrange a kill-on-close job for the child")
    }

    /// Spawn the process.
    ///
    /// Builds the attribute list, fills in `STARTUPINFOEXW`, calls
    /// `CreateProcessW`, and closes the thread handle unless the child was
    /// created suspended.
    pub fn spawn(&mut self) -> Result<Child> {
        todo!("build the attribute list and call CreateProcessW")
    }
}
