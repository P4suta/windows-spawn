//! Process-creation choices a command owns, and the capabilities one spawn borrows.

use windows_sys::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, CREATE_BREAKAWAY_FROM_JOB,
    CREATE_DEFAULT_ERROR_MODE, CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
    CREATE_PRESERVE_CODE_AUTHZ_LEVEL, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    DETACHED_PROCESS, EXTENDED_STARTUPINFO_PRESENT, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    INHERIT_PARENT_AFFINITY, NORMAL_PRIORITY_CLASS, REALTIME_PRIORITY_CLASS,
};

use crate::domain::creation_flag_rules::{self as flags, ConsoleFlag, PriorityFlag};
use crate::handles::{BorrowedPseudoConsole, Job, ParentProcess};
use crate::mitigation::MitigationPolicy;

/// The flag words the proofs reason about are the SDK's.
const _: () = assert!(
    flags::DETACHED_PROCESS == DETACHED_PROCESS
        && flags::NEW_CONSOLE == CREATE_NEW_CONSOLE
        && flags::NEW_PROCESS_GROUP == CREATE_NEW_PROCESS_GROUP
        && flags::INHERIT_PARENT_AFFINITY == INHERIT_PARENT_AFFINITY
        && flags::BREAKAWAY_FROM_JOB == CREATE_BREAKAWAY_FROM_JOB
        && flags::PRESERVE_CODE_AUTHZ_LEVEL == CREATE_PRESERVE_CODE_AUTHZ_LEVEL
        && flags::DEFAULT_ERROR_MODE == CREATE_DEFAULT_ERROR_MODE
        && flags::NO_WINDOW == CREATE_NO_WINDOW
        && flags::IDLE_PRIORITY == IDLE_PRIORITY_CLASS
        && flags::BELOW_NORMAL_PRIORITY == BELOW_NORMAL_PRIORITY_CLASS
        && flags::NORMAL_PRIORITY == NORMAL_PRIORITY_CLASS
        && flags::ABOVE_NORMAL_PRIORITY == ABOVE_NORMAL_PRIORITY_CLASS
        && flags::HIGH_PRIORITY == HIGH_PRIORITY_CLASS
        && flags::REALTIME_PRIORITY == REALTIME_PRIORITY_CLASS
        && flags::SUSPENDED == CREATE_SUSPENDED
        && flags::UNICODE_ENVIRONMENT == CREATE_UNICODE_ENVIRONMENT
        && flags::EXTENDED_STARTUPINFO == EXTENDED_STARTUPINFO_PRESENT,
    "a creation flag differs from windows-sys"
);

/// The console of a child, which [`crate::Command::console`] chooses.
///
/// Each mode's discriminant is the creation flag it sets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum ConsoleMode {
    /// The child shares the caller's console, if the caller has one.
    Inherit = 0,
    /// The child starts without a console: `DETACHED_PROCESS`.
    ///
    /// A console program that later starts another console program gives that one a new console window,
    /// so a GUI program that runs console tools should choose [`Self::NoWindow`] instead.
    NoConsole = DETACHED_PROCESS,
    /// The child gets a new console window: `CREATE_NEW_CONSOLE`.
    NewConsole = CREATE_NEW_CONSOLE,
    /// The child gets a console without a window, which its own console children share: `CREATE_NO_WINDOW`.
    ///
    /// This is the mode for a GUI program that runs console tools.
    NoWindow = CREATE_NO_WINDOW,
}

impl Default for ConsoleMode {
    /// Shares the caller's console, as `CreateProcessW` does without console flags.
    fn default() -> Self {
        Self::Inherit
    }
}

impl ConsoleMode {
    const fn flag(self) -> ConsoleFlag {
        match self {
            Self::Inherit => ConsoleFlag::Inherit,
            Self::NoConsole => ConsoleFlag::NoConsole,
            Self::NewConsole => ConsoleFlag::NewConsole,
            Self::NoWindow => ConsoleFlag::NoWindow,
        }
    }
}

/// The scheduling priority class of a child, which [`crate::Command::priority`] chooses.
///
/// Each class's discriminant is the creation flag it sets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum PriorityClass {
    /// `IDLE_PRIORITY_CLASS`: runs only when the system is idle.
    Idle = IDLE_PRIORITY_CLASS,
    /// `BELOW_NORMAL_PRIORITY_CLASS`: yields to normal processes, as a background build should.
    BelowNormal = BELOW_NORMAL_PRIORITY_CLASS,
    /// `NORMAL_PRIORITY_CLASS`.
    Normal = NORMAL_PRIORITY_CLASS,
    /// `ABOVE_NORMAL_PRIORITY_CLASS`.
    AboveNormal = ABOVE_NORMAL_PRIORITY_CLASS,
    /// `HIGH_PRIORITY_CLASS`: preempts normal processes.
    High = HIGH_PRIORITY_CLASS,
    /// `REALTIME_PRIORITY_CLASS`, which Windows lowers to [`Self::High`] for a caller without the privilege to raise priorities.
    Realtime = REALTIME_PRIORITY_CLASS,
}

impl PriorityClass {
    const fn flag(self) -> PriorityFlag {
        match self {
            Self::Idle => PriorityFlag::Idle,
            Self::BelowNormal => PriorityFlag::BelowNormal,
            Self::Normal => PriorityFlag::Normal,
            Self::AboveNormal => PriorityFlag::AboveNormal,
            Self::High => PriorityFlag::High,
            Self::Realtime => PriorityFlag::Realtime,
        }
    }
}

/// The process-creation choices a [`crate::Command`] owns.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Creation {
    pub(crate) console: ConsoleMode,
    priority: PriorityFlag,
    policy: u32,
    pub(crate) mitigation: MitigationPolicy,
    /// The child gets a private Job that its `Child` owns.
    pub(crate) owns_tree: bool,
}

impl Creation {
    /// Shares the caller's console, sets no priority class or policy flag, applies no mitigation, and leaves the process tree unowned.
    pub(crate) const fn new() -> Self {
        Self {
            console: ConsoleMode::Inherit,
            priority: PriorityFlag::Default,
            policy: 0,
            mitigation: MitigationPolicy::new(),
            owns_tree: false,
        }
    }

    pub(crate) const fn set_priority(&mut self, class: PriorityClass) {
        self.priority = class.flag();
    }

    /// Sets one policy flag.
    pub(crate) const fn set(&mut self, flag: u32) {
        self.policy |= flag;
    }

    /// The policy flags the setters chose.
    pub(crate) const fn policy(&self) -> u32 {
        self.policy
    }

    pub(crate) const fn console_flag(&self) -> ConsoleFlag {
        self.console.flag()
    }

    pub(crate) const fn priority_flag(&self) -> PriorityFlag {
        self.priority
    }
}

/// Capabilities that one spawn borrows: Jobs, an alternate parent, and a pseudoconsole.
///
/// What a command owns, such as its console mode or [`crate::Command::own_process_tree`], belongs to [`crate::Command`];
/// these options hold only borrows, so they can't outlive what they borrow:
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
#[derive(Clone, Debug)]
pub struct SpawnOptions<'a> {
    pub(crate) jobs: Vec<&'a Job>,
    pub(crate) parent: Option<&'a ParentProcess>,
    pub(crate) pseudoconsole: Option<BorrowedPseudoConsole<'a>>,
}

impl Default for SpawnOptions<'_> {
    /// Borrows nothing: no Job, the caller as the parent, and no pseudoconsole.
    fn default() -> Self {
        Self {
            jobs: Vec::new(),
            parent: None,
            pseudoconsole: None,
        }
    }
}

impl<'a> SpawnOptions<'a> {
    /// Creates options that borrow nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a Job, preserving root-to-inner ordering.
    ///
    /// Jobs are attached during creation, before the child runs any code.
    #[must_use]
    pub fn job(mut self, job: &'a Job) -> Self {
        self.jobs.push(job);
        self
    }

    /// Chooses another process as the logical parent: `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`.
    ///
    /// The child inherits from that process, so the [`crate::Command`] must configure all three standard streams.
    #[must_use]
    pub const fn parent_process(mut self, parent: &'a ParentProcess) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Attaches the child to a pseudoconsole: `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`.
    ///
    /// It takes a reference to a value that implements [`crate::AsPseudoConsole`], or a [`BorrowedPseudoConsole`].
    /// The pseudoconsole is the child's console and supplies all three standard streams,
    /// so the [`crate::Command`] must choose no console mode and configure no standard stream.
    #[must_use]
    pub fn pseudo_console(mut self, console: impl Into<BorrowedPseudoConsole<'a>>) -> Self {
        self.pseudoconsole = Some(console.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::sys::test_support::ChangingPseudoConsole;

    const SNAPSHOT: isize = 42;
    const CHANGED: isize = 99;

    #[test]
    fn creation_choices_combine_and_default_to_nothing() {
        let mut creation = Creation::new();
        assert_eq!(creation.policy(), 0);
        assert_eq!(creation.console_flag(), ConsoleFlag::Inherit);
        assert_eq!(creation.priority_flag(), PriorityFlag::Default);
        assert!(!creation.owns_tree);
        assert_eq!(ConsoleMode::default(), ConsoleMode::Inherit);
        for flag in flags::POLICY {
            creation.set(flag);
            creation.set(flag);
        }
        assert_eq!(
            creation.policy(),
            flags::POLICY.iter().fold(0, |word, flag| word | flag)
        );
        for (class, flag) in [
            (PriorityClass::Idle, PriorityFlag::Idle),
            (PriorityClass::BelowNormal, PriorityFlag::BelowNormal),
            (PriorityClass::Normal, PriorityFlag::Normal),
            (PriorityClass::AboveNormal, PriorityFlag::AboveNormal),
            (PriorityClass::High, PriorityFlag::High),
            (PriorityClass::Realtime, PriorityFlag::Realtime),
        ] {
            creation.set_priority(class);
            assert_eq!(creation.priority_flag(), flag);
        }
        for (mode, flag) in [
            (ConsoleMode::Inherit, ConsoleFlag::Inherit),
            (ConsoleMode::NoConsole, ConsoleFlag::NoConsole),
            (ConsoleMode::NewConsole, ConsoleFlag::NewConsole),
            (ConsoleMode::NoWindow, ConsoleFlag::NoWindow),
        ] {
            creation.console = mode;
            assert_eq!(creation.console_flag(), flag);
        }
    }

    #[test]
    fn options_keep_job_order_and_reuse_by_clone() {
        let outer = Job::create().unwrap();
        let inner = Job::create().unwrap();
        let options = SpawnOptions::new().job(&outer).job(&inner);
        let reused = options.clone();
        assert_eq!(reused.jobs.len(), 2);
        assert!(std::ptr::eq(reused.jobs[0], &raw const outer));
        assert!(std::ptr::eq(reused.jobs[1], &raw const inner));
        let empty = SpawnOptions::default();
        assert!(empty.jobs.is_empty() && empty.parent.is_none() && empty.pseudoconsole.is_none());
    }

    #[test]
    fn a_pseudoconsole_is_read_once_from_a_reference_or_a_borrow() {
        let pseudoconsole = ChangingPseudoConsole(Cell::new(SNAPSHOT));
        let options = SpawnOptions::new().pseudo_console(&pseudoconsole);
        pseudoconsole.0.set(CHANGED);
        assert_eq!(
            options.pseudoconsole.map(BorrowedPseudoConsole::as_raw),
            Some(SNAPSHOT)
        );
        assert!(format!("{options:?}").contains("BorrowedPseudoConsole"));

        let borrowed = crate::AsPseudoConsole::as_pseudo_console(&pseudoconsole);
        let from_borrow = SpawnOptions::new().pseudo_console(borrowed);
        assert_eq!(
            from_borrow.pseudoconsole.map(BorrowedPseudoConsole::as_raw),
            Some(CHANGED)
        );
    }
}
