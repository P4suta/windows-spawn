# 0002 — windows-spawn owns its `CreateProcessW` call

Status: accepted (2026-08-02)

## Context

Stable `std::process::Command` cannot receive a `STARTUPINFOEXW` attribute
list. Its raw-attribute extension is nightly-only and unsafe. Handing a public
attribute-list wrapper to callers would also split ownership of command-line
lowering, standard streams, inheritance, process handles, and rollback across
two APIs.

## Decision

windows-spawn owns the entire call. `Command` stores reusable intent, `SpawnPlan`
performs pure validation and normalization, and `SpawnTransaction` acquires all
temporary OS resources. The private `sys` layer is the only place that calls
Win32.

The public result uses `std::process::ExitStatus`, `std::process::Output`, and
`std::io::Error`. `Child` is a distinct owning process type because stable std
cannot adopt the process and pipe handles produced by this transaction.

## Consequences

- windows-spawn must test Windows argument quoting, environment ordering, executable
  lookup, standard streams, output draining, and exit-code preservation.
- Every process-creation failure has one rollback owner.
- Raw Win32 flags, attribute lists, and `windows-sys` types stay private.
- If std stabilizes a safe, sufficiently complete attribute interface, this
  decision can be revisited without changing the high-level capability types.
