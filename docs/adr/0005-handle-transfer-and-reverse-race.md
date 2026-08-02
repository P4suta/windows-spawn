# 0005 — Handle transfer and the reverse inheritance race

Status: accepted (2026-08-02)

## Context

`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits what the target child inherits, but
Windows requires listed handles to be inheritable while `CreateProcessW` runs.
Changing the source handle's flag would mutate caller-owned state. A temporary
inheritable duplicate avoids that mutation, but another concurrent broad
inheritance spawn in the same process can still inherit the duplicate.

A selected parent process introduces another handle table. Standard streams and
high-level `arg_handle`/`env_handle` values must be duplicated into that table
before their numeric values have meaning.

## Decision

windows-spawn makes inheritable local duplicates immediately before spawn and keeps
their lifetime as short as possible. It documents, but cannot eliminate, the
reverse race. The 0.1 series does not introduce a helper process because doing
so changes parent identity and failure semantics.

High-level handle arguments and environment values are privately duplicated at
configuration time, then remotely duplicated and lowered only for the selected
parent. Remote temporaries are reclaimed with `DuplicateHandle` close-source
semantics on both success and failure. Arbitrary pre-inheritable handles are not
accepted by the public API: handles enter a child only as standard I/O or
through the argument/environment handoff protocol.

## Consequences

- Source handles are never made inheritable in place.
- The public API cannot keep an arbitrary inheritable duplicate alive between
  spawn calls.
- Target-child over-inheritance is prevented; process-wide reverse leakage must
  still be considered by applications that concurrently use broad inheritance.
- Ownership and cleanup of both local and remote duplicates are deterministic.
