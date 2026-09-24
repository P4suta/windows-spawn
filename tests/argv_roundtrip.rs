//! End-to-end UTF-16 argument and environment round-trip test.

#[cfg(windows)]
mod windows {
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::ptr;

    use windows_spawn::Command;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, WaitForSingleObject, INFINITE, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    const OUTPUT: &str = "WINDOWS_SPAWN_ROUNDTRIP_OUTPUT";
    const VALUE: &str = "WINDOWS_SPAWN_ROUNDTRIP_VALUE";
    const MIXED: &str = "WINDOWS_SPAWN_ROUNDTRIP_MIXED";
    const REMOVE: &str = "WINDOWS_SPAWN_ROUNDTRIP_REMOVE";

    fn append_os(buffer: &mut Vec<u8>, value: &OsStr) {
        let wide: Vec<u16> = value.encode_wide().collect();
        let length = u32::try_from(wide.len()).expect("test values fit in a DWORD");
        buffer.extend_from_slice(&length.to_le_bytes());
        for unit in wide {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn child() -> io::Result<()> {
        let output = std::env::var_os(OUTPUT).expect("parent supplies output path");
        let arguments: Vec<OsString> = std::env::args_os().skip(2).collect();
        let mut encoded = encode_arguments(&arguments);
        append_os(
            &mut encoded,
            &std::env::var_os(VALUE).expect("parent supplies environment value"),
        );
        append_os(
            &mut encoded,
            &std::env::var_os(MIXED).expect("case-insensitive replacement survives"),
        );
        encoded.push(u8::from(std::env::var_os(REMOVE).is_some()));
        fs::write(output, encoded)
    }

    fn encode_arguments(arguments: &[OsString]) -> Vec<u8> {
        let mut encoded = Vec::new();
        let count = u32::try_from(arguments.len()).expect("test corpus fits in a DWORD");
        encoded.extend_from_slice(&count.to_le_bytes());
        for argument in arguments {
            append_os(&mut encoded, argument);
        }
        encoded
    }

    fn broker_child() -> io::Result<()> {
        let mut arguments = std::env::args_os().skip(2);
        let output = arguments.next().expect("parent supplies output path");
        let arguments: Vec<OsString> = arguments.collect();
        fs::write(output, encode_arguments(&arguments))
    }

    /// Starts `line` with a null application name, as a launch broker does.
    fn launch_like_a_broker(line: &OsStr) -> io::Result<u32> {
        let mut wide: Vec<u16> = line.encode_wide().chain(Some(0)).collect();
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).expect("STARTUPINFOW fits in a DWORD"),
            ..STARTUPINFOW::default()
        };
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: `wide` is a writable NUL-terminated buffer, no handles are inherited, and `startup` and `information` outlive the call.
        let created = unsafe {
            CreateProcessW(
                ptr::null(),
                wide.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                ptr::null(),
                ptr::null(),
                &startup,
                &mut information,
            )
        };
        if created == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateProcessW succeeded, so both handles are new and owned here.
        let (process, _thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(information.hProcess as RawHandle),
                OwnedHandle::from_raw_handle(information.hThread as RawHandle),
            )
        };
        // SAFETY: the owned process handle stays open for the wait.
        if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0_u32;
        // SAFETY: the owned process handle is valid and `code` is writable.
        if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(code)
    }

    fn broker_parent() -> io::Result<()> {
        let arguments = corpus();
        let output = std::env::temp_dir().join(format!(
            "windows-spawn-broker-roundtrip-{}-{}.bin",
            std::process::id(),
            arguments.len()
        ));
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("--broker-child").arg(&output).args(&arguments);
        let code = launch_like_a_broker(&command.to_command_line()?)?;
        if code != 0 {
            return Err(io::Error::other(format!(
                "broker round-trip child failed with exit code {code}"
            )));
        }

        let actual = fs::read(&output)?;
        fs::remove_file(output)?;
        if actual != encode_arguments(&arguments) {
            return Err(io::Error::other(
                "a broker-launched command line decoded to different arguments",
            ));
        }
        Ok(())
    }

    fn corpus() -> Vec<OsString> {
        vec![
            OsString::new(),
            OsString::from("plain"),
            OsString::from(" "),
            OsString::from("\t"),
            OsString::from("space separated words"),
            OsString::from("\""),
            OsString::from("embedded\"quote"),
            OsString::from("\\"),
            OsString::from("\\\\"),
            OsString::from("trailing\\"),
            OsString::from("trailing\\\\"),
            OsString::from("slashes\\\\\"before quote"),
            OsString::from("shell metacharacters &|<>^%()!"),
            OsString::from("日本語-Καλημέρα-🚀"),
            OsString::from_wide(&[0x0061, 0xd800, 0x0062]),
            OsString::from_wide(&[0xd83d, 0xde80]),
        ]
    }

    fn parent() -> io::Result<()> {
        let arguments = corpus();
        let environment = OsString::from_wide(&[
            0x74, 0x61, 0x62, 0x09, 0x65, 0x71, 0x3d, 0xd83d, 0xde80, 0xdfff,
        ]);
        let mixed = OsString::from("second-日本語");
        let output = std::env::temp_dir().join(format!(
            "windows-spawn-roundtrip-{}-{}.bin",
            std::process::id(),
            arguments.len()
        ));

        let mut command = Command::new(std::env::current_exe()?);
        command.arg("--child");
        command.args(&arguments);
        command
            .env_clear()
            .env(OUTPUT, &output)
            .env(VALUE, &environment)
            .env(MIXED, "first")
            .env("windows_spawn_roundtrip_mixed", &mixed)
            .env(REMOVE, "must disappear")
            .env_remove("windows_spawn_roundtrip_remove");
        if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", profile);
        }
        let status = command.status()?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "round-trip child failed with {status}"
            )));
        }

        let mut expected = encode_arguments(&arguments);
        append_os(&mut expected, &environment);
        append_os(&mut expected, &mixed);
        expected.push(0);

        let actual = fs::read(&output)?;
        fs::remove_file(output)?;
        if actual != expected {
            return Err(io::Error::other(format!(
                "UTF-16 round trip differed: expected {} bytes, got {}",
                expected.len(),
                actual.len()
            )));
        }
        Ok(())
    }

    pub(super) fn main() -> io::Result<()> {
        match std::env::args_os().nth(1) {
            Some(mode) if mode == "--child" => child(),
            Some(mode) if mode == "--broker-child" => broker_child(),
            Some(_) | None => {
                parent()?;
                broker_parent()
            }
        }
    }
}

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    windows::main()
}

#[cfg(not(windows))]
fn main() {}
