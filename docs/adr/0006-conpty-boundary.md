# 0006 — ConPTY integration is a borrowed capability

Status: accepted (2026-08-02)

## Context

The crate must attach an existing pseudoconsole without depending on a terminal
library or owning its `HPCON`. A safe raw constructor could accept invalid or
prematurely closed values.

## Decision

Use the unsafe `AsPseudoConsole` trait. Implementors guarantee a stable,
nonzero, live `HPCON` for the full borrow and retain ownership. The raw method
is public so terminal libraries can implement the bridge.

`conpty-oxide` depends on windows-spawn and implements the trait. It owns
ConPTY creation, pipes, waits, Tokio integration, and lifecycle. windows-spawn
owns command lowering, attributes, Jobs, and `CreateProcessW`.

The builder snapshots the `HPCON` numeric value and keeps only a lifetime
marker, so the stored options do not require dynamic dispatch. During process
creation ConPTY has no ordinary standard-handle set: the zero-initialized
startup fields remain zero and `STARTF_USESTDHANDLES` is not set, matching the
Microsoft ConPTY creation sequence.

## Consequences

- Ordinary users pass a safe borrow and do not construct raw `HPCON` values.
- windows-spawn does not depend on a terminal library.
- Pseudoconsole use conflicts with explicit standard streams and leaves the
  ordinary startup handles unused.
