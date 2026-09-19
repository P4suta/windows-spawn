use std::ffi::OsStr;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use crate::command::{Arg, Command, EnvOp, EnvValue};
use crate::error::{Error, InputField, Result, ValidationError};
use crate::handles::Stdio;
use crate::options::{SpawnOptions, TerminalMode};

#[derive(Debug)]
pub(crate) struct Running;

#[derive(Debug)]
pub(crate) struct Suspended;

mod sealed {
    pub(crate) trait Sealed {}

    impl Sealed for super::Running {}
    impl Sealed for super::Suspended {}
}

pub(crate) trait DesiredState: sealed::Sealed {}

impl DesiredState for Running {}
impl DesiredState for Suspended {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IoMode {
    Spawn,
    Output,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StdioSpec<'a> {
    Configured(&'a Stdio),
    Inherit,
    Null,
    Piped,
}

#[derive(Debug)]
pub(crate) struct StandardHandles<T> {
    pub(crate) stdin: T,
    pub(crate) stdout: T,
    pub(crate) stderr: T,
}

#[derive(Debug)]
pub(crate) enum StandardIo<'a> {
    Ordinary(StandardHandles<StdioSpec<'a>>),
    PseudoConsole,
}

#[derive(Debug)]
pub(crate) struct ValidatedPlan<'command, 'options, State> {
    pub(crate) command: &'command Command,
    pub(crate) options: SpawnOptions<'options>,
    pub(crate) stdio: StandardIo<'command>,
    state: PhantomData<State>,
}

impl<'command, 'options> ValidatedPlan<'command, 'options, Running> {
    pub(crate) fn running(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self> {
        Self::build(command, options, io_mode)
    }
}

impl<'command, 'options> ValidatedPlan<'command, 'options, Suspended> {
    pub(crate) fn suspended(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self> {
        Self::build(command, options, io_mode)
    }
}

impl<'command, 'options, State> ValidatedPlan<'command, 'options, State> {
    pub(crate) fn into_parts(
        self,
    ) -> (
        &'command Command,
        SpawnOptions<'options>,
        StandardIo<'command>,
    ) {
        let Self {
            command,
            options,
            stdio,
            state: _,
        } = self;
        (command, options, stdio)
    }

    fn build(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self> {
        validate_command(command)?;
        let pseudo_console = matches!(options.terminal, TerminalMode::PseudoConsole(_));
        let explicit_stdio =
            command.stdin.is_some() || command.stdout.is_some() || command.stderr.is_some();
        if pseudo_console && explicit_stdio {
            return Err(Error::Validation(ValidationError::PseudoConsoleWithStdio));
        }
        if pseudo_console && io_mode == IoMode::Output {
            return Err(Error::Validation(
                ValidationError::PseudoConsoleWithOutputCapture,
            ));
        }
        if options.parent.is_some()
            && !pseudo_console
            && (command.stdin.is_none() || command.stdout.is_none() || command.stderr.is_none())
        {
            return Err(Error::Validation(
                ValidationError::AlternateParentNeedsStdio,
            ));
        }
        let stdio = if pseudo_console {
            StandardIo::PseudoConsole
        } else {
            let handles = match io_mode {
                IoMode::Spawn => StandardHandles {
                    stdin: configured_or(command.stdin.as_ref(), StdioSpec::Inherit),
                    stdout: configured_or(command.stdout.as_ref(), StdioSpec::Inherit),
                    stderr: configured_or(command.stderr.as_ref(), StdioSpec::Inherit),
                },
                IoMode::Output => StandardHandles {
                    stdin: configured_or(command.stdin.as_ref(), StdioSpec::Null),
                    stdout: configured_or(command.stdout.as_ref(), StdioSpec::Piped),
                    stderr: configured_or(command.stderr.as_ref(), StdioSpec::Piped),
                },
            };
            StandardIo::Ordinary(handles)
        };
        Ok(Self {
            command,
            options,
            stdio,
            state: PhantomData,
        })
    }
}

fn configured_or<'a>(value: Option<&'a Stdio>, default: StdioSpec<'a>) -> StdioSpec<'a> {
    value.map_or(default, StdioSpec::Configured)
}

fn validate_command(command: &Command) -> Result<()> {
    if command.program.is_empty() {
        return Err(Error::Validation(ValidationError::EmptyProgram));
    }
    no_nul(&command.program, InputField::Program)?;
    if command.program.as_encoded_bytes().contains(&b'\"') {
        return Err(Error::Validation(ValidationError::QuotedProgram));
    }
    let program_path = Path::new(&command.program);
    if program_path.file_name().is_none() {
        return Err(Error::Validation(ValidationError::MissingProgramFileName));
    }
    if let Some(extension) = program_path.extension() {
        let extension = extension.as_encoded_bytes();
        if extension.eq_ignore_ascii_case(b"bat") || extension.eq_ignore_ascii_case(b"cmd") {
            return Err(Error::Validation(ValidationError::BatchFile));
        }
    }
    for arg in &command.args {
        match arg {
            Arg::Text(text) | Arg::Raw(text) => no_nul(text, InputField::Argument)?,
            Arg::Handle(_) => {}
        }
    }
    for operation in &command.env_ops {
        match operation {
            EnvOp::Set(key, value) => {
                validate_env_key(key)?;
                if let EnvValue::Text(value) = value {
                    no_nul(value, InputField::EnvironmentValue)?;
                }
            }
            EnvOp::Remove(key) => validate_env_key(key)?,
        }
    }
    if let Some(cwd) = &command.cwd {
        no_nul(cwd.as_os_str(), InputField::CurrentDirectory)?;
    }
    Ok(())
}

fn validate_env_key(key: &OsStr) -> Result<()> {
    if key.is_empty() {
        return Err(Error::Validation(ValidationError::EmptyEnvironmentName));
    }
    no_nul(key, InputField::EnvironmentName)?;
    if key.as_encoded_bytes().contains(&b'=') {
        return Err(Error::Validation(ValidationError::InvalidEnvironmentName));
    }
    Ok(())
}

fn no_nul(value: &OsStr, field: InputField) -> Result<()> {
    if value.encode_wide().any(|unit| unit == 0) {
        Err(Error::Validation(ValidationError::InteriorNul { field }))
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::OsStringExt;

    use super::*;
    use crate::handles::{borrowed_pseudoconsole_for_test, ParentProcess, Stdio};

    fn assert_validation(
        command: &Command,
        options: SpawnOptions<'_>,
        mode: IoMode,
        expected: ValidationError,
    ) {
        let result = ValidatedPlan::<Running>::running(command, options, mode);
        let Err(Error::Validation(actual)) = result else {
            panic!("validation error expected");
        };
        assert_eq!(actual, expected);
    }

    fn nul() -> OsString {
        OsString::from_wide(&[u16::from(b'x'), 0])
    }

    #[test]
    fn command_validation_rejects_every_invalid_text_shape() {
        assert_validation(
            &Command::new(""),
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::EmptyProgram,
        );
        assert_validation(
            &Command::new("bad\"name.exe"),
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::QuotedProgram,
        );
        assert_validation(
            &Command::new("C:\\"),
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::MissingProgramFileName,
        );
        assert_validation(
            &Command::new("script.cmd"),
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::BatchFile,
        );

        let mut argument = Command::new("program.exe");
        argument.arg(nul());
        assert_validation(
            &argument,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InteriorNul {
                field: InputField::Argument,
            },
        );
        let mut raw = Command::new("program.exe");
        raw.raw_arg(nul());
        assert_validation(
            &raw,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InteriorNul {
                field: InputField::Argument,
            },
        );

        let mut empty_name = Command::new("program.exe");
        empty_name.env("", "value");
        assert_validation(
            &empty_name,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::EmptyEnvironmentName,
        );
        let mut equals_name = Command::new("program.exe");
        equals_name.env("a=b", "value");
        assert_validation(
            &equals_name,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InvalidEnvironmentName,
        );
        let mut nul_name = Command::new("program.exe");
        nul_name.env(nul(), "value");
        assert_validation(
            &nul_name,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InteriorNul {
                field: InputField::EnvironmentName,
            },
        );
        let mut nul_value = Command::new("program.exe");
        nul_value.env("key", nul());
        assert_validation(
            &nul_value,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InteriorNul {
                field: InputField::EnvironmentValue,
            },
        );
        let mut removed = Command::new("program.exe");
        removed.env_remove(OsStr::new(""));
        assert_validation(
            &removed,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::EmptyEnvironmentName,
        );
        let mut directory = Command::new("program.exe");
        directory.current_dir(nul());
        assert_validation(
            &directory,
            SpawnOptions::new(),
            IoMode::Spawn,
            ValidationError::InteriorNul {
                field: InputField::CurrentDirectory,
            },
        );
    }

    #[test]
    fn capability_validation_rejects_conflicting_states() -> Result<()> {
        let owner = ();
        let terminal = TerminalMode::PseudoConsole(borrowed_pseudoconsole_for_test(&owner));
        let mut explicit = Command::new("program.exe");
        explicit.stdout(Stdio::null());
        assert_validation(
            &explicit,
            SpawnOptions::new().terminal(terminal),
            IoMode::Spawn,
            ValidationError::PseudoConsoleWithStdio,
        );
        assert_validation(
            &Command::new("program.exe"),
            SpawnOptions::new().terminal(terminal),
            IoMode::Output,
            ValidationError::PseudoConsoleWithOutputCapture,
        );

        let parent = ParentProcess::open(std::process::id())?;
        assert_validation(
            &Command::new("program.exe"),
            SpawnOptions::new().parent_process(&parent),
            IoMode::Spawn,
            ValidationError::AlternateParentNeedsStdio,
        );
        Ok(())
    }
}
