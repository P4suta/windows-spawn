# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow Semantic Versioning with Cargo's pre-1.0 rules.

## [Unreleased]

## [0.1.1](https://github.com/P4suta/windows-spawn/compare/v0.1.0...v0.1.1) - 2026-09-26

### Added

- render the command line for launch brokers ([#17](https://github.com/P4suta/windows-spawn/pull/17))

### Other

- add the required aggregate job ([#29](https://github.com/P4suta/windows-spawn/pull/29))
- release only from a signed tag and an approved deployment ([#27](https://github.com/P4suta/windows-spawn/pull/27))
- *(deps)* move dependency updates from Dependabot to Renovate ([#25](https://github.com/P4suta/windows-spawn/pull/25))
- enforce the repository laws mechanically ([#24](https://github.com/P4suta/windows-spawn/pull/24))
- inject Win32 failures to cover every error path ([#23](https://github.com/P4suta/windows-spawn/pull/23))
- run mutation testing on rust-mutants ([#22](https://github.com/P4suta/windows-spawn/pull/22))
- drain ConPTY output continuously and ban time in code ([#21](https://github.com/P4suta/windows-spawn/pull/21))
- prove lifecycle properties by events, not time ([#20](https://github.com/P4suta/windows-spawn/pull/20))
- make all prose terse and one sentence per line ([#18](https://github.com/P4suta/windows-spawn/pull/18))

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
