//! The `CreateProcessW` creation-flag word: the policy flags a caller sets, the console and priority class it chooses, and the flags the crate adds.

/// `DETACHED_PROCESS`: the child gets no console.
pub(crate) const DETACHED_PROCESS: u32 = 0x0000_0008;
/// `CREATE_NEW_CONSOLE`: the child gets a console of its own.
pub(crate) const NEW_CONSOLE: u32 = 0x0000_0010;
/// `CREATE_NEW_PROCESS_GROUP`: the child roots a new process group.
pub(crate) const NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// `INHERIT_PARENT_AFFINITY`: the child takes the parent's processor affinity.
pub(crate) const INHERIT_PARENT_AFFINITY: u32 = 0x0001_0000;
/// `CREATE_BREAKAWAY_FROM_JOB`: the child leaves the caller's Job where the Job allows it.
pub(crate) const BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
/// `CREATE_PRESERVE_CODE_AUTHZ_LEVEL`: the child keeps the caller's code-authorization level.
pub(crate) const PRESERVE_CODE_AUTHZ_LEVEL: u32 = 0x0200_0000;
/// `CREATE_DEFAULT_ERROR_MODE`: the child starts with the default hard-error mode.
pub(crate) const DEFAULT_ERROR_MODE: u32 = 0x0400_0000;
/// `CREATE_NO_WINDOW`: a console child runs without a console window.
pub(crate) const NO_WINDOW: u32 = 0x0800_0000;

/// `IDLE_PRIORITY_CLASS`.
pub(crate) const IDLE_PRIORITY: u32 = 0x0000_0040;
/// `BELOW_NORMAL_PRIORITY_CLASS`.
pub(crate) const BELOW_NORMAL_PRIORITY: u32 = 0x0000_4000;
/// `NORMAL_PRIORITY_CLASS`.
pub(crate) const NORMAL_PRIORITY: u32 = 0x0000_0020;
/// `ABOVE_NORMAL_PRIORITY_CLASS`.
pub(crate) const ABOVE_NORMAL_PRIORITY: u32 = 0x0000_8000;
/// `HIGH_PRIORITY_CLASS`.
pub(crate) const HIGH_PRIORITY: u32 = 0x0000_0080;
/// `REALTIME_PRIORITY_CLASS`.
pub(crate) const REALTIME_PRIORITY: u32 = 0x0000_0100;

/// The flags a caller sets, each through its own setter.
pub(crate) const POLICY: [u32; 5] = [
    NEW_PROCESS_GROUP,
    INHERIT_PARENT_AFFINITY,
    BREAKAWAY_FROM_JOB,
    PRESERVE_CODE_AUTHZ_LEVEL,
    DEFAULT_ERROR_MODE,
];

/// The console flags, of which a spawn sets at most one.
pub(crate) const CONSOLE: [u32; 3] = [DETACHED_PROCESS, NEW_CONSOLE, NO_WINDOW];

/// The priority-class flags, of which a spawn sets at most one.
pub(crate) const PRIORITY: [u32; 6] = [
    IDLE_PRIORITY,
    BELOW_NORMAL_PRIORITY,
    NORMAL_PRIORITY,
    ABOVE_NORMAL_PRIORITY,
    HIGH_PRIORITY,
    REALTIME_PRIORITY,
];

/// `CREATE_SUSPENDED`, which every spawn sets, so the crate releases its temporary resources before the child runs.
pub(crate) const SUSPENDED: u32 = 0x0000_0004;
/// `CREATE_UNICODE_ENVIRONMENT`, which every spawn sets.
pub(crate) const UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
/// `EXTENDED_STARTUPINFO_PRESENT`, which a spawn with an attribute list sets.
pub(crate) const EXTENDED_STARTUPINFO: u32 = 0x0008_0000;

/// The flags the crate sets itself.
pub(crate) const INTERNAL: [u32; 3] = [SUSPENDED, UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO];

/// The console a spawn chooses: the caller's, or one console flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(crate) enum ConsoleFlag {
    /// The caller's console.
    Inherit,
    /// `DETACHED_PROCESS`.
    NoConsole,
    /// `CREATE_NEW_CONSOLE`.
    NewConsole,
    /// `CREATE_NO_WINDOW`.
    NoWindow,
}

impl ConsoleFlag {
    /// The console flag the choice sets, or none.
    #[must_use]
    pub(crate) const fn flag(self) -> u32 {
        match self {
            Self::Inherit => 0,
            Self::NoConsole => DETACHED_PROCESS,
            Self::NewConsole => NEW_CONSOLE,
            Self::NoWindow => NO_WINDOW,
        }
    }
}

/// The priority class a spawn chooses: the one Windows gives by default, or one priority-class flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub(crate) enum PriorityFlag {
    /// No flag: normal, or the caller's class when that is idle or below normal.
    Default,
    /// `IDLE_PRIORITY_CLASS`.
    Idle,
    /// `BELOW_NORMAL_PRIORITY_CLASS`.
    BelowNormal,
    /// `NORMAL_PRIORITY_CLASS`.
    Normal,
    /// `ABOVE_NORMAL_PRIORITY_CLASS`.
    AboveNormal,
    /// `HIGH_PRIORITY_CLASS`.
    High,
    /// `REALTIME_PRIORITY_CLASS`.
    Realtime,
}

impl PriorityFlag {
    /// The priority-class flag the choice sets, or none.
    #[must_use]
    pub(crate) const fn flag(self) -> u32 {
        match self {
            Self::Default => 0,
            Self::Idle => IDLE_PRIORITY,
            Self::BelowNormal => BELOW_NORMAL_PRIORITY,
            Self::Normal => NORMAL_PRIORITY,
            Self::AboveNormal => ABOVE_NORMAL_PRIORITY,
            Self::High => HIGH_PRIORITY,
            Self::Realtime => REALTIME_PRIORITY,
        }
    }
}

const fn union(flags: &[u32]) -> u32 {
    let mut word = 0;
    let mut rest = flags;
    while let [flag, tail @ ..] = rest {
        word |= *flag;
        rest = tail;
    }
    word
}

/// Returns true if each flag is one bit and no two of them share it.
const fn distinct_bits(flags: &[u32]) -> bool {
    let mut seen = 0;
    let mut rest = flags;
    while let [flag, tail @ ..] = rest {
        if flag.count_ones() != 1 || seen & *flag != 0 {
            return false;
        }
        seen |= *flag;
        rest = tail;
    }
    true
}

/// Fails the build unless the policy, console, priority, and internal flags are distinct bits that no two groups share.
///
/// `creation_word` names it, so every compiler evaluates it; Rust 1.84 counts no use inside `const _`.
const LAYOUT: () = assert!(
    distinct_bits(&POLICY)
        && distinct_bits(&CONSOLE)
        && distinct_bits(&PRIORITY)
        && distinct_bits(&INTERNAL)
        && disjoint(&[
            union(&POLICY),
            union(&CONSOLE),
            union(&PRIORITY),
            union(&INTERNAL),
        ]),
    "creation flags are distinct bits, and no two groups share one"
);

/// Returns true if no two of the words share a bit.
const fn disjoint(words: &[u32]) -> bool {
    let mut seen = 0;
    let mut rest = words;
    while let [word, tail @ ..] = rest {
        if seen & *word != 0 {
            return false;
        }
        seen |= *word;
        rest = tail;
    }
    true
}

/// The word `CreateProcessW` receives: the `policy` flags, the console's and the priority's flags, and the flags the crate sets.
#[must_use]
pub(crate) const fn creation_word(
    policy: u32,
    console: ConsoleFlag,
    priority: PriorityFlag,
    attributes: bool,
) -> u32 {
    let () = LAYOUT;
    let mut word = (policy & union(&POLICY))
        | console.flag()
        | priority.flag()
        | UNICODE_ENVIRONMENT
        | SUSPENDED;
    if attributes {
        word |= EXTENDED_STARTUPINFO;
    }
    word
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// The policy flags of one command: the union of the setters a caller called.
    fn policy() -> u32 {
        let chosen: u8 = kani::any();
        let mut word = 0;
        for (index, flag) in POLICY.iter().enumerate() {
            if chosen & (1 << index) != 0 {
                word |= flag;
            }
        }
        word
    }

    #[kani::proof]
    fn the_word_holds_the_policy_one_console_one_priority_and_the_internal_flags() {
        let policy = policy();
        let console: ConsoleFlag = kani::any();
        let priority: PriorityFlag = kani::any();
        let attributes: bool = kani::any();
        let word = creation_word(policy, console, priority, attributes);
        assert_eq!(word & union(&POLICY), policy);
        assert_eq!(word & union(&CONSOLE), console.flag());
        assert!((word & union(&CONSOLE)).count_ones() <= 1);
        assert_eq!(word & union(&PRIORITY), priority.flag());
        assert!((word & union(&PRIORITY)).count_ones() <= 1);
        assert_ne!(word & SUSPENDED, 0);
        assert_ne!(word & UNICODE_ENVIRONMENT, 0);
        assert_eq!(word & EXTENDED_STARTUPINFO != 0, attributes);
        kani::cover!(
            console == ConsoleFlag::NoWindow
                && priority == PriorityFlag::Realtime
                && policy == union(&POLICY)
        );
        kani::cover!(
            console == ConsoleFlag::Inherit && priority == PriorityFlag::Default && policy == 0
        );
    }

    #[kani::proof]
    fn negative_control_a_spawn_that_runs_at_once() {
        let word = creation_word(policy(), kani::any(), kani::any(), kani::any());
        assert!(word & SUSPENDED == 0, "NEGATIVE_CREATION_FLAG_CONTROL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_console_and_priority_sets_its_own_flag() {
        assert_eq!(ConsoleFlag::Inherit.flag(), 0);
        assert_eq!(ConsoleFlag::NoConsole.flag(), DETACHED_PROCESS);
        assert_eq!(ConsoleFlag::NewConsole.flag(), NEW_CONSOLE);
        assert_eq!(ConsoleFlag::NoWindow.flag(), NO_WINDOW);
        let priorities = [
            PriorityFlag::Idle,
            PriorityFlag::BelowNormal,
            PriorityFlag::Normal,
            PriorityFlag::AboveNormal,
            PriorityFlag::High,
            PriorityFlag::Realtime,
        ];
        assert_eq!(priorities.map(PriorityFlag::flag), PRIORITY);
        assert_eq!(PriorityFlag::Default.flag(), 0);
        assert_eq!(
            creation_word(
                NEW_PROCESS_GROUP,
                ConsoleFlag::NewConsole,
                PriorityFlag::BelowNormal,
                true
            ),
            NEW_PROCESS_GROUP
                | NEW_CONSOLE
                | BELOW_NORMAL_PRIORITY
                | UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO
                | SUSPENDED
        );
        assert_eq!(
            creation_word(0, ConsoleFlag::Inherit, PriorityFlag::Default, false),
            UNICODE_ENVIRONMENT | SUSPENDED
        );
        assert_eq!(
            creation_word(
                DETACHED_PROCESS | IDLE_PRIORITY,
                ConsoleFlag::Inherit,
                PriorityFlag::Default,
                false
            ),
            UNICODE_ENVIRONMENT | SUSPENDED
        );
    }
}
