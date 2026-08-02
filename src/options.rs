//! Per-spawn capabilities and creation policy.

use std::fmt;
use std::ops::{BitOr, BitOrAssign};

use crate::handles::{AsPseudoConsole, Job, ParentProcess};
use crate::mitigation::MitigationPolicy;

/// What dropping a live [`crate::Child`] does to its process tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DropPolicy {
    /// Close windows-spawn's process handle without terminating the process.
    #[default]
    Detach,
    /// Terminate the child and all descendants in windows-spawn's private Job.
    KillTree,
}

/// Safe, named `CreateProcessW` creation flags.
///
/// Unicode-environment, extended-startup-info, and suspended flags are set
/// internally. There is no raw-bits constructor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CreationFlags(u32);

impl CreationFlags {
    /// Creates an empty flag set.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// Creates a process without inheriting a console.
    pub const DETACHED_PROCESS: Self = Self(0x0000_0008);
    /// Gives the child a new console.
    pub const NEW_CONSOLE: Self = Self(0x0000_0010);
    /// Makes the child the root of a new process group.
    pub const NEW_PROCESS_GROUP: Self = Self(0x0000_0200);
    /// Inherits the parent's processor affinity.
    pub const INHERIT_PARENT_AFFINITY: Self = Self(0x0001_0000);
    /// Allows the child to break away from the caller's Job when permitted.
    pub const BREAKAWAY_FROM_JOB: Self = Self(0x0100_0000);
    /// Preserves the caller's code-authorization level.
    pub const PRESERVE_CODE_AUTHZ_LEVEL: Self = Self(0x0200_0000);
    /// Prevents the child from inheriting the caller's hard-error mode.
    pub const DEFAULT_ERROR_MODE: Self = Self(0x0400_0000);
    /// Runs a console application without creating a console window.
    pub const NO_WINDOW: Self = Self(0x0800_0000);

    pub(crate) const fn bits(self) -> u32 {
        self.0
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for CreationFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for CreationFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Capabilities and policy needed only for one spawn operation.
///
/// Jobs are retained in call order, from the root Job to the innermost Job.
/// The options borrow every capability; they never assume ownership of a Job,
/// parent process, or pseudoconsole.
///
/// The borrow cannot escape the capability it protects:
///
/// ```compile_fail
/// use windows_spawn::{Command, Job, SpawnOptions};
///
/// let options;
/// {
///     let job = Job::create().unwrap();
///     options = SpawnOptions::new().job(&job);
/// }
/// Command::new("cmd.exe").spawn_with(options).unwrap();
/// ```
pub struct SpawnOptions<'a> {
    pub(crate) jobs: Vec<&'a Job>,
    pub(crate) parent: Option<&'a ParentProcess>,
    pub(crate) mitigation: MitigationPolicy,
    pub(crate) pseudoconsole: Option<&'a dyn AsPseudoConsole>,
    pub(crate) creation_flags: CreationFlags,
    pub(crate) drop_policy: DropPolicy,
}

impl fmt::Debug for SpawnOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpawnOptions")
            .field("jobs", &self.jobs)
            .field("parent", &self.parent)
            .field("mitigation", &self.mitigation)
            .field("pseudoconsole", &self.pseudoconsole.map(|_| "borrowed"))
            .field("creation_flags", &self.creation_flags)
            .field("drop_policy", &self.drop_policy)
            .finish()
    }
}

impl Default for SpawnOptions<'_> {
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            parent: None,
            mitigation: MitigationPolicy::new(),
            pseudoconsole: None,
            creation_flags: CreationFlags::new(),
            drop_policy: DropPolicy::Detach,
        }
    }
}

impl<'a> SpawnOptions<'a> {
    /// Creates default per-spawn options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a Job, preserving root-to-inner ordering.
    #[must_use]
    pub fn job(mut self, job: &'a Job) -> Self {
        self.jobs.push(job);
        self
    }

    /// Chooses another process as the logical parent.
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

    /// Attaches the child to a borrowed pseudoconsole.
    #[must_use]
    pub fn pseudoconsole<T: AsPseudoConsole>(mut self, pseudoconsole: &'a T) -> Self {
        self.pseudoconsole = Some(pseudoconsole);
        self
    }

    /// Adds named process-creation flags.
    #[must_use]
    pub const fn creation_flags(mut self, flags: CreationFlags) -> Self {
        self.creation_flags = flags;
        self
    }

    /// Chooses the live-child drop policy.
    #[must_use]
    pub const fn drop_policy(mut self, policy: DropPolicy) -> Self {
        self.drop_policy = policy;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_flags_combine_idempotently_and_options_keep_job_order() {
        let combined = CreationFlags::NEW_PROCESS_GROUP | CreationFlags::DEFAULT_ERROR_MODE;
        assert_eq!(combined.bits(), 0x0400_0200);
        assert_eq!(
            (CreationFlags::NEW_PROCESS_GROUP | CreationFlags::NEW_PROCESS_GROUP).bits(),
            CreationFlags::NEW_PROCESS_GROUP.bits()
        );
        let mut assigned = CreationFlags::NEW_PROCESS_GROUP;
        assigned |= CreationFlags::DEFAULT_ERROR_MODE;
        assert_eq!(assigned.bits(), combined.bits());

        let outer = Job::create().unwrap();
        let inner = Job::create().unwrap();
        let options = SpawnOptions::new().job(&outer).job(&inner);
        assert_eq!(options.jobs.len(), 2);
        assert!(std::ptr::eq(options.jobs[0], &outer));
        assert!(std::ptr::eq(options.jobs[1], &inner));
    }
}
