//! The typed failures of every fallible operation: rejected input, a failed Windows call, and failed cleanup.

use std::fmt;
use std::io;

/// The result type of windows-spawn operations.
pub type Result<T> = std::result::Result<T, SpawnError>;

/// A failed operation, with its validation, Windows, or cleanup category kept.
///
/// The message is complete on its own, so printing the error, or a chain of errors that holds it, shows each cause once.
/// [`Self::kind`] and [`Self::raw_os_error`] answer what they answer for [`io::Error`].
///
/// It converts into [`io::Error`], so `?` works in a function that returns [`io::Result`]:
/// a Windows failure becomes the [`io::Error`] Windows reported, as [`std::process::Command`] would return it,
/// and any other failure becomes the payload of an [`io::Error`] of the same kind.
#[derive(Debug)]
pub enum SpawnError {
    /// The input was rejected, and no process was created.
    ///
    /// A spawn finds an overlong command line only once it has lowered the line's handles,
    /// so by then it has acquired its pipes and duplicates, and it releases them before it returns.
    Validation(ValidationError),
    /// A Windows operation failed, or the crate found Windows in a state it can't continue from.
    Windows(WindowsError),
    /// Releasing resources failed, after a failure or after the operation itself succeeded.
    Cleanup(CleanupError),
}

impl SpawnError {
    pub(crate) const fn windows(phase: Phase, operation: Operation, source: io::Error) -> Self {
        Self::Windows(WindowsError::new(phase, operation, source))
    }

    /// The error for a main thread whose previous suspend count was `previous` instead of one.
    pub(crate) fn unexpected_suspend_count(previous: u32) -> Self {
        Self::windows(
            Phase::Resume,
            Operation::ResumeThread,
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the main thread's suspend count was {previous}, expected 1"),
            ),
        )
    }

    /// Returns the [`io::ErrorKind`] this error converts to.
    ///
    /// Rejected input is [`io::ErrorKind::InvalidInput`].
    /// A Windows failure has the kind of its [`io::Error`]: a program the search didn't find is [`io::ErrorKind::NotFound`],
    /// and a main thread that another thread suspended again is [`io::ErrorKind::InvalidData`].
    /// A cleanup failure has the kind of [`CleanupError::primary`], or else of its first release failure.
    #[must_use]
    pub fn kind(&self) -> io::ErrorKind {
        match self {
            Self::Validation(_) => io::ErrorKind::InvalidInput,
            Self::Windows(error) => error.source.kind(),
            Self::Cleanup(error) => error
                .primary()
                .map_or_else(|| error.first.source.kind(), Self::kind),
        }
    }

    /// Returns the Win32 error code, as [`io::Error::raw_os_error`] does.
    ///
    /// A cleanup failure returns the code of [`CleanupError::primary`], or else of its first release failure.
    /// Rejected input, and a failure the crate detected itself, such as a program the search didn't find, have none.
    #[must_use]
    pub fn raw_os_error(&self) -> Option<i32> {
        match self {
            Self::Validation(_) => None,
            Self::Windows(error) => error.raw_os_error(),
            Self::Cleanup(error) => error
                .primary()
                .map_or_else(|| error.first.raw_os_error(), Self::raw_os_error),
        }
    }
}

impl From<ValidationError> for SpawnError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Windows(error) => error.fmt(formatter),
            Self::Cleanup(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SpawnError {}

impl From<SpawnError> for io::Error {
    fn from(error: SpawnError) -> Self {
        match error {
            SpawnError::Windows(windows) => windows.source,
            other @ (SpawnError::Validation(_) | SpawnError::Cleanup(_)) => {
                Self::new(other.kind(), other)
            }
        }
    }
}

/// The phase of a spawn, or of a process's life, in which a Windows operation ran.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Phase {
    /// A capability was being created or configured, or a spawn was acquiring its resources.
    Preparation,
    /// The process was being created, suspended.
    Creation,
    /// A spawn was releasing its temporary resources, before the process ran.
    Reclamation,
    /// The main thread was being resumed.
    Resume,
    /// A created process, or a Job, was being observed or controlled.
    Runtime,
}

impl fmt::Display for Phase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Preparation => "preparation",
            Self::Creation => "creation",
            Self::Reclamation => "reclamation",
            Self::Resume => "resume",
            Self::Runtime => "runtime",
        })
    }
}

/// The Windows operation that failed.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Operation {
    /// A handle was duplicated in the current process.
    DuplicateLocalHandle,
    /// A handle was duplicated into an alternate parent process.
    DuplicateRemoteHandle,
    /// A handle duplicated into an alternate parent was closed there.
    ReclaimRemoteHandle,
    /// A process handle was opened or checked.
    OpenProcess,
    /// A Job object was created.
    CreateJob,
    /// A Job's limits or processes were queried.
    QueryJob,
    /// A Job's limits were changed.
    ConfigureJob,
    /// A process was assigned to a Job.
    AssignJob,
    /// A Job was terminated.
    TerminateJob,
    /// An anonymous pipe was created.
    CreatePipe,
    /// The null device was opened.
    OpenNullDevice,
    /// A standard handle of the current process was read.
    GetStandardHandle,
    /// A process-thread attribute list was created or filled.
    ConfigureAttributes,
    /// The program was searched for, or its path made absolute.
    ResolveExecutable,
    /// The process was created.
    CreateProcess,
    /// A process was terminated.
    TerminateProcess,
    /// The main thread was resumed.
    ResumeThread,
    /// A process was waited for.
    WaitProcess,
    /// A process's exit code was queried.
    QueryExitCode,
    /// A pipe was read.
    ReadPipe,
    /// A pipe was written.
    WritePipe,
    /// An output reader thread failed.
    JoinOutputReader,
    /// The process environment was read.
    ReadEnvironment,
}

impl fmt::Display for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DuplicateLocalHandle => "duplicating a handle",
            Self::DuplicateRemoteHandle => "duplicating a handle into the parent process",
            Self::ReclaimRemoteHandle => "closing a duplicate in the parent process",
            Self::OpenProcess => "opening a process",
            Self::CreateJob => "creating a Job",
            Self::QueryJob => "querying a Job",
            Self::ConfigureJob => "configuring a Job",
            Self::AssignJob => "assigning a process to a Job",
            Self::TerminateJob => "terminating a Job",
            Self::CreatePipe => "creating a pipe",
            Self::OpenNullDevice => "opening the null device",
            Self::GetStandardHandle => "reading a standard handle",
            Self::ConfigureAttributes => "configuring process attributes",
            Self::ResolveExecutable => "searching for the program",
            Self::CreateProcess => "creating the process",
            Self::TerminateProcess => "terminating the process",
            Self::ResumeThread => "resuming the main thread",
            Self::WaitProcess => "waiting for the process",
            Self::QueryExitCode => "reading the exit code",
            Self::ReadPipe => "reading a pipe",
            Self::WritePipe => "writing a pipe",
            Self::JoinOutputReader => "collecting output",
            Self::ReadEnvironment => "reading the environment",
        })
    }
}

/// Why the input was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// The program name was empty.
    EmptyProgram,
    /// A Windows string held a NUL unit inside it.
    InteriorNul {
        /// The input that held the NUL.
        field: InputField,
    },
    /// The program, or the absolute path it resolved to, held a double quote.
    QuotedProgram,
    /// The program path had no file name.
    MissingProgramFileName,
    /// A batch file was named as the program.
    ///
    /// `cmd.exe` parses a batch file's arguments by rules no quoting makes safe for arbitrary text, so the crate runs no batch file itself.
    /// Name the shell instead, as the crate documentation's batch-file recipe shows.
    BatchFile,
    /// An environment-variable name was empty.
    EmptyEnvironmentName,
    /// An environment-variable name held `=`.
    InvalidEnvironmentName,
    /// A console mode was chosen for a child that a pseudoconsole hosts.
    PseudoConsoleWithConsoleMode,
    /// A pseudoconsole was combined with configured standard I/O.
    PseudoConsoleWithStdio,
    /// Output capture was requested for streams a pseudoconsole owns.
    PseudoConsoleWithOutputCapture,
    /// An alternate parent was used without three configured standard streams.
    AlternateParentNeedsStdio,
    /// A handle's value exists only inside a spawn, so a rendered command line can't hold it.
    HandleNeedsSpawn {
        /// The input that held the handle.
        field: InputField,
    },
    /// The command line was longer than `CreateProcessW` accepts: 32,766 UTF-16 units before its terminator.
    CommandLineTooLong,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("program must not be empty"),
            Self::InteriorNul { field } => write!(formatter, "{field} contains an interior NUL"),
            Self::QuotedProgram => formatter.write_str("program must not contain a double quote"),
            Self::MissingProgramFileName => formatter.write_str("program path has no file name"),
            Self::BatchFile => formatter.write_str(
                "a batch file runs only through a shell the command names, such as `cmd.exe /D /S /C`",
            ),
            Self::EmptyEnvironmentName => {
                formatter.write_str("environment variable name must not be empty")
            }
            Self::InvalidEnvironmentName => {
                formatter.write_str("environment variable name must not contain `=`")
            }
            Self::PseudoConsoleWithConsoleMode => formatter
                .write_str("a pseudoconsole is the child's console, so no console mode may be chosen"),
            Self::PseudoConsoleWithStdio => {
                formatter.write_str("a pseudoconsole conflicts with explicit standard I/O")
            }
            Self::PseudoConsoleWithOutputCapture => {
                formatter.write_str("output capture conflicts with pseudoconsole standard I/O")
            }
            Self::AlternateParentNeedsStdio => formatter.write_str(
                "a child of an alternate parent can't inherit this process's standard streams; \
                 configure stdin, stdout, and stderr, for example with `Stdio::null()`",
            ),
            Self::HandleNeedsSpawn { field } => {
                write!(
                    formatter,
                    "a handle {field} has a value only inside a spawn"
                )
            }
            Self::CommandLineTooLong => formatter
                .write_str("command line exceeds the 32,766 UTF-16 units `CreateProcessW` accepts"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// An input that the crate passes to Windows as text.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum InputField {
    /// The program.
    Program,
    /// A command-line argument.
    Argument,
    /// An environment-variable name.
    EnvironmentName,
    /// An environment-variable value.
    EnvironmentValue,
    /// The working directory.
    CurrentDirectory,
}

impl fmt::Display for InputField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Program => "program",
            Self::Argument => "argument",
            Self::EnvironmentName => "environment variable name",
            Self::EnvironmentValue => "environment value",
            Self::CurrentDirectory => "current directory",
        })
    }
}

/// A failed Windows operation, with the phase it ran in.
///
/// The message holds the [`io::Error`] message, which [`Self::io_error`] returns.
#[derive(Debug)]
pub struct WindowsError {
    phase: Phase,
    operation: Operation,
    source: io::Error,
}

impl WindowsError {
    pub(crate) const fn new(phase: Phase, operation: Operation, source: io::Error) -> Self {
        Self {
            phase,
            operation,
            source,
        }
    }

    /// Returns the phase in which the operation ran.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns the operation that failed.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// Returns the Win32 error code, or `None` for a failure the crate detected itself.
    #[must_use]
    pub fn raw_os_error(&self) -> Option<i32> {
        self.source.raw_os_error()
    }

    /// Returns the underlying I/O error.
    #[must_use]
    pub const fn io_error(&self) -> &io::Error {
        &self.source
    }
}

impl fmt::Display for WindowsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} failed: {}", self.operation, self.source)
    }
}

impl std::error::Error for WindowsError {}

/// The failures of the operations that released resources, never empty, with the failure that made the release necessary.
#[derive(Debug)]
pub struct CleanupError {
    primary: Option<Box<SpawnError>>,
    first: WindowsError,
    rest: Vec<WindowsError>,
}

impl CleanupError {
    const fn new(first: WindowsError) -> Self {
        Self {
            primary: None,
            first,
            rest: Vec::new(),
        }
    }

    fn push(&mut self, error: WindowsError) {
        self.rest.push(error);
    }

    /// Keeps `primary` as the failure that made the release necessary.
    ///
    /// A primary that is itself a cleanup failure gives its own primary, and its release failures come first.
    fn attach(&mut self, primary: SpawnError) {
        match primary {
            SpawnError::Cleanup(earlier) => {
                let later = std::mem::replace(&mut self.first, earlier.first);
                let mut rest = earlier.rest;
                rest.push(later);
                rest.append(&mut self.rest);
                self.rest = rest;
                self.primary = earlier.primary;
            }
            primary @ (SpawnError::Validation(_) | SpawnError::Windows(_)) => {
                self.primary = Some(Box::new(primary));
            }
        }
    }

    /// Returns the failure that made the release necessary, if one did.
    #[must_use]
    pub fn primary(&self) -> Option<&SpawnError> {
        self.primary.as_deref()
    }

    /// Iterates over every release failure in the order they happened.
    pub fn failures(&self) -> impl Iterator<Item = &WindowsError> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(primary) = &self.primary {
            write!(
                formatter,
                "{primary}; releasing resources afterwards also failed: "
            )?;
        } else {
            formatter.write_str("releasing resources failed: ")?;
        }
        self.first.fmt(formatter)?;
        match self.rest.len() {
            0 => Ok(()),
            more => write!(formatter, " (and {more} more)"),
        }
    }
}

impl std::error::Error for CleanupError {}

/// Collects release failures, so that every release runs and none of their failures is lost.
#[derive(Debug, Default)]
pub(crate) struct Releases {
    failures: Option<CleanupError>,
}

impl Releases {
    /// Records the outcome of one release.
    pub(crate) fn record(&mut self, phase: Phase, operation: Operation, outcome: io::Result<()>) {
        if let Err(source) = outcome {
            let error = WindowsError::new(phase, operation, source);
            match &mut self.failures {
                Some(failures) => failures.push(error),
                None => self.failures = Some(CleanupError::new(error)),
            }
        }
    }

    /// Returns the collected failures after an operation that succeeded.
    pub(crate) fn finish(self) -> Result<()> {
        self.failures
            .map_or(Ok(()), |failures| Err(SpawnError::Cleanup(failures)))
    }

    /// Returns the error to report after `primary` failed: `primary` alone, or the collected failures with it.
    pub(crate) fn after(self, primary: SpawnError) -> SpawnError {
        match self.failures {
            None => primary,
            Some(mut failures) => {
                failures.attach(primary);
                SpawnError::Cleanup(failures)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    const ACCESS_DENIED: i32 = 5;
    const INVALID_HANDLE: i32 = 6;
    const INVALID_PARAMETER: i32 = 87;

    #[test]
    fn every_validation_error_input_field_phase_and_operation_has_a_message() {
        let fields = [
            InputField::Program,
            InputField::Argument,
            InputField::EnvironmentName,
            InputField::EnvironmentValue,
            InputField::CurrentDirectory,
        ];
        for field in fields {
            assert_ne!(field.to_string(), "");
        }
        let errors = [
            ValidationError::EmptyProgram,
            ValidationError::InteriorNul {
                field: InputField::Program,
            },
            ValidationError::QuotedProgram,
            ValidationError::MissingProgramFileName,
            ValidationError::BatchFile,
            ValidationError::EmptyEnvironmentName,
            ValidationError::InvalidEnvironmentName,
            ValidationError::PseudoConsoleWithConsoleMode,
            ValidationError::PseudoConsoleWithStdio,
            ValidationError::PseudoConsoleWithOutputCapture,
            ValidationError::AlternateParentNeedsStdio,
            ValidationError::HandleNeedsSpawn {
                field: InputField::Argument,
            },
            ValidationError::CommandLineTooLong,
        ];
        for error in errors {
            assert_ne!(error.to_string(), "");
            assert!(error.source().is_none());
        }
        let phases = [
            Phase::Preparation,
            Phase::Creation,
            Phase::Reclamation,
            Phase::Resume,
            Phase::Runtime,
        ];
        for phase in phases {
            assert_ne!(phase.to_string(), "");
        }
        let operations = [
            Operation::DuplicateLocalHandle,
            Operation::DuplicateRemoteHandle,
            Operation::ReclaimRemoteHandle,
            Operation::OpenProcess,
            Operation::CreateJob,
            Operation::QueryJob,
            Operation::ConfigureJob,
            Operation::AssignJob,
            Operation::TerminateJob,
            Operation::CreatePipe,
            Operation::OpenNullDevice,
            Operation::GetStandardHandle,
            Operation::ConfigureAttributes,
            Operation::ResolveExecutable,
            Operation::CreateProcess,
            Operation::TerminateProcess,
            Operation::ResumeThread,
            Operation::WaitProcess,
            Operation::QueryExitCode,
            Operation::ReadPipe,
            Operation::WritePipe,
            Operation::JoinOutputReader,
            Operation::ReadEnvironment,
        ];
        for operation in operations {
            assert_ne!(operation.to_string(), "");
        }
    }

    #[test]
    fn typed_errors_keep_kind_code_phase_and_operation_and_print_each_cause_once() {
        let validation = SpawnError::from(ValidationError::EmptyProgram);
        assert_eq!(validation.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(validation.raw_os_error(), None);
        assert!(validation.source().is_none());
        assert_eq!(validation.to_string(), "program must not be empty");
        let rejected = io::Error::from(validation);
        assert_eq!(rejected.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(rejected.to_string(), "program must not be empty");

        let suspend = SpawnError::unexpected_suspend_count(2);
        assert_eq!(suspend.kind(), io::ErrorKind::InvalidData);
        assert_eq!(suspend.raw_os_error(), None);
        assert!(matches!(
            &suspend,
            SpawnError::Windows(error) if error.operation() == Operation::ResumeThread
                && error.phase() == Phase::Resume
        ));
        assert_eq!(
            suspend.to_string(),
            "resuming the main thread failed: the main thread's suspend count was 2, expected 1"
        );

        let windows = WindowsError::new(
            Phase::Creation,
            Operation::CreateProcess,
            io::Error::from_raw_os_error(ACCESS_DENIED),
        );
        assert_eq!(windows.phase(), Phase::Creation);
        assert_eq!(windows.operation(), Operation::CreateProcess);
        assert_eq!(windows.raw_os_error(), Some(ACCESS_DENIED));
        assert_eq!(windows.io_error().raw_os_error(), Some(ACCESS_DENIED));
        assert!(windows.source().is_none());
        let message = windows.to_string();
        assert!(message.starts_with("creating the process failed: "));
        assert!(message.ends_with(&format!("(os error {ACCESS_DENIED})")));
        let windows = SpawnError::Windows(windows);
        assert_eq!(windows.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(windows.raw_os_error(), Some(ACCESS_DENIED));
        assert!(windows.source().is_none());

        let reported = io::Error::from(windows);
        assert_eq!(reported.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(reported.raw_os_error(), Some(ACCESS_DENIED));

        let uncoded = SpawnError::windows(
            Phase::Preparation,
            Operation::ResolveExecutable,
            io::Error::new(io::ErrorKind::NotFound, "program not found"),
        );
        assert_eq!(uncoded.kind(), io::ErrorKind::NotFound);
        assert_eq!(uncoded.raw_os_error(), None);
        assert_eq!(
            uncoded.to_string(),
            "searching for the program failed: program not found"
        );
        let missing = io::Error::from(uncoded);
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert_eq!(missing.to_string(), "program not found");
    }

    fn failed(phase: Phase, operation: Operation, code: i32) -> Releases {
        let mut releases = Releases::default();
        releases.record(phase, operation, Ok(()));
        releases.record(phase, operation, Err(io::Error::from_raw_os_error(code)));
        releases
    }

    fn creation_failure() -> SpawnError {
        SpawnError::windows(
            Phase::Creation,
            Operation::CreateProcess,
            io::Error::from_raw_os_error(INVALID_PARAMETER),
        )
    }

    fn operations(cleanup: &CleanupError) -> Vec<Operation> {
        cleanup.failures().map(WindowsError::operation).collect()
    }

    #[test]
    fn releases_keep_every_failure_and_the_primary_one() {
        assert!(Releases::default().finish().is_ok());
        assert!(matches!(
            Releases::default().after(creation_failure()),
            SpawnError::Windows(_)
        ));

        let mut releases = failed(
            Phase::Reclamation,
            Operation::ReclaimRemoteHandle,
            ACCESS_DENIED,
        );
        releases.record(
            Phase::Reclamation,
            Operation::DuplicateLocalHandle,
            Err(io::Error::from_raw_os_error(INVALID_HANDLE)),
        );
        let error = releases.after(creation_failure());
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.raw_os_error(), Some(INVALID_PARAMETER));
        let SpawnError::Cleanup(cleanup) = error else {
            panic!("release failures after a Windows failure become a cleanup error");
        };
        assert_eq!(
            operations(&cleanup),
            [
                Operation::ReclaimRemoteHandle,
                Operation::DuplicateLocalHandle
            ]
        );
        assert!(matches!(
            cleanup.primary(),
            Some(SpawnError::Windows(primary)) if primary.operation() == Operation::CreateProcess
        ));
        assert!(cleanup.source().is_none());
        let message = cleanup.to_string();
        assert!(message.starts_with("creating the process failed: "));
        assert!(
            message.contains("; releasing resources afterwards also failed: closing a duplicate")
        );
        assert!(message.ends_with(" (and 1 more)"));
        let converted = io::Error::from(SpawnError::Cleanup(cleanup));
        assert_eq!(converted.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(converted.raw_os_error(), None);

        let Err(SpawnError::Cleanup(alone)) = failed(
            Phase::Reclamation,
            Operation::ReclaimRemoteHandle,
            ACCESS_DENIED,
        )
        .finish() else {
            panic!("a release failure after a success becomes a cleanup error");
        };
        assert!(alone.primary().is_none());
        assert!(alone
            .to_string()
            .starts_with("releasing resources failed: closing a duplicate"));
        let alone = SpawnError::Cleanup(alone);
        assert_eq!(alone.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(alone.raw_os_error(), Some(ACCESS_DENIED));

        let SpawnError::Cleanup(kept) = failed(
            Phase::Reclamation,
            Operation::ReclaimRemoteHandle,
            ACCESS_DENIED,
        )
        .after(SpawnError::from(ValidationError::CommandLineTooLong)) else {
            panic!("release failures keep a primary that names no Windows call");
        };
        assert!(matches!(
            kept.primary(),
            Some(SpawnError::Validation(ValidationError::CommandLineTooLong))
        ));
        assert_eq!(
            SpawnError::Cleanup(kept).kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_cleanup_primary_merges_into_one_error() {
        let earlier = failed(
            Phase::Reclamation,
            Operation::ReclaimRemoteHandle,
            ACCESS_DENIED,
        )
        .after(creation_failure());
        let SpawnError::Cleanup(merged) = failed(
            Phase::Reclamation,
            Operation::DuplicateLocalHandle,
            INVALID_HANDLE,
        )
        .after(earlier) else {
            panic!("release failures after a cleanup failure stay a cleanup error");
        };
        assert_eq!(
            operations(&merged),
            [
                Operation::ReclaimRemoteHandle,
                Operation::DuplicateLocalHandle
            ]
        );
        assert!(matches!(merged.primary(), Some(SpawnError::Windows(_))));
    }
}
