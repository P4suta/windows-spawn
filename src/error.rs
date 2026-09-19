use std::fmt;
use std::io;

/// The result type returned by windows-spawn operations.
pub type Result<T> = std::result::Result<T, Error>;

/// A process-spawn failure with its validation, Windows, or cleanup category preserved.
#[derive(Debug)]
pub enum Error {
    /// Input was rejected before operating-system resources were acquired.
    Validation(ValidationError),
    /// A Windows operation failed.
    Windows(WindowsError),
    /// One or more resource-reclamation operations failed.
    Cleanup(CleanupError),
}

impl Error {
    pub(crate) fn windows(phase: Phase, operation: Operation, source: io::Error) -> Self {
        Self::Windows(WindowsError::new(phase, operation, source))
    }

    fn io_kind(&self) -> io::ErrorKind {
        match self {
            Self::Validation(ValidationError::UnexpectedSuspendCount { .. }) => {
                io::ErrorKind::InvalidData
            }
            Self::Validation(_) => io::ErrorKind::InvalidInput,
            Self::Windows(error) => error.source.kind(),
            Self::Cleanup(_) => io::ErrorKind::Other,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(formatter),
            Self::Windows(error) => error.fmt(formatter),
            Self::Cleanup(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Windows(error) => Some(error),
            Self::Cleanup(error) => Some(error),
        }
    }
}

impl From<Error> for io::Error {
    fn from(error: Error) -> Self {
        Self::new(error.io_kind(), error)
    }
}

/// The phase in which a Windows operation was attempted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Phase {
    /// Resources needed for creation were being prepared.
    Preparation,
    /// The process was being created in its initial suspended state.
    Creation,
    /// Temporary resources were being reclaimed before execution.
    Reclamation,
    /// The primary thread was being resumed.
    Resume,
    /// A running process was being observed or controlled.
    Runtime,
    /// Resources were being explicitly cleaned up.
    Cleanup,
}

/// The Windows operation associated with a failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Operation {
    /// A handle was duplicated in the current process.
    DuplicateLocalHandle,
    /// A handle was duplicated into an alternate parent process.
    DuplicateRemoteHandle,
    /// A remotely duplicated handle was reclaimed.
    ReclaimRemoteHandle,
    /// A process handle was opened.
    OpenProcess,
    /// A Job object was created.
    CreateJob,
    /// Job limits were queried or changed.
    ConfigureJob,
    /// A process was assigned to a Job.
    AssignJob,
    /// A Job was terminated.
    TerminateJob,
    /// An anonymous pipe was created.
    CreatePipe,
    /// The null device or another file was opened.
    OpenFile,
    /// A process-thread attribute list was created or populated.
    ConfigureAttributes,
    /// An executable path was resolved.
    ResolveExecutable,
    /// The process was created.
    CreateProcess,
    /// A process was terminated.
    TerminateProcess,
    /// The primary thread was resumed.
    ResumeThread,
    /// A process wait was performed.
    WaitProcess,
    /// A process exit code was queried.
    QueryExitCode,
    /// A pipe was read.
    ReadPipe,
    /// A pipe was written.
    WritePipe,
    /// An output reader thread failed.
    JoinOutputReader,
    /// The process environment was read.
    ReadEnvironment,
    /// A system directory was queried.
    QuerySystemDirectory,
    /// A checked Windows command line was encoded.
    BuildCommandLine,
}

/// A validated description of why user input was rejected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// The program name was empty.
    EmptyProgram,
    /// A Windows string contained an interior NUL code unit.
    InteriorNul {
        /// The input field containing the NUL.
        field: InputField,
    },
    /// The program name contained a double quote.
    QuotedProgram,
    /// The program path had no file name.
    MissingProgramFileName,
    /// A batch file was supplied without an explicit command shell.
    BatchFile,
    /// An environment-variable name was empty.
    EmptyEnvironmentName,
    /// An environment-variable name contained `=`.
    InvalidEnvironmentName,
    /// A pseudoconsole was combined with ordinary standard I/O.
    PseudoConsoleWithStdio,
    /// Output capture was requested for pseudoconsole-owned streams.
    PseudoConsoleWithOutputCapture,
    /// An alternate parent was used without three explicit standard streams.
    AlternateParentNeedsStdio,
    /// The primary thread did not have the expected initial suspend count.
    UnexpectedSuspendCount {
        /// The suspend count returned by Windows.
        actual: u32,
    },
    /// A checked size calculation exceeded its supported range.
    SizeOverflow,
    /// The encoded command line exceeded the Windows process-creation limit.
    CommandLineTooLong,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("program must not be empty"),
            Self::InteriorNul { field } => write!(formatter, "{field} contains an interior NUL"),
            Self::QuotedProgram => formatter.write_str("program must not contain a double quote"),
            Self::MissingProgramFileName => formatter.write_str("program path has no file name"),
            Self::BatchFile => {
                formatter.write_str("batch files must be invoked through an explicit command shell")
            }
            Self::EmptyEnvironmentName => {
                formatter.write_str("environment variable name must not be empty")
            }
            Self::InvalidEnvironmentName => {
                formatter.write_str("environment variable name must not contain `=`")
            }
            Self::PseudoConsoleWithStdio => {
                formatter.write_str("a pseudoconsole conflicts with explicit standard I/O")
            }
            Self::PseudoConsoleWithOutputCapture => {
                formatter.write_str("output capture conflicts with pseudoconsole standard I/O")
            }
            Self::AlternateParentNeedsStdio => formatter.write_str(
                "an alternate parent requires all three standard streams to be explicit",
            ),
            Self::UnexpectedSuspendCount { actual } => write!(
                formatter,
                "primary thread suspend count was {actual}, expected 1"
            ),
            Self::SizeOverflow => formatter.write_str("a checked size calculation overflowed"),
            Self::CommandLineTooLong => {
                formatter.write_str("the encoded command line exceeds the Windows limit")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// A field accepted as Windows text.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum InputField {
    /// The executable program.
    Program,
    /// A command-line argument.
    Argument,
    /// An environment-variable name.
    EnvironmentName,
    /// An environment-variable value.
    EnvironmentValue,
    /// The current directory.
    CurrentDirectory,
}

impl fmt::Display for InputField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Program => "program",
            Self::Argument => "argument",
            Self::EnvironmentName => "environment variable name",
            Self::EnvironmentValue => "environment value",
            Self::CurrentDirectory => "current directory",
        };
        formatter.write_str(name)
    }
}

/// A Windows failure annotated with its phase and operation.
#[derive(Debug)]
pub struct WindowsError {
    phase: Phase,
    operation: Operation,
    code: u32,
    source: io::Error,
}

impl WindowsError {
    pub(crate) fn new(phase: Phase, operation: Operation, source: io::Error) -> Self {
        let code = source
            .raw_os_error()
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
        Self {
            phase,
            operation,
            code,
            source,
        }
    }

    /// Returns the phase in which the failure occurred.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns the operation that failed.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    /// Returns the Win32 error code, or zero for a non-Win32 I/O failure.
    #[must_use]
    pub const fn code(&self) -> u32 {
        self.code
    }
}

impl fmt::Display for WindowsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} failed during {:?} with Win32 code {}: {}",
            self.operation, self.phase, self.code, self.source
        )
    }
}

impl std::error::Error for WindowsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// A nonempty collection of resource-reclamation failures.
#[derive(Debug)]
pub struct CleanupError {
    primary: Option<WindowsError>,
    first: WindowsError,
    rest: Vec<WindowsError>,
}

impl CleanupError {
    pub(crate) fn new(first: WindowsError) -> Self {
        Self {
            primary: None,
            first,
            rest: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, error: WindowsError) {
        self.rest.push(error);
    }

    pub(crate) fn set_primary(&mut self, primary: WindowsError) {
        self.primary = Some(primary);
    }

    /// Returns the primary operation failure that preceded cleanup, if any.
    #[must_use]
    pub const fn primary(&self) -> Option<&WindowsError> {
        self.primary.as_ref()
    }

    /// Iterates over every cleanup failure in occurrence order.
    pub fn failures(&self) -> impl Iterator<Item = &WindowsError> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(primary) = &self.primary {
            write!(
                formatter,
                "primary failure: {primary}; {} cleanup operation(s) failed; first cleanup failure: {}",
                self.rest.len() + 1,
                self.first
            )
        } else {
            write!(
                formatter,
                "{} cleanup operation(s) failed; first failure: {}",
                self.rest.len() + 1,
                self.first
            )
        }
    }
}

impl std::error::Error for CleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.first)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::error::Error as _;
    use std::io;

    use super::*;

    #[test]
    fn validation_messages_and_input_fields_cover_the_closed_sets() {
        let fields = [
            InputField::Program,
            InputField::Argument,
            InputField::EnvironmentName,
            InputField::EnvironmentValue,
            InputField::CurrentDirectory,
        ];
        for field in fields {
            assert!(!field.to_string().is_empty());
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
            ValidationError::PseudoConsoleWithStdio,
            ValidationError::PseudoConsoleWithOutputCapture,
            ValidationError::AlternateParentNeedsStdio,
            ValidationError::UnexpectedSuspendCount { actual: 2 },
            ValidationError::SizeOverflow,
            ValidationError::CommandLineTooLong,
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
            assert!(error.source().is_none());
        }
    }

    #[test]
    fn typed_errors_preserve_kind_source_phase_operation_and_code() {
        let validation = Error::Validation(ValidationError::EmptyProgram);
        assert_eq!(validation.io_kind(), io::ErrorKind::InvalidInput);
        assert!(validation.source().is_some());
        assert!(!validation.to_string().is_empty());

        let suspend = Error::Validation(ValidationError::UnexpectedSuspendCount { actual: 3 });
        assert_eq!(suspend.io_kind(), io::ErrorKind::InvalidData);

        let windows = WindowsError::new(
            Phase::Creation,
            Operation::CreateProcess,
            io::Error::from_raw_os_error(5),
        );
        assert_eq!(windows.phase(), Phase::Creation);
        assert_eq!(windows.operation(), Operation::CreateProcess);
        assert_eq!(windows.code(), 5);
        assert!(windows.source().is_some());
        assert!(!windows.to_string().is_empty());
        let windows = Error::Windows(windows);
        assert_eq!(windows.io_kind(), io::ErrorKind::PermissionDenied);
        assert!(windows.source().is_some());
        assert!(!windows.to_string().is_empty());

        let no_code = WindowsError::new(
            Phase::Runtime,
            Operation::WaitProcess,
            io::Error::other("synthetic"),
        );
        assert_eq!(no_code.code(), 0);
    }

    #[test]
    fn cleanup_errors_are_nonempty_and_retain_optional_primary_failure() {
        let first = WindowsError::new(
            Phase::Reclamation,
            Operation::ReclaimRemoteHandle,
            io::Error::from_raw_os_error(5),
        );
        let mut cleanup = CleanupError::new(first);
        cleanup.push(WindowsError::new(
            Phase::Cleanup,
            Operation::TerminateJob,
            io::Error::from_raw_os_error(6),
        ));
        assert_eq!(cleanup.failures().count(), 2);
        assert!(cleanup.primary().is_none());
        assert!(!cleanup.to_string().is_empty());
        cleanup.set_primary(WindowsError::new(
            Phase::Creation,
            Operation::CreateProcess,
            io::Error::from_raw_os_error(87),
        ));
        assert_eq!(
            cleanup.primary().map(WindowsError::operation),
            Some(Operation::CreateProcess)
        );
        assert!(cleanup.source().is_some());
        assert!(!cleanup.to_string().is_empty());

        let error = Error::Cleanup(cleanup);
        assert_eq!(error.io_kind(), io::ErrorKind::Other);
        assert!(error.source().is_some());
        assert!(!error.to_string().is_empty());
        let converted = io::Error::from(error);
        assert_eq!(converted.kind(), io::ErrorKind::Other);
        assert!(converted.get_ref().is_some());
    }
}
