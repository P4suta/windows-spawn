# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow Semantic Versioning with Cargo's pre-1.0 rules.

## [Unreleased]

## [0.2.0](https://github.com/P4suta/windows-spawn/compare/v0.1.0...v0.2.0) - 2026-10-24

### Added

- [**breaking**] design the 0.2 API and prove the spawn decisions ([#33](https://github.com/P4suta/windows-spawn/pull/33))
- render the command line for launch brokers ([#17](https://github.com/P4suta/windows-spawn/pull/17))

### Other

- verify Context7 library ownership ([#58](https://github.com/P4suta/windows-spawn/pull/58))
- *(ci)* use purpose-scoped Doppler credentials ([#55](https://github.com/P4suta/windows-spawn/pull/55))
- *(deps)* update all non-major dependencies ([#30](https://github.com/P4suta/windows-spawn/pull/30))
- select checks and preserve Rust 1.84 compatibility ([#54](https://github.com/P4suta/windows-spawn/pull/54))
- remove generated development metadata ([#51](https://github.com/P4suta/windows-spawn/pull/51))
- target hosted assurance at native safety boundaries ([#49](https://github.com/P4suta/windows-spawn/pull/49))
- *(deps)* lock file maintenance ([#47](https://github.com/P4suta/windows-spawn/pull/47))
- record the reason for the Rust 1.84 MSRV ([#46](https://github.com/P4suta/windows-spawn/pull/46))
- [**breaking**] require Rust 1.84 and let lint exceptions expire ([#43](https://github.com/P4suta/windows-spawn/pull/43))
- cut the nightly deep verification to about an hour ([#41](https://github.com/P4suta/windows-spawn/pull/41))
- *(deps)* lock file maintenance ([#31](https://github.com/P4suta/windows-spawn/pull/31))
- stress, fuzz, and run the spawn where bounded tests stop ([#39](https://github.com/P4suta/windows-spawn/pull/39))
- make spawn state and handle types explicit ([#36](https://github.com/P4suta/windows-spawn/pull/36))
- add the required aggregate job ([#29](https://github.com/P4suta/windows-spawn/pull/29))
- release only from a signed tag and an approved deployment ([#27](https://github.com/P4suta/windows-spawn/pull/27))
- *(deps)* move dependency updates from Dependabot to Renovate ([#25](https://github.com/P4suta/windows-spawn/pull/25))
- enforce the repository laws mechanically ([#24](https://github.com/P4suta/windows-spawn/pull/24))
- inject Win32 failures to cover every error path ([#23](https://github.com/P4suta/windows-spawn/pull/23))
- run mutation testing on rust-mutants ([#22](https://github.com/P4suta/windows-spawn/pull/22))
- drain ConPTY output continuously and ban time in code ([#21](https://github.com/P4suta/windows-spawn/pull/21))
- prove lifecycle properties by events, not time ([#20](https://github.com/P4suta/windows-spawn/pull/20))
- make all prose terse and one sentence per line ([#18](https://github.com/P4suta/windows-spawn/pull/18))

The 0.2 API follows `std::process` and places each choice on the type that owns it, as ADR 0015 records; `docs/migration-0.2.md` maps every 0.1 item.

### Added

- `Command::own_process_tree`, which makes `Child::kill`, dropping the `Child`, and output collection end the child's whole process tree.
- `Child::wait_tree`, which waits until every process of an owned tree has exited.
- `Command::console` with `ConsoleMode`, `Command::priority` with `PriorityClass`, and one `Command` setter for each creation flag.
- `SpawnError`, which keeps whether input was rejected, which Windows operation failed in which phase, and which releases failed; `windows_spawn::Result`.
- `BorrowedPseudoConsole` with `borrow_raw`, for a raw `HPCON`.
- `From<ChildStdin>`, `From<ChildStdout>`, `From<ChildStderr>`, `From<io::Stdout>`, and `From<io::Stderr>` for `Stdio`; `Write for &ChildStdin`; `AsRawHandle` for the handle types.
- `ValidationError::CommandLineTooLong` and `ValidationError::PseudoConsoleWithConsoleMode`, which reject a spawn before `CreateProcessW` runs.
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
- The MSRV is Rust 1.84, as ADR 0018 records.
- `Command::own_process_tree`, `console`, `priority`, `mitigation`, and the creation-flag setters are `const fn`.

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
