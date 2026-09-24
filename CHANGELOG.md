# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow Semantic Versioning with Cargo's pre-1.0 rules.

## [Unreleased]

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
