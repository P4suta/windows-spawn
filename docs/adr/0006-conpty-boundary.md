# 0006 — ConPTY integration is a borrowed capability

Status: accepted (2026-08-02)

## Context

The crate must attach an existing pseudoconsole without depending on a terminal library or owning its `HPCON`.
A safe raw constructor could accept invalid or closed values.

## Decision

Use the unsafe `AsPseudoConsole` trait.
Implementors guarantee a live, nonzero `HPCON` for the whole borrow and keep ownership.
The raw method is public so terminal libraries can implement it.

The terminal library owns pseudoconsole creation, pipes, waits, runtime integration, and lifecycle.
windows-spawn owns command lowering, attributes, Jobs, and `CreateProcessW`, and does not depend on or test against any terminal library.

The builder snapshots the `HPCON` value and keeps a lifetime marker, avoiding dynamic dispatch.
Creation sets `STARTF_USESTDHANDLES` with `hStdInput`, `hStdOutput`, and `hStdError` null.
No standard handle is listed in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`; with no other transfer, `bInheritHandles` is `FALSE`.

This follows Microsoft Terminal's `ConptyConnection`.
The Microsoft Learn sample leaves `STARTF_USESTDHANDLES` clear; in a parent with redirected standard I/O, the child can then use the parent's pipes instead of the pseudoconsole.
Null slots make the boundary deterministic and testable.

## Consequences

- Users pass a safe borrow and never build a raw `HPCON`.
- Dependencies point from the terminal library to windows-spawn only.
- A pseudoconsole conflicts with explicit standard streams, and the ordinary startup handles are null.

## References

- [Microsoft Terminal `ConptyConnection.cpp`](https://github.com/microsoft/terminal/blob/fbda436dc654cf551dd196b2667ef95d3e0a7262/src/cascadia/TerminalConnection/ConptyConnection.cpp)
- [Creating a Pseudoconsole session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
