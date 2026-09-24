# 0002 — windows-spawn owns its `CreateProcessW` call

Status: accepted (2026-08-02)

## Context

Stable `std::process::Command` cannot pass a `STARTUPINFOEXW` attribute list; its raw extension is nightly-only and unsafe.
A public attribute-list wrapper would split command-line lowering, standard I/O, inheritance, process handles, and rollback across APIs.

## Decision

`Command` stores intent, `SpawnPlan` validates it, and `SpawnTransaction` owns temporary OS resources.
Only the private `sys` module calls Win32.

Return `std::process::ExitStatus`, `std::process::Output`, and `std::io::Error`.
Use a separate `Child`, because stable std cannot adopt the transaction's process and pipe handles.

## Consequences

- Quoting, environment order, executable lookup, standard I/O, output draining, and exit codes are tested here.
- One transaction owns rollback for every creation error.
- Raw flags, attribute lists, and `windows-sys` types stay private.
- Revisit if std gains a safe, complete attribute interface.
