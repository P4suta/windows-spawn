# 0005 — Handle transfer and the reverse inheritance race

Status: accepted (2026-08-02)

## Context

`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits what the child inherits, but every listed handle must be inheritable during `CreateProcessW`.
Making the source inheritable would change caller-owned state.
A temporary inheritable duplicate avoids that but can leak into a concurrent broad-inheritance spawn in the same process.

An alternate parent has its own handle table.
Standard streams and `arg_handle`/`env_handle` values must be duplicated into it before their child-visible values are known.

## Decision

Create inheritable local duplicates just before the spawn and close them when creation returns.
Document the reverse race.
Use no helper process; it would change parent identity and failure semantics.

Keep configured handle arguments and environment values as private duplicates, and duplicate and lower them for the effective parent on each spawn.
Reclaim remote temporaries with `DuplicateHandle` close-source semantics on success and failure.
Accept handles only through standard I/O or the argument/environment handoff protocol.

## Consequences

- Source handles are never made inheritable in place.
- No inheritable duplicate outlives a spawn.
- The target child cannot over-inherit; callers must avoid concurrent broad inheritance when transferred handles are sensitive.
- Each local and remote duplicate has one cleanup owner.
