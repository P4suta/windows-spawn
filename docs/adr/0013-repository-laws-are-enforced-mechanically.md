# 0013 — Repository laws are enforced mechanically

Status: accepted (2026-09-25)

## Context

Rules kept by review alone erode: timed waits, explanatory comments, and silent conversions came back after they were removed.
Rust 1.75, the MSRV, has neither `#[expect]` nor `reason =`, so a lint exception can neither state its reason nor prove it is still needed.

## Decision

Every law has a mechanism, and CI runs it.

- Code fails loudly and converts explicitly.
  `[workspace.lints]` denies the Clippy `all`, `pedantic`, `nursery`, and `cargo` groups and a restriction set: no `unwrap`, `expect`, `panic`, indexing, unchecked arithmetic, `as`, or discarded `Result`.
  `clippy.toml` permits `unwrap`, `expect`, panics, and indexing in tests only.
- Time decides nothing (ADR 0010).
  `clippy.toml` bans the time types, timed waits, and file times.
  `cargo xtask gates` bans time-named identifiers, delay commands in strings, and `WaitForSingleObject` with a bound other than `INFINITE` or `0`.
- Reasons belong in commit messages.
  The gate allows doc comments and whole-line `// SAFETY:` runs, and nothing else.
- Every lint exception is registered.
  The gate collects each `#[allow]` lint and requires an entry with the same file, lint, and count, with a reason, in `xtask/src/gates.rs`.
  An entry no attribute uses also fails.
- Defaults are chosen explicitly.
  The gate bans `#[default]`; a documented `impl Default` states the choice.
- Types are closed or generic.
  The gate bans `Box<dyn …>`; a closed set is an enum, and an open one is a type parameter.
- Nothing escapes ownership or mutates process state.
  `clippy.toml` bans `mem::forget`, `process::exit`, `env::set_var`, and `env::remove_var`.

The gate is a lexer in xtask with no dependencies, because `syn` needs `unicode-ident`, whose license `deny.toml` does not allow.
Rustc lints are limited to those Rust 1.75 recognizes.

## Consequences

- Changing a law changes the lint table, `clippy.toml`, or the gate, with its tests.
- Pointer-integer conversions keep `as` behind registered exceptions; there is no alternative before Rust 1.84.
- When the MSRV reaches Rust 1.81, `#[expect(…, reason = …)]` replaces the registry.
