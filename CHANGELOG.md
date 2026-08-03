# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Semantic
Versioning with Cargo's pre-1.0 compatibility rules.

## [Unreleased]

### Added

- A compiled, executed `Command` documentation example. The crate previously had
  only `compile_fail` boundary pins, so no usage example was ever type-checked.
- `clippy::undocumented_unsafe_blocks` is denied, making CONTRIBUTING's
  "every `unsafe` block carries a specific safety justification" rule
  machine-checked instead of a convention.
- A `_typos.toml` so the spell-check gate has a checked-in configuration
  matching the sibling repositories.

### Changed

- Strengthened process creation with private running/suspended typestates,
  unified handle-transfer ownership, and value-based pseudoconsole storage.
- Aligned ConPTY startup with Microsoft Terminal by setting
  `STARTF_USESTDHANDLES` while keeping all ordinary standard handles null.
- Require `SuspendedChild::resume` to observe the expected suspend count of
  exactly one; externally changed counts now fail and roll back the process.

### Fixed

- Prevented ConPTY children from reading or writing the parent's redirected
  standard streams instead of the pseudoconsole channels.
- Always join both output reader threads when output capture encounters a
  reader error or panic.
- The release-artifact checksum helper no longer relies on `LowerHex` being
  implemented for the digest output, so it builds against both `sha2` 0.10 and
  0.11. The bump itself stays deferred because `sha2` 0.11 requires Rust 1.85,
  above this crate's 1.75 minimum.

## [0.1.0] - 2026-08-02

### Added

- Reusable Command and borrowed per-spawn capabilities.
- Transactional process creation with automatic rollback.
- Explicit handle lists, high-level handle arguments and environment values,
  alternate parents, ordered Jobs, typed mitigation policies, and ConPTY.
- Owned standard streams, cached exit status, concurrent output draining, and
  SuspendedChild type-state transitions with pre-resume process and primary
  thread inspection.
- README-backed crate documentation and packaged lifecycle examples and ADRs.
- Rust 1.75, cross-architecture, public API, coverage, mutation, supply-chain,
  license, package, and CodeQL gates.

### Removed

- The arbitrary `InheritableHandle` escape hatch and
  `SpawnOptions::inherit_handle`. Handles are transferred through standard
  I/O or the argument/environment protocol, with inheritable duplicates limited
  to the spawn transaction.

[Unreleased]: https://github.com/P4suta/windows-spawn/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/P4suta/windows-spawn/releases/tag/v0.1.0
