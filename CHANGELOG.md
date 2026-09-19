# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Semantic
Versioning with Cargo's pre-1.0 compatibility rules.

## [Unreleased]

## [0.2.0] - 2026-09-19

### Changed

- Replaced the spawn transaction with consuming states from validated intent
  through suspended creation, reclamation, and resume. Ordinary spawns are now
  created suspended internally and resume only after temporary resources are
  reclaimed.
- Replaced `io::Result` at the public boundary with typed validation, Windows,
  and nonempty cleanup errors. Converting the typed error to `io::Error`
  preserves it as the source.
- Replaced boolean mitigation choices, drop policy, terminal flags, and free
  creation flags with closed policy enums and dedicated setters.
- Bound pseudoconsole values to their owner lifetime through
  `BorrowedPseudoConsole` and the unsafe `AsPseudoConsole` contract.

### Added

- Typed resource kinds and handle-table markers, pinned process-attribute
  backing, explicit process-tree cleanup, and a single-operation unsafe lint.
- An OComment-backed repository comment policy built from pinned source and a
  typed invariant registry whose enforcement values are limited to type,
  const, lint, and Kani.

### Removed

- `DropPolicy`, `CreationFlags`, raw pseudoconsole values, and boolean DEP,
  ATL-thunk, and SEHOP setters. No compatibility shims are provided.

## [0.1.0] - 2026-08-03

Initial release.

### Added

- Reusable `Command` and borrowed per-spawn capabilities.
- Transactional process creation with automatic rollback, built on private
  running/suspended typestates that make a mismatched internal transition
  unrepresentable.
- Explicit handle lists, high-level handle arguments and environment values,
  alternate parents, ordered Jobs, typed mitigation policies, and ConPTY.
  Pseudoconsole creation sets `STARTF_USESTDHANDLES` with all three standard
  handles null, matching Microsoft Terminal's `ConptyConnection.cpp`, so a
  hosted child cannot fall back to standard handles the parent redirected.
- Owned standard streams, cached exit status, and concurrent output draining.
  Both reader threads are joined even when one reader fails or panics.
- `SuspendedChild` type-state transitions with pre-resume process and primary
  thread inspection. `resume` requires the primary thread's previous suspend
  count to be exactly one; externally changed counts fail and roll back.
- Crate documentation backed by `docs/crate.md`, with the README compiled and
  executed as a doctest so its example cannot drift from the API.
- Packaged lifecycle examples and ADRs.
- Rust 1.75, cross-architecture, public API, coverage, mutation, supply-chain,
  license, spelling, package, and CodeQL gates.
  `clippy::undocumented_unsafe_blocks` is denied, making CONTRIBUTING's
  "every `unsafe` block carries a specific safety justification" rule
  machine-checked instead of a convention.

### Removed

- The arbitrary `InheritableHandle` escape hatch and
  `SpawnOptions::inherit_handle`. Handles are transferred through standard
  I/O or the argument/environment protocol, with inheritable duplicates limited
  to the spawn transaction.

### Known limitations

- Windows retains a process-wide reverse inheritance race: unrelated
  broad-inheritance spawns can receive a short-lived inheritable duplicate.
  Avoid concurrent broad inheritance when transferred handles are sensitive.
  See [ADR 0005](docs/adr/0005-handle-transfer-and-reverse-race.md).
- The `sha2` bump to 0.11 stays deferred because it requires Rust 1.85, above
  this crate's 1.75 minimum. The release-artifact checksum helper builds
  against both 0.10 and 0.11.

[Unreleased]: https://github.com/P4suta/windows-spawn/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/P4suta/windows-spawn/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/P4suta/windows-spawn/releases/tag/v0.1.0
