# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Semantic
Versioning with Cargo's pre-1.0 compatibility rules.

## [Unreleased]

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
