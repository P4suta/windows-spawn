//! The command-line parsers Windows ships, against the parser models `proofs/lean` proves the encoding for.
//!
//! The rows of `proofs/lean/vectors/command_line_parsing.tsv` hold every line over `a`, space, double quote, and backslash, up to six units after the program,
//! with the arguments each modelled C runtime reads from it.
//! `CommandLineToArgvW` splits every row in this process.
//! Every third row also starts a child, which splits its own command line with the Universal CRT, with `msvcrt.dll`, with `CommandLineToArgvW`, and with `std`.
//! Seeded random arguments over every unit but NUL then pass through the crate's encoder into such children, which must read them back.
//! The test compiles only with `--cfg windows_spawn_vectors`, which `cargo xtask test` passes, because the vectors stay outside the package.

#[cfg(all(windows, windows_spawn_vectors))]
mod random;

#[cfg(all(windows, windows_spawn_vectors))]
mod windows {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::mem::transmute;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    use windows_spawn::{Command, Stdio};
    use windows_sys::Win32::Foundation::LocalFree;

    use crate::random::{scaled, Generator};
    use windows_sys::Win32::System::Environment::GetCommandLineW;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

    const VECTORS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proofs/lean/vectors/command_line_parsing.tsv"
    ));
    const ROWS: usize = 5461;
    const SPAWN_EVERY: usize = 3;
    const CHILD: &str = "WINDOWS_SPAWN_PARSER_CHILD";

    /// A dialect the Lean models define.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Dialect {
        Ucrt,
        Msvcrt,
    }

    /// Each real parser and the modelled dialect it must agree with on every row.
    const PARSERS: [(&str, Dialect); 4] = [
        ("ucrt", Dialect::Ucrt),
        ("msvcrt", Dialect::Msvcrt),
        ("shell", Dialect::Msvcrt),
        ("std", Dialect::Ucrt),
    ];

    struct Row {
        line: Vec<u16>,
        ucrt: Vec<Vec<u16>>,
        msvcrt: Vec<Vec<u16>>,
    }

    impl Row {
        fn expected(&self, dialect: Dialect) -> &[Vec<u16>] {
            match dialect {
                Dialect::Ucrt => &self.ucrt,
                Dialect::Msvcrt => &self.msvcrt,
            }
        }
    }

    fn invalid(message: String) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }

    fn units(hex: &str) -> io::Result<Vec<u16>> {
        hex.split(' ')
            .filter(|unit| !unit.is_empty())
            .map(|unit| u16::from_str_radix(unit, 16).map_err(|error| invalid(error.to_string())))
            .collect()
    }

    fn hex(units: &[u16]) -> String {
        units
            .iter()
            .map(|unit| format!("{unit:x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn arguments(column: &str) -> io::Result<Vec<Vec<u16>>> {
        column.split('|').map(units).collect()
    }

    fn rows() -> io::Result<Vec<Row>> {
        let mut rows = Vec::new();
        for row in VECTORS.lines() {
            let mut columns = row.split('\t');
            let mut next = || {
                columns
                    .next()
                    .ok_or_else(|| invalid(format!("short row {row}")))
            };
            rows.push(Row {
                line: units(next()?)?,
                ucrt: arguments(next()?)?,
                msvcrt: arguments(next()?)?,
            });
        }
        if rows.len() == ROWS {
            Ok(rows)
        } else {
            Err(invalid(format!("{} rows, not {ROWS}", rows.len())))
        }
    }

    /// Copies `count` NUL-terminated strings out of an argument vector.
    ///
    /// # Safety
    ///
    /// `vector` points to `count` readable pointers, each to a NUL-terminated UTF-16 string.
    unsafe fn read_vector(vector: *const *mut u16, count: i32) -> io::Result<Vec<Vec<u16>>> {
        let count = usize::try_from(count).map_err(|error| invalid(error.to_string()))?;
        let mut arguments = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: the caller guarantees `count` readable pointers.
            let argument = unsafe { *vector.add(index) };
            let mut length = 0;
            // SAFETY: each string is NUL-terminated, so the scan stays inside it.
            while unsafe { *argument.add(length) } != 0 {
                length = length.saturating_add(1);
            }
            // SAFETY: the scan just measured `length` readable units.
            arguments.push(unsafe { std::slice::from_raw_parts(argument, length) }.to_vec());
        }
        Ok(arguments)
    }

    /// Splits a NUL-terminated line with `CommandLineToArgvW`.
    fn shell(line: &[u16]) -> io::Result<Vec<Vec<u16>>> {
        let mut count = 0;
        // SAFETY: `line` is NUL-terminated and `count` is writable.
        let vector = unsafe { CommandLineToArgvW(line.as_ptr(), &mut count) };
        if vector.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CommandLineToArgvW returned `count` NUL-terminated strings.
        let arguments = unsafe { read_vector(vector, count) };
        // SAFETY: the vector is one LocalAlloc block that only this function holds.
        unsafe { LocalFree(vector.cast()) };
        arguments
    }

    type Procedure = unsafe extern "system" fn() -> isize;

    fn procedure(library: &str, name: &[u8]) -> io::Result<Procedure> {
        let library: Vec<u16> = OsStr::new(library).encode_wide().chain(Some(0)).collect();
        // SAFETY: `library` is NUL-terminated; the module stays loaded for the process.
        let module = unsafe { LoadLibraryW(library.as_ptr()) };
        if module.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `name` is NUL-terminated ASCII.
        unsafe { GetProcAddress(module, name.as_ptr()) }.ok_or_else(io::Error::last_os_error)
    }

    /// Splits this process's command line with the Universal CRT.
    fn ucrt() -> io::Result<Vec<Vec<u16>>> {
        let configure = procedure("ucrtbase.dll", b"_configure_wide_argv\0")?;
        let count = procedure("ucrtbase.dll", b"__p___argc\0")?;
        let vector = procedure("ucrtbase.dll", b"__p___wargv\0")?;
        // SAFETY: the Universal CRT exports these with the signatures of corecrt_startup.h.
        let (configure, count, vector) = unsafe {
            (
                transmute::<Procedure, unsafe extern "C" fn(i32) -> i32>(configure),
                transmute::<Procedure, unsafe extern "C" fn() -> *mut i32>(count),
                transmute::<Procedure, unsafe extern "C" fn() -> *mut *mut *mut u16>(vector),
            )
        };
        // SAFETY: mode 1, `_crt_argv_unexpanded_arguments`, parses GetCommandLineW into the CRT's argument globals.
        if unsafe { configure(1) } != 0 {
            return Err(invalid("_configure_wide_argv failed".to_owned()));
        }
        // SAFETY: the globals hold the vector `configure` built.
        unsafe { read_vector(*vector(), *count()) }
    }

    /// Splits this process's command line with the system's `msvcrt.dll`.
    fn msvcrt() -> io::Result<Vec<Vec<u16>>> {
        let get = procedure("msvcrt.dll", b"__wgetmainargs\0")?;
        // SAFETY: msvcrt.dll exports `__wgetmainargs` with this signature; `_startupinfo` holds one int.
        let get = unsafe {
            transmute::<
                Procedure,
                unsafe extern "C" fn(
                    *mut i32,
                    *mut *mut *mut u16,
                    *mut *mut *mut u16,
                    i32,
                    *mut i32,
                ) -> i32,
            >(get)
        };
        let mut count = 0;
        let mut vector = ptr::null_mut();
        let mut environment = ptr::null_mut();
        let mut startup = 0;
        // SAFETY: every out pointer is writable.
        if unsafe { get(&mut count, &mut vector, &mut environment, 0, &mut startup) } != 0 {
            return Err(invalid("__wgetmainargs failed".to_owned()));
        }
        // SAFETY: __wgetmainargs built `count` NUL-terminated strings.
        unsafe { read_vector(vector, count) }
    }

    /// Prints what each parser reads from this process's command line, without the program name.
    fn child() -> io::Result<()> {
        // SAFETY: GetCommandLineW returns the process's NUL-terminated command line.
        let own = unsafe { GetCommandLineW() };
        let mut length = 0;
        // SAFETY: the command line is NUL-terminated.
        while unsafe { *own.add(length) } != 0 {
            length = length.saturating_add(1);
        }
        // SAFETY: the scan just measured `length` readable units, and one more for the NUL.
        let own = unsafe { std::slice::from_raw_parts(own, length.saturating_add(1)) };
        let std_arguments: Vec<Vec<u16>> = std::env::args_os()
            .map(|argument| argument.encode_wide().collect())
            .collect();
        for (name, arguments) in [
            ("ucrt", ucrt()?),
            ("msvcrt", msvcrt()?),
            ("shell", shell(own)?),
            ("std", std_arguments),
        ] {
            let shown: Vec<String> = arguments
                .iter()
                .skip(1)
                .map(|argument| hex(argument))
                .collect();
            println!("{name}\t{}\t{}", shown.len(), shown.join("|"));
        }
        Ok(())
    }

    fn spawned(row: &Row) -> io::Result<Vec<(String, Vec<Vec<u16>>)>> {
        let tail = row.line.get(2..).unwrap_or_default();
        let mut command = Command::new(std::env::current_exe()?);
        command.raw_arg(OsString::from_wide(tail));
        parsed(&mut command)
    }

    /// Runs `command` as a parser child, and returns what each parser read.
    fn parsed(command: &mut Command) -> io::Result<Vec<(String, Vec<Vec<u16>>)>> {
        command.env(CHILD, "1").stdin(Stdio::null());
        let output = command.output()?;
        if !output.status.success() {
            return Err(invalid(format!(
                "the parser child failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let text = String::from_utf8(output.stdout).map_err(|error| invalid(error.to_string()))?;
        text.lines()
            .map(|line| {
                let mut columns = line.splitn(3, '\t');
                let (Some(name), Some(count), Some(column)) =
                    (columns.next(), columns.next(), columns.next())
                else {
                    return Err(invalid(format!("unexpected child line {line}")));
                };
                let count: usize = count
                    .parse()
                    .map_err(|error| invalid(format!("bad count in {line}: {error}")))?;
                let arguments = if count == 0 {
                    Vec::new()
                } else {
                    arguments(column)?
                };
                if arguments.len() != count {
                    return Err(invalid(format!(
                        "{line} holds {} arguments",
                        arguments.len()
                    )));
                }
                Ok((name.to_owned(), arguments))
            })
            .collect()
    }

    fn parent() -> io::Result<()> {
        let rows = rows()?;
        let mut mismatches = Vec::new();
        for row in &rows {
            let mut line = row.line.clone();
            line.push(0);
            if shell(&line)? != row.expected(Dialect::Msvcrt) {
                mismatches.push(format!("shell in process: {}", hex(&row.line)));
            }
        }
        let mut children = 0_usize;
        for row in rows.iter().step_by(SPAWN_EVERY) {
            let results = spawned(row)?;
            for (name, dialect) in PARSERS {
                let read = results
                    .iter()
                    .find(|(parser, _)| parser == name)
                    .map(|(_, arguments)| arguments.as_slice())
                    .ok_or_else(|| invalid(format!("the child did not report {name}")))?;
                let expected = row.expected(dialect).get(1..).unwrap_or_default();
                if read != expected {
                    mismatches.push(format!(
                        "{name} ({dialect:?}): {} read {:?}, expected {:?}",
                        hex(&row.line),
                        read.iter()
                            .map(|argument| hex(argument))
                            .collect::<Vec<_>>(),
                        expected
                            .iter()
                            .map(|argument| hex(argument))
                            .collect::<Vec<_>>()
                    ));
                }
            }
            children = children.saturating_add(1);
        }
        if !mismatches.is_empty() {
            let shown: Vec<&String> = mismatches.iter().take(40).collect();
            return Err(invalid(format!(
                "{} disagreements with the models, the first ones:\n{}",
                mismatches.len(),
                shown
                    .iter()
                    .map(|line| line.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            )));
        }
        println!(
            "{} rows through CommandLineToArgvW, {children} children through four parsers: all agree with the models",
            rows.len()
        );
        random_lines()
    }

    /// The seed, the number, and the size of the random command lines.
    const SEED: u64 = 0x5eed_c0de_cafe_f00d;
    const RANDOM_LINES: usize = 400;
    const MOST_ARGUMENTS: u64 = 4;
    const LONGEST_ARGUMENT: u64 = 10;

    /// Units a parser or the encoder reads specially, and units that resemble them:
    /// space, tab, double quote, backslash, and vertical bar; line feed, vertical tab, form feed, and carriage return;
    /// no-break space, ideographic space, and line separator; a lone high and a lone low surrogate; and the byte-order mark.
    const INTERESTING: [u16; 15] = [
        0x20, 0x09, 0x22, 0x5c, 0x7c, 0x0a, 0x0b, 0x0c, 0x0d, 0x00a0, 0x3000, 0x2028, 0xd800,
        0xdc00, 0xfeff,
    ];

    /// Passes random arguments through the crate's encoder, and requires every parser to read them back.
    ///
    /// The vectors hold four units only; these lines hold any unit but NUL,
    /// so they test that every parser copies the units it gives no meaning, as the models assume.
    fn random_lines() -> io::Result<()> {
        let mut generator = Generator::new(SEED);
        let mut mismatches = Vec::new();
        let lines = scaled(RANDOM_LINES)?;
        for _ in 0..lines {
            let count = generator.below(MOST_ARGUMENTS).saturating_add(1);
            let arguments: Vec<Vec<u16>> = (0..count)
                .map(|_| generator.text(LONGEST_ARGUMENT, &INTERESTING))
                .collect();
            let mut command = Command::new(std::env::current_exe()?);
            command.args(
                arguments
                    .iter()
                    .map(|argument| OsString::from_wide(argument)),
            );
            for (name, read) in parsed(&mut command)? {
                if read != arguments {
                    mismatches.push(format!(
                        "{name}: {:?} read {:?}",
                        arguments
                            .iter()
                            .map(|argument| hex(argument))
                            .collect::<Vec<_>>(),
                        read.iter()
                            .map(|argument| hex(argument))
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
        if !mismatches.is_empty() {
            let shown: Vec<&str> = mismatches.iter().take(40).map(String::as_str).collect();
            return Err(invalid(format!(
                "{} random lines read back differently, the first ones:\n{}",
                mismatches.len(),
                shown.join("\n")
            )));
        }
        println!("{lines} random command lines through four parsers: every argument read back");
        Ok(())
    }

    pub(super) fn main() -> io::Result<()> {
        if std::env::var_os(CHILD).is_some() {
            child()
        } else {
            parent()
        }
    }
}

#[cfg(all(windows, windows_spawn_vectors))]
fn main() -> std::io::Result<()> {
    windows::main()
}

#[cfg(not(all(windows, windows_spawn_vectors)))]
fn main() {}
