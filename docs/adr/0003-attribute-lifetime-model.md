# 0003 — Attribute storage belongs to the spawn transaction

Status: accepted (2026-08-02)

## Context

`UpdateProcThreadAttribute` keeps pointers until `CreateProcessW`.
The list needs aligned storage and each value a stable address.
The pseudoconsole is the exception: its `HPCON` value itself is `lpValue`.

## Decision

One `SpawnTransaction` owns the attribute list.
The backing allocation is word-aligned, each ordinary value has stable owned storage, and both outlive `DeleteProcThreadAttributeList`.
The pseudoconsole value is passed as its Win32 contract requires.

The native list pointer is derived from the backing allocation on each use; no second raw pointer is stored.
`SpawnOptions::pseudoconsole` snapshots the `HPCON` value and keeps the capability's lifetime with a private marker.

`SpawnOptions<'a>` carries the lifetime of borrowed Jobs, parent, and pseudoconsole; the reusable `Command` borrows none of them.

## Consequences

- No public raw attribute API or self-referential builder.
- Attribute pointers cannot outlive their values.
- The list is deleted before its values and storage.
- Moving `AttributeList` cannot leave a stale pointer.
- The unsafe lifetime proof stays in `sys`.
