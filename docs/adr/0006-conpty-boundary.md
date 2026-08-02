# 0006 — ConPTY integration is a borrowed capability

Status: accepted (2026-08-02)

## Context

windows-spawn must attach an existing pseudoconsole without depending on one
particular terminal library or taking ownership of its `HPCON`. Exposing a raw
safe constructor would let callers supply invalid or prematurely closed values.

## Decision

The boundary is the unsafe trait `AsPseudoConsole`. Implementors guarantee that
the returned pseudoconsole value remains valid for the borrow and that ownership
is not transferred. `conpty-oxide::Pcon` implements the trait once; its users
pass a normal borrow through `SpawnOptions::pseudoconsole`.

`AsPseudoConsole::raw_pseudoconsole` is visible in rustdoc so external terminal
libraries can implement the bridge without relying on hidden API. Its safety
contract requires a stable, nonzero, live `HPCON` for the complete borrow.

The dependency is one-way: conpty-oxide depends on windows-spawn. It retains ConPTY
creation, its pipes, registered waits, Tokio integration, and lifecycle API;
windows-spawn owns command lowering, attributes, Jobs, and `CreateProcessW`.

## Consequences

- Ordinary users do not construct raw HPCON values or write unsafe code.
- windows-spawn has no dependency on or knowledge of conpty-oxide.
- A pseudoconsole conflicts with explicit standard streams during planning,
  and the process starts with invalid ordinary standard handles as required by
  ConPTY.
