//! Standard I/O planning: where each stream comes from, and which combinations of options a spawn refuses.

use super::creation_flag_rules::{validate_console, ConsoleConflict};

/// Whether the caller takes the child's standard streams or the crate captures them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IoMode {
    /// `spawn` and `status`: an unconfigured stream inherits the caller's.
    Spawn,
    /// `output`: unconfigured input reads the null device, and unconfigured output and error go to pipes.
    Output,
}

/// One value for each standard stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Streams<T> {
    /// Standard input.
    pub(crate) stdin: T,
    /// Standard output.
    pub(crate) stdout: T,
    /// Standard error.
    pub(crate) stderr: T,
}

/// Where one standard stream comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source<T> {
    /// The caller configured the stream.
    Configured(T),
    /// The caller's own stream.
    Inherit,
    /// The null device.
    Null,
    /// A new pipe whose other end the caller receives.
    Piped,
}

/// The standard I/O of a spawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StandardIo<T> {
    /// Three standard handles.
    Ordinary(Streams<Source<T>>),
    /// A pseudoconsole supplies the console, and the three standard handles stay null.
    PseudoConsole,
}

/// A combination of options that can't create a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conflict {
    /// The console flags conflict.
    Console(ConsoleConflict),
    /// A pseudoconsole supplies the standard streams, so none may be configured.
    PseudoConsoleWithExplicitStdio,
    /// Output capture needs pipes, which a pseudoconsole replaces.
    OutputWithPseudoConsole,
    /// An alternate parent's handle table holds no caller stream to inherit.
    ParentWithoutExplicitStdio,
}

impl Conflict {
    /// The message of the `InvalidInput` error that reports the conflict.
    #[must_use]
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Console(ConsoleConflict::DetachedWithNewConsole) => {
                "DETACHED_PROCESS and NEW_CONSOLE are mutually exclusive"
            }
            Self::Console(ConsoleConflict::NoWindowWithConsoleMode) => {
                "NO_WINDOW cannot be combined with DETACHED_PROCESS or NEW_CONSOLE"
            }
            Self::Console(ConsoleConflict::ConsoleModeWithPseudoConsole) => {
                "console creation flags conflict with a pseudoconsole"
            }
            Self::PseudoConsoleWithExplicitStdio => {
                "a pseudoconsole conflicts with explicit standard I/O"
            }
            Self::OutputWithPseudoConsole => {
                "output capture conflicts with pseudoconsole standard I/O"
            }
            Self::ParentWithoutExplicitStdio => {
                "an alternate parent requires all three standard streams to be explicit"
            }
        }
    }
}

/// What planning reads from a spawn request.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Request<T> {
    /// The streams the caller configured.
    pub(crate) configured: Streams<Option<T>>,
    /// The public creation flags.
    pub(crate) flags: u32,
    /// Whether a pseudoconsole supplies the console.
    pub(crate) pseudoconsole: bool,
    /// Whether another process becomes the parent.
    pub(crate) parent: bool,
    /// Who takes the standard streams.
    pub(crate) mode: IoMode,
}

/// Decides the standard I/O of a spawn, or the first conflict among its options.
///
/// # Errors
///
/// Returns a console conflict first, then a pseudoconsole conflict, then an alternate parent without three configured streams.
pub(crate) fn plan<T>(request: Request<T>) -> Result<StandardIo<T>, Conflict> {
    let Request {
        configured,
        flags,
        pseudoconsole,
        parent,
        mode,
    } = request;
    validate_console(flags, pseudoconsole).map_err(Conflict::Console)?;
    if pseudoconsole {
        let explicit = configured.stdin.is_some()
            || configured.stdout.is_some()
            || configured.stderr.is_some();
        if explicit {
            return Err(Conflict::PseudoConsoleWithExplicitStdio);
        }
        if mode == IoMode::Output {
            return Err(Conflict::OutputWithPseudoConsole);
        }
        return Ok(StandardIo::PseudoConsole);
    }
    if parent
        && (configured.stdin.is_none()
            || configured.stdout.is_none()
            || configured.stderr.is_none())
    {
        return Err(Conflict::ParentWithoutExplicitStdio);
    }
    let defaults = match mode {
        IoMode::Spawn => Streams {
            stdin: Source::Inherit,
            stdout: Source::Inherit,
            stderr: Source::Inherit,
        },
        IoMode::Output => Streams {
            stdin: Source::Null,
            stdout: Source::Piped,
            stderr: Source::Piped,
        },
    };
    Ok(StandardIo::Ordinary(Streams {
        stdin: configured.stdin.map_or(defaults.stdin, Source::Configured),
        stdout: configured
            .stdout
            .map_or(defaults.stdout, Source::Configured),
        stderr: configured
            .stderr
            .map_or(defaults.stderr, Source::Configured),
    }))
}

#[cfg(kani)]
mod proofs {
    use super::*;

    fn configured() -> Option<u8> {
        if kani::any() {
            Some(kani::any())
        } else {
            None
        }
    }

    fn mode() -> IoMode {
        if kani::any() {
            IoMode::Spawn
        } else {
            IoMode::Output
        }
    }

    fn expected(configured: Option<u8>, default: Source<u8>) -> Source<u8> {
        configured.map_or(default, Source::Configured)
    }

    #[kani::proof]
    fn plans_follow_the_documented_rules() {
        let request = Request {
            configured: Streams {
                stdin: configured(),
                stdout: configured(),
                stderr: configured(),
            },
            flags: kani::any(),
            pseudoconsole: kani::any(),
            parent: kani::any(),
            mode: mode(),
        };
        let Streams {
            stdin,
            stdout,
            stderr,
        } = request.configured;
        let explicit = stdin.is_some() || stdout.is_some() || stderr.is_some();
        let complete = stdin.is_some() && stdout.is_some() && stderr.is_some();
        let console = validate_console(request.flags, request.pseudoconsole);
        let result = plan(request);
        match result {
            Ok(StandardIo::PseudoConsole) => {
                assert!(request.pseudoconsole && !explicit && request.mode == IoMode::Spawn);
                assert!(console.is_ok());
            }
            Ok(StandardIo::Ordinary(streams)) => {
                assert!(!request.pseudoconsole && console.is_ok());
                assert!(!request.parent || complete);
                let (input, output) = match request.mode {
                    IoMode::Spawn => (Source::Inherit, Source::Inherit),
                    IoMode::Output => (Source::Null, Source::Piped),
                };
                assert_eq!(streams.stdin, expected(stdin, input));
                assert_eq!(streams.stdout, expected(stdout, output));
                assert_eq!(streams.stderr, expected(stderr, output));
            }
            Err(Conflict::Console(conflict)) => assert_eq!(console, Err(conflict)),
            Err(Conflict::PseudoConsoleWithExplicitStdio) => {
                assert!(console.is_ok() && request.pseudoconsole && explicit);
            }
            Err(Conflict::OutputWithPseudoConsole) => {
                assert!(console.is_ok() && request.pseudoconsole && !explicit);
                assert_eq!(request.mode, IoMode::Output);
            }
            Err(Conflict::ParentWithoutExplicitStdio) => {
                assert!(console.is_ok() && request.parent && !request.pseudoconsole && !complete);
            }
        }
        let refused = console.is_err()
            || (request.pseudoconsole && (explicit || request.mode == IoMode::Output))
            || (request.parent && !request.pseudoconsole && !complete);
        assert_eq!(result.is_err(), refused);
        kani::cover!(matches!(result, Ok(StandardIo::PseudoConsole)) && request.parent);
        kani::cover!(matches!(result, Ok(StandardIo::Ordinary(_))) && request.parent);
        kani::cover!(result == Err(Conflict::OutputWithPseudoConsole));
        kani::cover!(result == Err(Conflict::ParentWithoutExplicitStdio) && explicit);
    }

    fn conflict() -> Conflict {
        match kani::any::<u8>() % 6 {
            0 => Conflict::Console(ConsoleConflict::DetachedWithNewConsole),
            1 => Conflict::Console(ConsoleConflict::NoWindowWithConsoleMode),
            2 => Conflict::Console(ConsoleConflict::ConsoleModeWithPseudoConsole),
            3 => Conflict::PseudoConsoleWithExplicitStdio,
            4 => Conflict::OutputWithPseudoConsole,
            _ => Conflict::ParentWithoutExplicitStdio,
        }
    }

    #[kani::proof]
    #[kani::unwind(80)]
    fn each_conflict_reports_its_own_message() {
        let first = conflict();
        let second = conflict();
        assert!(!first.message().is_empty());
        assert_eq!(first == second, first.message() == second.message());
        kani::cover!(first != second);
    }

    #[kani::proof]
    fn negative_control_parent_with_inherited_stdio() {
        let request = Request {
            configured: Streams {
                stdin: configured(),
                stdout: configured(),
                stderr: configured(),
            },
            flags: 0,
            pseudoconsole: false,
            parent: true,
            mode: mode(),
        };
        assert!(plan(request).is_ok(), "NEGATIVE_PLAN_CONTROL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::creation_flag_rules::{DETACHED_PROCESS, NEW_CONSOLE};

    fn request(configured: Streams<Option<u8>>, mode: IoMode) -> Request<u8> {
        Request {
            configured,
            flags: 0,
            pseudoconsole: false,
            parent: false,
            mode,
        }
    }

    const NONE: Streams<Option<u8>> = Streams {
        stdin: None,
        stdout: None,
        stderr: None,
    };

    #[test]
    fn defaults_follow_the_io_mode() {
        assert_eq!(
            plan(request(NONE, IoMode::Output)),
            Ok(StandardIo::Ordinary(Streams {
                stdin: Source::Null,
                stdout: Source::Piped,
                stderr: Source::Piped,
            }))
        );
        let partial = Streams {
            stdin: Some(7),
            ..NONE
        };
        assert_eq!(
            plan(request(partial, IoMode::Spawn)),
            Ok(StandardIo::Ordinary(Streams {
                stdin: Source::Configured(7),
                stdout: Source::Inherit,
                stderr: Source::Inherit,
            }))
        );
    }

    #[test]
    fn conflicts_carry_their_messages() {
        let console = Request {
            flags: DETACHED_PROCESS | NEW_CONSOLE,
            ..request(NONE, IoMode::Spawn)
        };
        let error = plan(console).unwrap_err();
        assert_eq!(
            error.message(),
            "DETACHED_PROCESS and NEW_CONSOLE are mutually exclusive"
        );
        let pseudoconsole = Request {
            pseudoconsole: true,
            ..request(NONE, IoMode::Output)
        };
        assert_eq!(plan(pseudoconsole), Err(Conflict::OutputWithPseudoConsole));
        let parent = Request {
            parent: true,
            ..request(NONE, IoMode::Spawn)
        };
        assert_eq!(plan(parent), Err(Conflict::ParentWithoutExplicitStdio));
        let mut messages: Vec<&str> = [
            Conflict::Console(ConsoleConflict::DetachedWithNewConsole),
            Conflict::Console(ConsoleConflict::NoWindowWithConsoleMode),
            Conflict::Console(ConsoleConflict::ConsoleModeWithPseudoConsole),
            Conflict::PseudoConsoleWithExplicitStdio,
            Conflict::OutputWithPseudoConsole,
            Conflict::ParentWithoutExplicitStdio,
        ]
        .into_iter()
        .map(Conflict::message)
        .collect();
        messages.sort_unstable();
        messages.dedup();
        assert_eq!(messages.len(), 6);
    }
}
