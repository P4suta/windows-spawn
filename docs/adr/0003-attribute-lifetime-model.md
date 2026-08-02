# 0003 — Attribute storage belongs to the spawn transaction

Status: accepted (2026-08-02)

## Context

`UpdateProcThreadAttribute` retains pointers until `CreateProcessW`. The list
requires aligned storage and each value requires a stable address. The
pseudoconsole is an exception: its `HPCON` value, rather than its address, is
passed as `lpValue`.

## Decision

Keep the attribute list private to one `SpawnTransaction`. Give its backing
allocation word alignment and each normal value stable owned storage. Retain
both until after `DeleteProcThreadAttributeList`. Pass the borrowed
pseudoconsole value according to its Win32 contract.

`SpawnOptions<'a>` carries the lifetime of borrowed Jobs, parent process, and
pseudoconsole capabilities; reusable `Command` does not borrow them.

## Consequences

- No public raw attribute API or self-referential builder.
- Attribute pointers cannot outlive their values.
- Delete the list before its values and backing storage.
- Keep the unsafe lifetime proof inside `sys`.
