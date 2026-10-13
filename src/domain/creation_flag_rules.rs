//! The `CreateProcessW` creation-flag word: the flags a caller may name, the flags the crate adds, and the console modes.

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

/// Every flag a caller may name.
pub(crate) const PUBLIC: [u32; 8] = [
    DETACHED_PROCESS,
    NEW_CONSOLE,
    NEW_PROCESS_GROUP,
    INHERIT_PARENT_AFFINITY,
    BREAKAWAY_FROM_JOB,
    PRESERVE_CODE_AUTHZ_LEVEL,
    DEFAULT_ERROR_MODE,
    NO_WINDOW,
];

/// `CREATE_SUSPENDED`, which only a suspended spawn sets.
pub(crate) const SUSPENDED: u32 = 0x0000_0004;
/// `CREATE_UNICODE_ENVIRONMENT`, which every spawn sets.
pub(crate) const UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
/// `EXTENDED_STARTUPINFO_PRESENT`, which a spawn with an attribute list sets.
pub(crate) const EXTENDED_STARTUPINFO: u32 = 0x0008_0000;

/// The flags the crate sets itself.
pub(crate) const INTERNAL: [u32; 3] = [SUSPENDED, UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO];

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

/// Fails the build unless the flags are distinct bits and no public flag is an internal one.
///
/// `creation_word` names it, so every compiler evaluates it; Rust 1.75 counts no use inside `const _`.
const LAYOUT: () = assert!(
    distinct_bits(&PUBLIC) && distinct_bits(&INTERNAL) && union(&PUBLIC) & union(&INTERNAL) == 0,
    "creation flags are distinct bits, and no public flag is an internal one"
);

/// The word `CreateProcessW` receives for the `requested` public flags.
#[must_use]
pub(crate) const fn creation_word(requested: u32, attributes: bool, suspended: bool) -> u32 {
    let () = LAYOUT;
    let mut word = requested | UNICODE_ENVIRONMENT;
    if attributes {
        word |= EXTENDED_STARTUPINFO;
    }
    if suspended {
        word |= SUSPENDED;
    }
    word
}

/// Why a set of console flags can't create a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConsoleConflict {
    /// `DETACHED_PROCESS` and `CREATE_NEW_CONSOLE` request two console modes.
    DetachedWithNewConsole,
    /// `CREATE_NO_WINDOW` joins another console mode.
    NoWindowWithConsoleMode,
    /// A console mode joins a pseudoconsole, which supplies the console.
    ConsoleModeWithPseudoConsole,
}

/// Admits at most one console mode, and none beside a pseudoconsole.
///
/// # Errors
///
/// Returns the first conflict in the order the variants list them.
pub(crate) const fn validate_console(
    flags: u32,
    pseudoconsole: bool,
) -> Result<(), ConsoleConflict> {
    let detached = flags & DETACHED_PROCESS != 0;
    let new_console = flags & NEW_CONSOLE != 0;
    let no_window = flags & NO_WINDOW != 0;
    if detached && new_console {
        return Err(ConsoleConflict::DetachedWithNewConsole);
    }
    if no_window && (detached || new_console) {
        return Err(ConsoleConflict::NoWindowWithConsoleMode);
    }
    if pseudoconsole && (detached || new_console || no_window) {
        return Err(ConsoleConflict::ConsoleModeWithPseudoConsole);
    }
    Ok(())
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// The flags of one public `CreationFlags` value: the union of the named flags a caller combined.
    fn requested() -> u32 {
        let chosen: u8 = kani::any();
        let mut word = 0;
        for (index, flag) in PUBLIC.iter().enumerate() {
            if chosen & (1 << index) != 0 {
                word |= flag;
            }
        }
        word
    }

    #[kani::proof]
    fn internal_flags_follow_only_the_spawn_state() {
        let requested = requested();
        let attributes: bool = kani::any();
        let suspended: bool = kani::any();
        let word = creation_word(requested, attributes, suspended);
        assert_eq!(word & SUSPENDED != 0, suspended);
        assert_eq!(word & EXTENDED_STARTUPINFO != 0, attributes);
        assert_ne!(word & UNICODE_ENVIRONMENT, 0);
        assert_eq!(word & union(&PUBLIC), requested);
        kani::cover!(!suspended && requested == union(&PUBLIC));
        kani::cover!(suspended && requested == 0);
    }

    #[kani::proof]
    fn console_validation_admits_at_most_one_mode() {
        let flags: u32 = kani::any();
        let pseudoconsole: bool = kani::any();
        let modes = [DETACHED_PROCESS, NEW_CONSOLE, NO_WINDOW]
            .iter()
            .filter(|mode| flags & **mode != 0)
            .count();
        let admitted = validate_console(flags, pseudoconsole).is_ok();
        assert_eq!(admitted, modes <= usize::from(!pseudoconsole));
        kani::cover!(admitted && modes == 1);
        kani::cover!(!admitted && modes == 1);
        kani::cover!(!admitted && modes == 3);
    }

    #[kani::proof]
    fn negative_control_suspended_running_spawn() {
        let requested = requested() | SUSPENDED;
        let word = creation_word(requested, kani::any(), false);
        assert!(word & SUSPENDED == 0, "NEGATIVE_CREATION_FLAG_CONTROL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_conflict_has_its_own_combination() {
        assert_eq!(
            validate_console(DETACHED_PROCESS | NEW_CONSOLE, false),
            Err(ConsoleConflict::DetachedWithNewConsole)
        );
        assert_eq!(
            validate_console(NO_WINDOW | NEW_CONSOLE, false),
            Err(ConsoleConflict::NoWindowWithConsoleMode)
        );
        assert_eq!(
            validate_console(NO_WINDOW, true),
            Err(ConsoleConflict::ConsoleModeWithPseudoConsole)
        );
        assert_eq!(
            validate_console(union(&PUBLIC) & !NEW_CONSOLE & !NO_WINDOW, false),
            Ok(())
        );
        assert_eq!(
            creation_word(NEW_PROCESS_GROUP, true, true),
            NEW_PROCESS_GROUP | UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO | SUSPENDED
        );
        assert_eq!(creation_word(0, false, false), UNICODE_ENVIRONMENT);
    }
}
