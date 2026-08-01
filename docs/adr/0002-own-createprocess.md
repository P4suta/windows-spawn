# 0002 — spawnkit owns its `CreateProcessW` call

Status: accepted (2026-08-01)

## Context

The obvious cheaper design is to build the attribute list and then hand it to
`std::process::Command`, inheriting std's argument quoting, environment block
construction, stdio plumbing and `Child`.

Stable std does not allow this. `std::os::windows::process::ProcThreadAttributeList`
and `CommandExt::raw_attribute` are gated behind the unstable feature
`windows_process_extensions_raw_attribute` (tracking issue rust-lang/rust#114854),
open since 2023, with "Creating safe interface for setting attributes" still
listed as an unresolved question. The stable surface — `creation_flags`,
`raw_arg`, `async_pipes` — has no way to reach `STARTUPINFOEXW::lpAttributeList`.
There is no escape hatch to inject one.

## Decision

`spawnkit` calls `CreateProcessW` itself, and owns the command line, the
environment block, the standard handles and the resulting process handle.

## Consequences

- We re-implement, and must test, the parts std already gets right:
  `CommandLineToArgvW`-compatible quoting, the sorted NUL-separated environment
  block, `NUL`/pipe stdio setup, and exit-code retrieval.
- `Child` is our own type, not `std::process::Child`. It exposes `AsHandle` and
  `into_raw_handle` so it can be handed to other crates.
- If the std feature ever stabilises with a safe interface, this ADR should be
  revisited — the layer might collapse into a thin `CommandExt` shim.
- Owning the call is also what makes ADR 0003 possible: the attribute list and
  the `CreateProcessW` invocation are in the same function, so one lifetime can
  span both.
