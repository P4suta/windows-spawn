use std::fmt;

use windows_sys::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_DEFAULT_ERROR_MODE, CREATE_NEW_CONSOLE,
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_PRESERVE_CODE_AUTHZ_LEVEL, DETACHED_PROCESS,
    INHERIT_PARENT_AFFINITY,
};

use crate::handles::{AsPseudoConsole, BorrowedPseudoConsole, Job, ParentProcess};
use crate::mitigation::MitigationPolicy;

/// The action taken when a live [`crate::Child`] releases its private Job.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum JobClosePolicy {
    /// Preserve the child and any descendants after owned handles are closed.
    #[default]
    PreserveProcesses,
    /// Terminate the child and descendants when the private Job is closed.
    TerminateProcesses,
}

/// A mutually exclusive Windows console-creation mode.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ConsoleMode {
    /// Inherit the caller's console relationship.
    #[default]
    Inherit,
    /// Start without inheriting a console.
    Detached,
    /// Allocate a new console.
    NewConsole,
    /// Run a console application without a console window.
    NoWindow,
}

impl ConsoleMode {
    pub(crate) const fn creation_bits(self) -> u32 {
        match self {
            Self::Inherit => 0,
            Self::Detached => DETACHED_PROCESS,
            Self::NewConsole => CREATE_NEW_CONSOLE,
            Self::NoWindow => CREATE_NO_WINDOW,
        }
    }
}

/// The source of the child's terminal connection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerminalMode<'a> {
    /// Use an ordinary Windows console mode.
    Console(ConsoleMode),
    /// Connect the child to a borrowed pseudoconsole.
    PseudoConsole(BorrowedPseudoConsole<'a>),
}

impl Default for TerminalMode<'_> {
    fn default() -> Self {
        Self::Console(ConsoleMode::Inherit)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct CreationPolicy(u32);

impl CreationPolicy {
    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    const fn with(self, flag: u32) -> Self {
        Self(self.0 | flag)
    }
}

/// Capabilities and closed creation policies used by one spawn operation.
pub struct SpawnOptions<'a> {
    pub(crate) jobs: Vec<&'a Job>,
    pub(crate) parent: Option<&'a ParentProcess>,
    pub(crate) mitigation: MitigationPolicy,
    pub(crate) terminal: TerminalMode<'a>,
    pub(crate) creation: CreationPolicy,
    pub(crate) job_close: JobClosePolicy,
}

impl fmt::Debug for SpawnOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpawnOptions")
            .field("jobs", &self.jobs)
            .field("parent", &self.parent)
            .field("mitigation", &self.mitigation)
            .field("terminal", &self.terminal)
            .field("creation", &self.creation)
            .field("job_close", &self.job_close)
            .finish()
    }
}

impl Default for SpawnOptions<'_> {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            parent: None,
            mitigation: MitigationPolicy::new(),
            terminal: TerminalMode::default(),
            creation: CreationPolicy::default(),
            job_close: JobClosePolicy::PreserveProcesses,
        }
    }
}

impl<'a> SpawnOptions<'a> {
    /// Creates default per-spawn options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a Job in root-to-innermost assignment order.
    #[must_use]
    pub fn job(mut self, job: &'a Job) -> Self {
        self.jobs.push(job);
        self
    }

    /// Selects another process as the logical parent.
    #[must_use]
    pub fn parent_process(mut self, parent: &'a ParentProcess) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Applies a process-creation mitigation policy.
    #[must_use]
    pub const fn mitigation(mut self, mitigation: MitigationPolicy) -> Self {
        self.mitigation = mitigation;
        self
    }

    /// Selects exactly one ordinary-console or pseudoconsole mode.
    #[must_use]
    pub const fn terminal(mut self, terminal: TerminalMode<'a>) -> Self {
        self.terminal = terminal;
        self
    }

    /// Borrows a pseudoconsole and selects it as the terminal mode.
    #[must_use]
    pub fn pseudo_console<T: AsPseudoConsole + ?Sized>(mut self, owner: &'a T) -> Self {
        self.terminal = TerminalMode::PseudoConsole(owner.as_pseudo_console());
        self
    }

    /// Makes the child the root of a new process group.
    #[must_use]
    pub const fn new_process_group(mut self) -> Self {
        self.creation = self.creation.with(CREATE_NEW_PROCESS_GROUP);
        self
    }

    /// Inherits the logical parent's processor affinity.
    #[must_use]
    pub const fn inherit_parent_affinity(mut self) -> Self {
        self.creation = self.creation.with(INHERIT_PARENT_AFFINITY);
        self
    }

    /// Requests permission to break away from the caller's Job.
    #[must_use]
    pub const fn breakaway_from_job(mut self) -> Self {
        self.creation = self.creation.with(CREATE_BREAKAWAY_FROM_JOB);
        self
    }

    /// Preserves the caller's code-authorization level.
    #[must_use]
    pub const fn preserve_code_authorization(mut self) -> Self {
        self.creation = self.creation.with(CREATE_PRESERVE_CODE_AUTHZ_LEVEL);
        self
    }

    /// Prevents inheritance of the caller's hard-error mode.
    #[must_use]
    pub const fn default_error_mode(mut self) -> Self {
        self.creation = self.creation.with(CREATE_DEFAULT_ERROR_MODE);
        self
    }

    /// Selects how the private process-tree Job behaves when released.
    #[must_use]
    pub const fn job_close_policy(mut self, policy: JobClosePolicy) -> Self {
        self.job_close = policy;
        self
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn console_and_creation_policies_encode_every_closed_choice() {
        assert_eq!(ConsoleMode::Inherit.creation_bits(), 0);
        assert_eq!(ConsoleMode::Detached.creation_bits(), DETACHED_PROCESS);
        assert_eq!(ConsoleMode::NewConsole.creation_bits(), CREATE_NEW_CONSOLE);
        assert_eq!(ConsoleMode::NoWindow.creation_bits(), CREATE_NO_WINDOW);

        let options = SpawnOptions::new()
            .terminal(TerminalMode::Console(ConsoleMode::NewConsole))
            .new_process_group()
            .inherit_parent_affinity()
            .breakaway_from_job()
            .preserve_code_authorization()
            .default_error_mode();
        assert_eq!(
            options.creation.bits(),
            CREATE_NEW_PROCESS_GROUP
                | INHERIT_PARENT_AFFINITY
                | CREATE_BREAKAWAY_FROM_JOB
                | CREATE_PRESERVE_CODE_AUTHZ_LEVEL
                | CREATE_DEFAULT_ERROR_MODE
        );
        assert!(matches!(
            options.terminal,
            TerminalMode::Console(ConsoleMode::NewConsole)
        ));
        assert!(!format!("{options:?}").is_empty());
    }
}
