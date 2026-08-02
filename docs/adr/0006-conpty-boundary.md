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

## Consequences

- Ordinary users pass a safe borrow and do not construct raw `HPCON` values.
- windows-spawn does not depend on a terminal library.
- Pseudoconsole use conflicts with explicit standard streams and creates the
  process with invalid ordinary standard handles, as required by ConPTY.
