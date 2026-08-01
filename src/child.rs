//! The spawned process handle.

use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle, RawHandle};

use crate::attributes::Job;
use crate::error::Result;

/// A process created by [`WindowsCommand::spawn`](crate::WindowsCommand::spawn).
///
/// Holding a `Child` holds an open process handle, which keeps the process's
/// exit code and PID valid even after the process has exited — so unlike a bare
/// PID, a `Child` cannot be aimed at the wrong process by PID reuse.
#[derive(Debug)]
pub struct Child {
    process: OwnedHandle,
    /// Kept only when the child was created suspended; dropped by
    /// [`Child::resume`].
    main_thread: Option<OwnedHandle>,
    pid: u32,
    /// Present when
    /// [`kill_tree_on_drop`](crate::WindowsCommand::kill_tree_on_drop) was
    /// requested: dropping this job kills the whole tree.
    job: Option<Job>,
    stdin: Option<OwnedHandle>,
    stdout: Option<OwnedHandle>,
    stderr: Option<OwnedHandle>,
}

impl Child {
    /// The process id.
    pub fn id(&self) -> u32 {
        todo!("return the pid captured at creation")
    }

    /// Wait for the process to exit.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        todo!("WaitForSingleObject + GetExitCodeProcess")
    }

    /// Check whether the process has exited, without blocking.
    ///
    /// Returns `Ok(None)` while it is still running.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        todo!("WaitForSingleObject with a zero timeout")
    }

    /// Terminate the process.
    ///
    /// This is `TerminateProcess`: the child gets no chance to clean up, and
    /// its own children are unaffected unless it was placed in a job. For a
    /// tree kill, use
    /// [`kill_tree_on_drop`](crate::WindowsCommand::kill_tree_on_drop) or
    /// [`Job::kill_on_close`].
    pub fn kill(&mut self) -> Result<()> {
        todo!("TerminateProcess(self.process, 1)")
    }

    /// Resume a child created with
    /// [`suspended`](crate::WindowsCommand::suspended).
    ///
    /// A no-op if the child was not created suspended.
    pub fn resume(&mut self) -> Result<()> {
        todo!("ResumeThread on the stored main thread handle")
    }

    /// Take the parent end of the child's stdin pipe, if
    /// [`Stdio::Piped`](crate::Stdio::Piped) was used.
    pub fn take_stdin(&mut self) -> Option<OwnedHandle> {
        todo!("take the pipe end")
    }

    /// Take the parent end of the child's stdout pipe, if
    /// [`Stdio::Piped`](crate::Stdio::Piped) was used.
    pub fn take_stdout(&mut self) -> Option<OwnedHandle> {
        todo!("take the pipe end")
    }

    /// Take the parent end of the child's stderr pipe, if
    /// [`Stdio::Piped`](crate::Stdio::Piped) was used.
    pub fn take_stderr(&mut self) -> Option<OwnedHandle> {
        todo!("take the pipe end")
    }

    /// Give up ownership of the process handle.
    ///
    /// The caller becomes responsible for `CloseHandle`. Any kill-on-drop job
    /// is dropped with the `Child`, so the tree-kill guarantee does *not*
    /// survive this call — the doc comment says so because the alternative is
    /// a very confusing bug report.
    pub fn into_raw_handle(self) -> RawHandle {
        todo!("consume self and return the raw process handle")
    }
}

impl AsHandle for Child {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        todo!("borrow the process handle")
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // TODO(sys): if `job` is present it is dropped here, and
        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE terminates the tree. Handles are
        // closed by `OwnedHandle`. Nothing to do explicitly, and deliberately
        // no `todo!()`: destructors must not panic.
    }
}

/// How a process exited.
///
/// Windows exit codes are `DWORD`s, so this exposes a `u32` rather than
/// std's `Option<i32>`: there is no "killed by signal" case to model, and
/// reinterpreting `0xC0000005` as a negative `i32` helps nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExitStatus(u32);

impl ExitStatus {
    /// Whether the process exited with code 0.
    pub fn success(self) -> bool {
        todo!("return self.0 == 0")
    }

    /// The raw exit code.
    ///
    /// Values above `0xC0000000` are usually `NTSTATUS` codes from an unhandled
    /// exception rather than something the program chose to return.
    pub fn code(self) -> u32 {
        todo!("return the exit code")
    }
}
