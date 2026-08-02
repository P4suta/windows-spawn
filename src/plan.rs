//! Pure normalization and validation before any OS resource is acquired.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use crate::command::{Arg, Command, EnvOp, EnvValue};
use crate::handles::Stdio;
use crate::options::{CreationFlags, SpawnOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpawnMode {
    Running,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Invalid,
}

#[derive(Debug)]
pub(crate) struct SpawnPlan<'command, 'options> {
    pub(crate) command: &'command Command,
    pub(crate) options: SpawnOptions<'options>,
    pub(crate) mode: SpawnMode,
    pub(crate) stdin: StdioSpec<'command>,
    pub(crate) stdout: StdioSpec<'command>,
    pub(crate) stderr: StdioSpec<'command>,
}

impl<'command, 'options> SpawnPlan<'command, 'options> {
    pub(crate) fn new(
        command: &'command Command,
        options: SpawnOptions<'options>,
        mode: SpawnMode,
        io_mode: IoMode,
    ) -> io::Result<Self> {
        validate_command(command)?;
        validate_creation_flags(options.creation_flags, options.pseudoconsole.is_some())?;

        let explicit_stdio =
            command.stdin.is_some() || command.stdout.is_some() || command.stderr.is_some();
        if options.pseudoconsole.is_some() {
            if explicit_stdio {
                return Err(invalid(
                    "a pseudoconsole conflicts with explicit standard I/O",
                ));
            }
            if io_mode == IoMode::Output {
                return Err(invalid(
                    "output capture conflicts with pseudoconsole standard I/O",
                ));
            }
        }
        if options.parent.is_some()
            && options.pseudoconsole.is_none()
            && (command.stdin.is_none() || command.stdout.is_none() || command.stderr.is_none())
        {
            return Err(invalid(
                "an alternate parent requires all three standard streams to be explicit",
            ));
        }
        let (stdin, stdout, stderr) = if options.pseudoconsole.is_some() {
            (StdioSpec::Invalid, StdioSpec::Invalid, StdioSpec::Invalid)
        } else {
            match io_mode {
                IoMode::Spawn => (
                    configured_or(command.stdin.as_ref(), StdioSpec::Inherit),
                    configured_or(command.stdout.as_ref(), StdioSpec::Inherit),
                    configured_or(command.stderr.as_ref(), StdioSpec::Inherit),
                ),
                IoMode::Output => (
                    configured_or(command.stdin.as_ref(), StdioSpec::Null),
                    configured_or(command.stdout.as_ref(), StdioSpec::Piped),
                    configured_or(command.stderr.as_ref(), StdioSpec::Piped),
                ),
            }
        };

        Ok(Self {
            command,
            options,
            mode,
            stdin,
            stdout,
            stderr,
        })
    }
}

fn configured_or<'a>(value: Option<&'a Stdio>, default: StdioSpec<'a>) -> StdioSpec<'a> {
    value.map_or(default, StdioSpec::Configured)
}

fn validate_command(command: &Command) -> io::Result<()> {
    if command.program.is_empty() {
        return Err(invalid("program must not be empty"));
    }
    no_nul(&command.program, "program contains an interior NUL")?;
    if command.program.as_encoded_bytes().contains(&b'\"') {
        return Err(invalid("program must not contain a double quote"));
    }
    let program_path = Path::new(&command.program);
    if program_path.file_name().is_none() {
        return Err(invalid("program path has no file name"));
    }
    if let Some(extension) = program_path.extension() {
        let extension = extension.as_encoded_bytes();
        if extension.eq_ignore_ascii_case(b"bat") || extension.eq_ignore_ascii_case(b"cmd") {
            return Err(invalid(
                "batch files must be invoked through an explicit command shell",
            ));
        }
    }

    for arg in &command.args {
        match arg {
            Arg::Text(text) | Arg::Raw(text) => {
                no_nul(text, "argument contains an interior NUL")?;
            }
            Arg::Handle(_) => {}
        }
    }
    for operation in &command.env_ops {
        match operation {
            EnvOp::Set(key, value) => {
                validate_env_key(key)?;
                if let EnvValue::Text(value) = value {
                    no_nul(value, "environment value contains an interior NUL")?;
                }
            }
            EnvOp::Remove(key) => validate_env_key(key)?,
        }
    }
    if let Some(cwd) = &command.cwd {
        no_nul(
            cwd.as_os_str(),
            "current directory contains an interior NUL",
        )?;
    }
    Ok(())
}

fn validate_env_key(key: &OsStr) -> io::Result<()> {
    if key.is_empty() {
        return Err(invalid("environment variable name must not be empty"));
    }
    no_nul(key, "environment variable name contains an interior NUL")?;
    if key.as_encoded_bytes().contains(&b'=') {
        return Err(invalid("environment variable name must not contain `=`"));
    }
    Ok(())
}

fn no_nul(value: &OsStr, message: &'static str) -> io::Result<()> {
    if value.encode_wide().any(|unit| unit == 0) {
        Err(invalid(message))
    } else {
        Ok(())
    }
}

fn validate_creation_flags(flags: CreationFlags, pseudoconsole: bool) -> io::Result<()> {
    let detached = flags.contains(CreationFlags::DETACHED_PROCESS);
    let new_console = flags.contains(CreationFlags::NEW_CONSOLE);
    let no_window = flags.contains(CreationFlags::NO_WINDOW);
    if detached && new_console {
        return Err(invalid(
            "DETACHED_PROCESS and NEW_CONSOLE are mutually exclusive",
        ));
    }
    if no_window && (detached || new_console) {
        return Err(invalid(
            "NO_WINDOW cannot be combined with DETACHED_PROCESS or NEW_CONSOLE",
        ));
    }
    if pseudoconsole && (detached || new_console || no_window) {
        return Err(invalid(
            "console creation flags conflict with a pseudoconsole",
        ));
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use super::*;
    use crate::handles::{AsPseudoConsole, ParentProcess};

    struct InvalidPseudoConsole;

    // SAFETY: every test rejects the request during pure planning, before the
    // sentinel value can reach the system layer.
    unsafe impl AsPseudoConsole for InvalidPseudoConsole {
        fn raw_pseudoconsole(&self) -> isize {
            1
        }
    }

    #[test]
    fn rejects_batch_and_empty_programs() {
        let empty = Command::new("");
        assert_eq!(
            SpawnPlan::new(
                &empty,
                SpawnOptions::new(),
                SpawnMode::Running,
                IoMode::Spawn,
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );

        for script in ["thing.cmd", "THING.BAT"] {
            let command = Command::new(script);
            assert!(SpawnPlan::new(
                &command,
                SpawnOptions::new(),
                SpawnMode::Running,
                IoMode::Spawn,
            )
            .is_err());
        }
    }

    #[test]
    fn rejects_conflicting_console_flags() {
        let command = Command::new("cmd.exe");
        let options = SpawnOptions::new()
            .creation_flags(CreationFlags::DETACHED_PROCESS | CreationFlags::NEW_CONSOLE);
        assert!(SpawnPlan::new(&command, options, SpawnMode::Running, IoMode::Spawn).is_err());

        for flags in [
            CreationFlags::NO_WINDOW | CreationFlags::DETACHED_PROCESS,
            CreationFlags::NO_WINDOW | CreationFlags::NEW_CONSOLE,
        ] {
            let options = SpawnOptions::new().creation_flags(flags);
            assert!(SpawnPlan::new(&command, options, SpawnMode::Running, IoMode::Spawn).is_err());
        }
    }

    #[test]
    fn rejects_every_malformed_text_component() {
        let nul = OsString::from_wide(&[u16::from(b'x'), 0, u16::from(b'y')]);
        let mut cases = vec![Command::new(nul.clone()), Command::new("bad\"program.exe")];
        cases.push(Command::new(r"C:\"));

        let mut text_arg = Command::new("cmd.exe");
        text_arg.arg(&nul);
        cases.push(text_arg);
        let mut raw_arg = Command::new("cmd.exe");
        raw_arg.raw_arg(&nul);
        cases.push(raw_arg);
        let mut empty_key = Command::new("cmd.exe");
        empty_key.env("", "value");
        cases.push(empty_key);
        let mut equals_key = Command::new("cmd.exe");
        equals_key.env("A=B", "value");
        cases.push(equals_key);
        let mut nul_key = Command::new("cmd.exe");
        nul_key.env(&nul, "value");
        cases.push(nul_key);
        let mut nul_value = Command::new("cmd.exe");
        nul_value.env("KEY", &nul);
        cases.push(nul_value);
        let mut removed_key = Command::new("cmd.exe");
        removed_key.env_remove("A=B");
        cases.push(removed_key);
        let mut cwd = Command::new("cmd.exe");
        cwd.current_dir(&nul);
        cases.push(cwd);

        for command in cases {
            assert_eq!(
                SpawnPlan::new(
                    &command,
                    SpawnOptions::new(),
                    SpawnMode::Running,
                    IoMode::Spawn,
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::InvalidInput
            );
        }

        let valid_command = Command::new("cmd.exe");
        for flags in [
            CreationFlags::DETACHED_PROCESS,
            CreationFlags::NEW_CONSOLE,
            CreationFlags::NO_WINDOW,
        ] {
            let options = SpawnOptions::new().creation_flags(flags);
            assert!(
                SpawnPlan::new(&valid_command, options, SpawnMode::Running, IoMode::Spawn,).is_ok()
            );
        }
    }

    #[test]
    fn pseudoconsole_and_parent_conflicts_are_rejected() {
        let pseudoconsole = InvalidPseudoConsole;
        let mut explicit = Command::new("cmd.exe");
        explicit.stdin(Stdio::null());
        assert!(SpawnPlan::new(
            &explicit,
            SpawnOptions::new().pseudoconsole(&pseudoconsole),
            SpawnMode::Running,
            IoMode::Spawn,
        )
        .is_err());

        let plain = Command::new("cmd.exe");
        assert!(SpawnPlan::new(
            &plain,
            SpawnOptions::new().pseudoconsole(&pseudoconsole),
            SpawnMode::Running,
            IoMode::Output,
        )
        .is_err());
        assert!(SpawnPlan::new(
            &plain,
            SpawnOptions::new()
                .pseudoconsole(&pseudoconsole)
                .creation_flags(CreationFlags::NEW_CONSOLE),
            SpawnMode::Running,
            IoMode::Spawn,
        )
        .is_err());

        let parent = ParentProcess::open(std::process::id()).unwrap();
        assert!(SpawnPlan::new(
            &plain,
            SpawnOptions::new().parent_process(&parent),
            SpawnMode::Running,
            IoMode::Spawn,
        )
        .is_err());
    }

    #[test]
    fn successful_plans_choose_the_expected_stdio_modes() {
        let command = Command::new("cmd.exe");
        let output = SpawnPlan::new(
            &command,
            SpawnOptions::new(),
            SpawnMode::Running,
            IoMode::Output,
        )
        .unwrap();
        assert!(matches!(output.stdin, StdioSpec::Null));
        assert!(matches!(output.stdout, StdioSpec::Piped));
        assert!(matches!(output.stderr, StdioSpec::Piped));

        let pseudoconsole = InvalidPseudoConsole;
        let pcon = SpawnPlan::new(
            &command,
            SpawnOptions::new().pseudoconsole(&pseudoconsole),
            SpawnMode::Suspended,
            IoMode::Spawn,
        )
        .unwrap();
        assert!(matches!(pcon.stdin, StdioSpec::Invalid));
        assert!(matches!(pcon.stdout, StdioSpec::Invalid));
        assert!(matches!(pcon.stderr, StdioSpec::Invalid));
    }
}
