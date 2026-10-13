//! End-to-end UTF-16 argument and environment round-trip test.
//!
//! Seeded random environments then pass through the crate's block builder into children,
//! which must hold exactly the variables the commands set, merged and ordered as Windows compares names.

#[cfg(windows)]
mod random;

#[cfg(windows)]
mod windows {
    use std::cmp::Ordering;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io;
    use std::mem::size_of;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::ptr;

    use windows_spawn::{Command, Stdio};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::Globalization::{
        CompareStringOrdinal, CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN,
    };
    use windows_sys::Win32::System::Environment::{
        FreeEnvironmentStringsW, GetEnvironmentStringsW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, WaitForSingleObject, INFINITE, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    use crate::random::{scaled, Generator};

    const OUTPUT: &str = "WINDOWS_SPAWN_ROUNDTRIP_OUTPUT";
    const VALUE: &str = "WINDOWS_SPAWN_ROUNDTRIP_VALUE";
    const MIXED: &str = "WINDOWS_SPAWN_ROUNDTRIP_MIXED";
    const REMOVE: &str = "WINDOWS_SPAWN_ROUNDTRIP_REMOVE";

    /// Writes `count` as the little-endian `DWORD` that precedes what it counts.
    fn append_count(buffer: &mut Vec<u8>, count: usize) -> io::Result<()> {
        let count = u32::try_from(count).map_err(io::Error::other)?;
        buffer.extend_from_slice(&count.to_le_bytes());
        Ok(())
    }

    fn append_os(buffer: &mut Vec<u8>, value: &OsStr) -> io::Result<()> {
        let wide: Vec<u16> = value.encode_wide().collect();
        append_count(buffer, wide.len())?;
        for unit in wide {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
        Ok(())
    }

    /// Reads a variable the parent sets for the child.
    fn parent_variable(name: &str) -> io::Result<OsString> {
        std::env::var_os(name).ok_or_else(|| io::Error::other(format!("the parent sets {name}")))
    }

    fn child() -> io::Result<()> {
        let output = parent_variable(OUTPUT)?;
        let arguments: Vec<OsString> = std::env::args_os().skip(2).collect();
        let mut encoded = encode_arguments(&arguments)?;
        append_os(&mut encoded, &parent_variable(VALUE)?)?;
        append_os(&mut encoded, &parent_variable(MIXED)?)?;
        encoded.push(u8::from(std::env::var_os(REMOVE).is_some()));
        fs::write(output, encoded)
    }

    fn encode_arguments(arguments: &[OsString]) -> io::Result<Vec<u8>> {
        let mut encoded = Vec::new();
        append_count(&mut encoded, arguments.len())?;
        for argument in arguments {
            append_os(&mut encoded, argument)?;
        }
        Ok(encoded)
    }

    fn broker_child() -> io::Result<()> {
        let mut arguments = std::env::args_os().skip(2);
        let output = arguments
            .next()
            .ok_or_else(|| io::Error::other("the parent passes the output path"))?;
        let arguments: Vec<OsString> = arguments.collect();
        fs::write(output, encode_arguments(&arguments)?)
    }

    /// Starts `line` with a null application name, as a launch broker does.
    fn launch_like_a_broker(line: &OsStr) -> io::Result<u32> {
        let mut wide: Vec<u16> = line.encode_wide().chain(Some(0)).collect();
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).map_err(io::Error::other)?,
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
                OwnedHandle::from_raw_handle(information.hProcess),
                OwnedHandle::from_raw_handle(information.hThread),
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
        if actual != encode_arguments(&arguments)? {
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

        let mut expected = encode_arguments(&arguments)?;
        append_os(&mut expected, &environment)?;
        append_os(&mut expected, &mixed)?;
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

    /// The seed, the number, and the size of the random environments.
    const ENVIRONMENT_SEED: u64 = 0x0e17_5eed_b10c_0001;
    const RANDOM_ENVIRONMENTS: usize = 150;
    const MOST_VARIABLES: u64 = 4;
    const LONGEST_NAME: u64 = 8;
    const LONGEST_VALUE: u64 = 10;

    /// The unit between a name and its value, which no name the crate accepts holds.
    const SEPARATOR: u16 = 0x3d;
    /// The unit a generated name gets in place of the separator.
    const UNDERSCORE: u16 = 0x5f;

    /// The variables a tool or Windows sets inside every process, which no generated name can match:
    /// Application Verifier's, so `cargo xtask verifier` can run this test, and the LLVM profile runtime's, so coverage can.
    /// Windows on ARM64 also gives every process its architecture, even a process whose environment holds nothing else.
    const INJECTED_VARIABLES: [&str; 3] = [
        "OANOCACHE",
        "__LLVM_PROFILE_RT_INIT_ONCE",
        "PROCESSOR_ARCHITECTURE",
    ];

    /// Units whose case Windows folds and units that look alike:
    /// `A`, `a`, `Z`, `z`, `_`, `0`, `É`, `é`, `ß`, `İ`, `ı`, `Ǆ`, `ǅ`, `ǆ`, `Σ`, `σ`, `ς`, Cyrillic capital and small zhe (U+0416 and U+0436), fullwidth `Ａ` and `ａ`,
    /// a lone high and a lone low surrogate, space, double quote, and backslash.
    const NAME_UNITS: [u16; 26] = [
        0x41, 0x61, 0x5a, 0x7a, 0x5f, 0x30, 0x00c9, 0x00e9, 0x00df, 0x0130, 0x0131, 0x01c4, 0x01c5,
        0x01c6, 0x03a3, 0x03c3, 0x03c2, 0x0416, 0x0436, 0xff21, 0xff41, 0xd800, 0xdc00, 0x20, 0x22,
        0x5c,
    ];
    /// The separator, space, double quote, backslash, line feed, `é`, a lone high and a lone low surrogate, and the byte-order mark.
    const VALUE_UNITS: [u16; 9] = [
        SEPARATOR, 0x20, 0x22, 0x5c, 0x0a, 0x00e9, 0xd800, 0xdc00, 0xfeff,
    ];

    fn hex(units: &[u16]) -> String {
        units
            .iter()
            .map(|unit| format!("{unit:x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Orders two names as the crate's environment map does, with the comparison Windows uses for names.
    ///
    /// A name too long for the comparison, which no generated name reaches, falls back to the order of its units, as an unknown result does.
    fn by_name(left: &[u16], right: &[u16]) -> Ordering {
        let (Ok(left_length), Ok(right_length)) =
            (i32::try_from(left.len()), i32::try_from(right.len()))
        else {
            return left.cmp(right);
        };
        // SAFETY: both pointers are readable for their lengths.
        match unsafe {
            CompareStringOrdinal(left.as_ptr(), left_length, right.as_ptr(), right_length, 1)
        } {
            CSTR_LESS_THAN => Ordering::Less,
            CSTR_EQUAL => Ordering::Equal,
            CSTR_GREATER_THAN => Ordering::Greater,
            _ => left.cmp(right),
        }
    }

    /// Prints every entry of this process's environment block, and every variable `std` reads from it.
    fn environment_child() -> io::Result<()> {
        // SAFETY: GetEnvironmentStringsW returns a double-NUL block that stays valid until FreeEnvironmentStringsW.
        let block = unsafe { GetEnvironmentStringsW() };
        if block.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut entries = Vec::new();
        let mut offset = 0_usize;
        loop {
            let mut length = 0_usize;
            // SAFETY: every entry ends with a NUL inside the block, and the scan stops there.
            while unsafe { *block.add(offset.saturating_add(length)) } != 0 {
                length = length.saturating_add(1);
            }
            if length == 0 {
                break;
            }
            // SAFETY: the scan just measured `length` readable units.
            entries.push(unsafe { std::slice::from_raw_parts(block.add(offset), length) }.to_vec());
            offset = offset.saturating_add(length).saturating_add(1);
        }
        // SAFETY: the block came from GetEnvironmentStringsW, and nothing reads it after this.
        unsafe { FreeEnvironmentStringsW(block) };
        for entry in entries {
            println!("raw\t{}", hex(&entry));
        }
        for (name, value) in std::env::vars_os() {
            let name: Vec<u16> = name.encode_wide().collect();
            let value: Vec<u16> = value.encode_wide().collect();
            println!("std\t{}\t{}", hex(&name), hex(&value));
        }
        Ok(())
    }

    /// Passes random variables through the crate's block builder, and requires the child to hold exactly them.
    fn random_environments() -> io::Result<()> {
        let mut generator = Generator::new(ENVIRONMENT_SEED);
        let profile = std::env::var_os("LLVM_PROFILE_FILE");
        let mut mismatches = Vec::new();
        let environments = scaled(RANDOM_ENVIRONMENTS)?;
        for _ in 0..environments {
            let mut variables: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
            if let Some(profile) = &profile {
                variables.push((
                    OsStr::new("LLVM_PROFILE_FILE").encode_wide().collect(),
                    profile.encode_wide().collect(),
                ));
            }
            let count = generator.below(MOST_VARIABLES).saturating_add(1);
            for _ in 0..count {
                let mut name = generator.text(LONGEST_NAME.saturating_sub(1), &NAME_UNITS);
                name.push(generator.unit(&NAME_UNITS));
                for unit in &mut name {
                    if *unit == SEPARATOR {
                        *unit = UNDERSCORE;
                    }
                }
                variables.push((name, generator.text(LONGEST_VALUE, &VALUE_UNITS)));
            }

            let mut command = Command::new(std::env::current_exe()?);
            command
                .arg("--environment-child")
                .env_clear()
                .stdin(Stdio::null());
            for (name, value) in &variables {
                command.env(OsString::from_wide(name), OsString::from_wide(value));
            }
            let output = command.output()?;
            if !output.status.success() {
                return Err(io::Error::other(format!(
                    "the environment child failed with {}",
                    output.status
                )));
            }

            let mut expected: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
            for (name, value) in variables {
                match expected
                    .iter_mut()
                    .find(|(held, _)| by_name(held, &name) == Ordering::Equal)
                {
                    Some((_, held)) => *held = value,
                    None => expected.push((name, value)),
                }
            }
            expected.sort_by(|left, right| by_name(&left.0, &right.0));
            let raw: Vec<String> = expected
                .iter()
                .map(|(name, value)| {
                    let mut entry = name.clone();
                    entry.push(SEPARATOR);
                    entry.extend(value);
                    format!("raw\t{}", hex(&entry))
                })
                .collect();
            let read_by_std: Vec<String> = expected
                .iter()
                .map(|(name, value)| format!("std\t{}\t{}", hex(name), hex(value)))
                .collect();
            let report = String::from_utf8_lossy(&output.stdout);
            let prefixes: Vec<String> = INJECTED_VARIABLES
                .iter()
                .flat_map(|name| {
                    let name = hex(&OsStr::new(name).encode_wide().collect::<Vec<_>>());
                    [format!("raw\t{name} 3d "), format!("std\t{name}\t")]
                })
                .collect();
            let lines: Vec<&str> = report
                .lines()
                .filter(|line| !prefixes.iter().any(|prefix| line.starts_with(prefix)))
                .collect();
            let expected_lines: Vec<&str> =
                raw.iter().chain(&read_by_std).map(String::as_str).collect();
            if lines != expected_lines {
                mismatches.push(format!(
                    "expected {expected_lines:?}, the child holds {lines:?}"
                ));
            }
        }
        if !mismatches.is_empty() {
            let shown: Vec<&str> = mismatches.iter().take(10).map(String::as_str).collect();
            return Err(io::Error::other(format!(
                "{} random environments arrived differently, the first ones:\n{}",
                mismatches.len(),
                shown.join("\n")
            )));
        }
        println!("{environments} random environments through the block builder: every child holds exactly its variables");
        Ok(())
    }

    pub(super) fn main() -> io::Result<()> {
        match std::env::args_os().nth(1) {
            Some(mode) if mode == "--child" => child(),
            Some(mode) if mode == "--broker-child" => broker_child(),
            Some(mode) if mode == "--environment-child" => environment_child(),
            Some(_) | None => {
                parent()?;
                broker_parent()?;
                random_environments()
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
