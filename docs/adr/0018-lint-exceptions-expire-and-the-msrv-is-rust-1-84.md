# 0018 — Lint exceptions expire, and the MSRV is Rust 1.84

Status: accepted (2026-10-13)

## Context

Rust 1.75 has neither `#[expect]` nor `reason =`, so ADR 0013 registered every `#[allow]` in `xtask/src/gates.rs` with a reason and a count.
A registered exception could still outlive its need, and three remained: the `unsafe_code` of `sys`, the `as` that read a handle's value, and a probe's `process::exit`.
Rust 1.75 also has no pointer `addr`, so reading a handle's value took `as`, and the FFI calls borrowed each local as a reference before it became a pointer.
Rust 1.84 has `#[expect]` with `reason =`, `addr` and `ptr::without_provenance_mut`, `&raw mut`, and a resolver that keeps dependencies within the MSRV.

windows-sys 0.61, the crate's only Windows dependency, requires Rust 1.71, and the `windows` crate 0.62 requires Rust 1.82.
windows-spawn adds convenience over these bindings, so a project that must build with an older Rust can call windows-sys directly.

## Decision

- The MSRV is Rust 1.84, and only the maintainer moves it.
- Every lint exception is an `#[expect(lint, reason = "…")]` on the one item it concerns.
  Clippy's `allow_attributes` and `allow_attributes_without_reason` refuse an `#[allow]` and an exception without a reason.
  `unfulfilled_lint_expectations` fails an exception the code no longer needs.
  The gate refuses every `allow` it reads, including the inner attributes that Clippy skips, and the registry goes.
- A handle converts through its address alone.
  `addr` reads its value, and `ptr::without_provenance_mut` makes one from a value, so Kani proves the round trip for every `isize`.
- An FFI call takes a pointer to a local as `&raw mut` or `&raw const`, which makes the pointer without an intermediate reference, as Clippy's `borrow_as_ptr` requires.
- The workspace uses resolver 3, so `cargo update` keeps the dependencies within the MSRV.
- `.github/renovate.json` keeps Renovate off the numeric Rust toolchain in CI, which is the MSRV.
  The organization's preset automerges minor updates, and its open update moves the MSRV check from Rust 1.75 to the newest release.

Rejected:

- Rust 1.75 and its registry, which can't tell when an exception stops applying.
- Rust 1.81, which has `#[expect]` but not `addr`, so reading a handle's value would keep `as`.
- Rust 1.82, the MSRV of the `windows` crate, for the same reason.
- An MSRV that follows windows-sys, which would keep the registry for projects that can call windows-sys directly.

## Consequences

- A project on a Rust older than 1.84 stays on windows-spawn 0.1 or calls windows-sys directly.
- The `Command` setters that only record a choice are `const fn`, which Rust 1.83 allows for `&mut self`.
- No `allow` remains in the repository.
