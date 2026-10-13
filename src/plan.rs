//! Pure normalization and validation before any OS resource is acquired.

use std::ffi::OsStr;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use crate::command::{Arg, Command, EnvOp, EnvValue};
use crate::domain::plan_rules::{self, Conflict, Request};
pub(crate) use crate::domain::plan_rules::{IoMode, Source, StandardIo, Streams};
use crate::error::{InputField, ValidationError};
use crate::handles::Stdio;
use crate::options::{ConsoleMode, SpawnOptions};

/// The state of a spawn that resumes the process before returning it.
#[derive(Debug)]
pub(crate) struct Running;

/// The state of a spawn that returns the process still suspended.
#[derive(Debug)]
pub(crate) struct Suspended;

#[derive(Debug)]
pub(crate) struct SpawnPlan<'command, 'options, M> {
    pub(crate) command: &'command Command,
    pub(crate) options: SpawnOptions<'options>,
    pub(crate) stdio: StandardIo<&'command Stdio>,
    state: PhantomData<M>,
}

impl<'command, 'options> SpawnPlan<'command, 'options, Running> {
    pub(crate) fn new_running(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self, ValidationError> {
        Self::build(command, options, io_mode)
    }
}

impl<'command, 'options> SpawnPlan<'command, 'options, Suspended> {
    pub(crate) fn new_suspended(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self, ValidationError> {
        Self::build(command, options, io_mode)
    }
}

impl<'command, 'options, M> SpawnPlan<'command, 'options, M> {
    fn build(
        command: &'command Command,
        options: SpawnOptions<'options>,
        io_mode: IoMode,
    ) -> Result<Self, ValidationError> {
        validate_command(command)?;
        let stdio = plan_rules::plan(Request {
            configured: Streams {
                stdin: command.stdin.as_ref(),
                stdout: command.stdout.as_ref(),
                stderr: command.stderr.as_ref(),
            },
            console: command.creation.console != ConsoleMode::Inherit,
            pseudoconsole: options.pseudoconsole.is_some(),
            parent: options.parent.is_some(),
            mode: io_mode,
        })
        .map_err(conflict_error)?;
        Ok(Self {
            command,
            options,
            stdio,
            state: PhantomData,
        })
    }
}

/// The validation error that reports a planning conflict.
const fn conflict_error(conflict: Conflict) -> ValidationError {
    match conflict {
        Conflict::PseudoConsoleWithConsoleMode => ValidationError::PseudoConsoleWithConsoleMode,
        Conflict::PseudoConsoleWithExplicitStdio => ValidationError::PseudoConsoleWithStdio,
        Conflict::OutputWithPseudoConsole => ValidationError::PseudoConsoleWithOutputCapture,
        Conflict::ParentWithoutExplicitStdio => ValidationError::AlternateParentNeedsStdio,
    }
}

pub(crate) fn validate_command(command: &Command) -> Result<(), ValidationError> {
    if command.program.is_empty() {
        return Err(ValidationError::EmptyProgram);
    }
    no_nul(&command.program, InputField::Program)?;
    if command.program.as_encoded_bytes().contains(&b'\"') {
        return Err(ValidationError::QuotedProgram);
    }
    let program_path = Path::new(&command.program);
    if program_path.file_name().is_none() {
        return Err(ValidationError::MissingProgramFileName);
    }
    if let Some(extension) = program_path.extension() {
        let extension = extension.as_encoded_bytes();
        if extension.eq_ignore_ascii_case(b"bat") || extension.eq_ignore_ascii_case(b"cmd") {
            return Err(ValidationError::BatchFile);
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

fn validate_env_key(key: &OsStr) -> Result<(), ValidationError> {
    if key.is_empty() {
        return Err(ValidationError::EmptyEnvironmentName);
    }
    no_nul(key, InputField::EnvironmentName)?;
    if key.as_encoded_bytes().contains(&b'=') {
        return Err(ValidationError::InvalidEnvironmentName);
    }
    Ok(())
}

fn no_nul(value: &OsStr, field: InputField) -> Result<(), ValidationError> {
    if value.encode_wide().any(|unit| unit == 0) {
        Err(ValidationError::InteriorNul { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use super::*;
    use crate::handles::ParentProcess;
    use crate::sys::test_support::TestConsole;

    fn refusal(command: &Command, options: SpawnOptions<'_>, mode: IoMode) -> ValidationError {
        SpawnPlan::new_running(command, options, mode).unwrap_err()
    }

    #[test]
    fn rejects_batch_and_empty_programs() {
        let error = refusal(&Command::new(""), SpawnOptions::new(), IoMode::Spawn);
        assert_eq!(error, ValidationError::EmptyProgram);
        assert_eq!(error.to_string(), "program must not be empty");

        for script in ["thing.cmd", "THING.BAT"] {
            assert_eq!(
                refusal(&Command::new(script), SpawnOptions::new(), IoMode::Spawn),
                ValidationError::BatchFile
            );
        }
    }

    #[test]
    fn every_console_mode_plans_and_only_inherit_joins_a_pseudoconsole() {
        let pseudoconsole = TestConsole::create();
        for mode in [
            ConsoleMode::Inherit,
            ConsoleMode::NoConsole,
            ConsoleMode::NewConsole,
            ConsoleMode::NoWindow,
        ] {
            let mut command = Command::new("cmd.exe");
            command.console(mode);
            assert!(SpawnPlan::new_running(&command, SpawnOptions::new(), IoMode::Output).is_ok());
            let attached = SpawnPlan::new_running(
                &command,
                SpawnOptions::new().pseudo_console(&pseudoconsole),
                IoMode::Spawn,
            );
            if mode == ConsoleMode::Inherit {
                assert!(matches!(
                    attached.map(|plan| plan.stdio),
                    Ok(StandardIo::PseudoConsole)
                ));
            } else {
                assert_eq!(
                    attached.unwrap_err(),
                    ValidationError::PseudoConsoleWithConsoleMode
                );
            }
        }
    }

    #[test]
    fn rejects_every_malformed_text_component() {
        let nul = OsString::from_wide(&[u16::from(b'x'), 0, u16::from(b'y')]);
        let field = |field| ValidationError::InteriorNul { field };
        let mut cases = vec![
            (Command::new(nul.clone()), field(InputField::Program)),
            (
                Command::new("bad\"program.exe"),
                ValidationError::QuotedProgram,
            ),
            (
                Command::new(r"C:\"),
                ValidationError::MissingProgramFileName,
            ),
        ];

        let mut text_arg = Command::new("cmd.exe");
        text_arg.arg(&nul);
        cases.push((text_arg, field(InputField::Argument)));
        let mut raw_arg = Command::new("cmd.exe");
        raw_arg.raw_arg(&nul);
        cases.push((raw_arg, field(InputField::Argument)));
        let mut empty_key = Command::new("cmd.exe");
        empty_key.env("", "value");
        cases.push((empty_key, ValidationError::EmptyEnvironmentName));
        let mut equals_key = Command::new("cmd.exe");
        equals_key.env("A=B", "value");
        cases.push((equals_key, ValidationError::InvalidEnvironmentName));
        let mut nul_key = Command::new("cmd.exe");
        nul_key.env(&nul, "value");
        cases.push((nul_key, field(InputField::EnvironmentName)));
        let mut nul_value = Command::new("cmd.exe");
        nul_value.env("KEY", &nul);
        cases.push((nul_value, field(InputField::EnvironmentValue)));
        let mut removed_key = Command::new("cmd.exe");
        removed_key.env_remove("A=B");
        cases.push((removed_key, ValidationError::InvalidEnvironmentName));
        let mut cwd = Command::new("cmd.exe");
        cwd.current_dir(&nul);
        cases.push((cwd, field(InputField::CurrentDirectory)));

        for (command, expected) in cases {
            assert_eq!(
                refusal(&command, SpawnOptions::new(), IoMode::Spawn),
                expected
            );
        }
    }

    #[test]
    fn pseudoconsole_and_parent_conflicts_are_rejected() {
        let pseudoconsole = TestConsole::create();
        let mut explicit = Command::new("cmd.exe");
        explicit.stdin(Stdio::null());
        assert_eq!(
            refusal(
                &explicit,
                SpawnOptions::new().pseudo_console(&pseudoconsole),
                IoMode::Spawn,
            ),
            ValidationError::PseudoConsoleWithStdio
        );

        let plain = Command::new("cmd.exe");
        assert_eq!(
            refusal(
                &plain,
                SpawnOptions::new().pseudo_console(&pseudoconsole),
                IoMode::Output,
            ),
            ValidationError::PseudoConsoleWithOutputCapture
        );

        let parent = ParentProcess::open(std::process::id()).unwrap();
        assert_eq!(
            refusal(
                &plain,
                SpawnOptions::new().parent_process(&parent),
                IoMode::Spawn,
            ),
            ValidationError::AlternateParentNeedsStdio
        );

        for missing in 0..3 {
            let mut command = Command::new("cmd.exe");
            if missing != 0 {
                command.stdin(Stdio::null());
            }
            if missing != 1 {
                command.stdout(Stdio::null());
            }
            if missing != 2 {
                command.stderr(Stdio::null());
            }
            assert!(SpawnPlan::new_running(
                &command,
                SpawnOptions::new().parent_process(&parent),
                IoMode::Spawn,
            )
            .is_err());
        }
    }

    #[test]
    fn successful_plans_choose_the_expected_stdio_modes() {
        let command = Command::new("cmd.exe");
        let output = SpawnPlan::new_running(&command, SpawnOptions::new(), IoMode::Output).unwrap();
        let StandardIo::Ordinary(output_stdio) = output.stdio else {
            panic!("output capture must use ordinary standard I/O");
        };
        assert!(matches!(output_stdio.stdin, Source::Null));
        assert!(matches!(output_stdio.stdout, Source::Piped));
        assert!(matches!(output_stdio.stderr, Source::Piped));

        let pseudoconsole = TestConsole::create();
        let pcon = SpawnPlan::new_suspended(
            &command,
            SpawnOptions::new().pseudo_console(&pseudoconsole),
            IoMode::Spawn,
        )
        .unwrap();
        assert!(matches!(pcon.stdio, StandardIo::PseudoConsole));
    }
}
