# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow Semantic Versioning with Cargo's pre-1.0 rules.

## [Unreleased]

The 0.2 API follows `std::process` and places each choice on the type that owns it, as ADR 0015 records; `docs/migration-0.2.md` maps every 0.1 item.

### Added

- `Command::own_process_tree`, which makes `Child::kill`, dropping the `Child`, and output collection end the child's whole process tree.
- `Child::wait_tree`, which waits until every process of an owned tree has exited.
- `Command::console` with `ConsoleMode`, `Command::priority` with `PriorityClass`, and one `Command` setter for each creation flag.
- `SpawnError`, which keeps whether input was rejected, which Windows operation failed in which phase, and which releases failed; `windows_spawn::Result`.
- `BorrowedPseudoConsole` with `borrow_raw`, for a raw `HPCON`.
- `From<ChildStdin>`, `From<ChildStdout>`, `From<ChildStderr>`, `From<io::Stdout>`, and `From<io::Stderr>` for `Stdio`; `Write for &ChildStdin`; `AsRawHandle` for the handle types.
- `ValidationError::CommandLineTooLong` and `ValidationError::PseudoConsoleWithConsoleMode`, which reject a spawn before Windows is asked.
- Formal verification in layers, as ADR 0014 records.
  Kani proves the decision cores, and Lean and Verus prove that the C runtime parsers read back every encoded command line.
  Models in `TLA+`, Quint, Alloy, and Prolog check output draining, the child lifecycle, the handle transfer, and the program search.
  `cargo xtask proofs` runs them, and Windows tests replay the models' vectors and traces through the crate.
- Seeded random command lines and environments that Windows tests pass through the crate into children, and `cargo xtask verifier`, which runs the Windows tests under `appverif.exe`.

### Changed

- Fallible calls return `SpawnError` instead of `io::Error`; it converts into `io::Error`, keeping the code Windows reported.
- Every spawn creates its process suspended and resumes it only after releasing the attribute list and the transferred duplicates.
  A release that fails terminates the process before it runs.
- `Child::kill` returns `Ok` once the process has exited, as `std::process::Child::kill` does.
- The mitigation, console, and creation-flag choices moved from `SpawnOptions` to `Command`; `SpawnOptions` lends only Jobs, an alternate parent, and a pseudoconsole.
- `AsPseudoConsole` is a safe trait returning a `BorrowedPseudoConsole`, and `SpawnOptions::pseudo_console` replaces `SpawnOptions::pseudoconsole`.
- `MitigationPolicy::dep`, `dep_atl_thunk`, and `sehop` take `Dep`, `AtlThunkEmulation`, and `Sehop`; `FontDisable` and `font_disable` are `NonSystemFonts` and `non_system_fonts`.
- `SuspendedChild::primary_thread_handle` is `main_thread_handle`.
- `Job::assign` takes any process handle.
- `Debug` prints a `Command` as std does.
- `SuspendedChild::id` is `const`, like `Child::id`, and returns the identifier recorded at creation, so it cannot panic.

### Removed

- `DropPolicy`, replaced by `Command::own_process_tree`.
- `CreationFlags` and `SpawnOptions::creation_flags`, replaced by the `Command` setters.

### Fixed

- `Command::to_command_line` accepts a `PATH` set from a handle when a later operation replaces or removes it, as a spawn does.
- An attribute-list size beyond one allocation returns an error instead of panicking.

## [0.1.0] - 2026-08-03

Initial release.

### Added

- Reusable `Command` and borrowed per-spawn capabilities.
- Transactional process creation with rollback; private running and suspended typestates make a mismatched internal transition unrepresentable.
- Explicit handle lists, handle arguments and environment values, alternate parents, ordered Jobs, typed mitigation policies, and ConPTY.
  Pseudoconsole creation sets `STARTF_USESTDHANDLES` with null standard handles, as Microsoft Terminal's `ConptyConnection.cpp` does, so the child cannot use handles the parent redirected.
- Owned standard streams, cached exit status, and concurrent output draining; both readers are joined even if one fails or panics.
- `SuspendedChild` with pre-resume process and primary-thread inspection; `resume` requires a previous suspend count of exactly one and rolls back otherwise.
- Crate documentation from `docs/crate.md`; the README example runs as a doctest.
- Examples and ADRs in the package.
- Rust 1.75, cross-architecture, public API, coverage, mutation, supply-chain, license, spelling, package, and CodeQL gates; `clippy::undocumented_unsafe_blocks` is denied.

### Removed

- `InheritableHandle` and `SpawnOptions::inherit_handle`.
  Handles transfer only through standard I/O or the argument and environment protocol, and inheritable duplicates live only during the spawn.

### Known limitations

- Windows keeps a process-wide reverse inheritance race: an unrelated broad-inheritance spawn can receive a short-lived inheritable duplicate.
  Avoid concurrent broad inheritance when transferred handles are sensitive; see [ADR 0005](docs/adr/0005-handle-transfer-and-reverse-race.md).
- `sha2` stays at 0.10 because 0.11 requires Rust 1.85; the checksum helper builds with both.

[Unreleased]: https://github.com/P4suta/windows-spawn/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/P4suta/windows-spawn/releases/tag/v0.1.0
