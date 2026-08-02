# 0008 — Mutation testing is a scheduled audited gate

Status: accepted (2026-08-02)

## Context

The Windows integration suite includes process trees, suspended processes, and
pipe EOF behavior. A complete mutation run is too expensive for every pull
request, but blanket exclusions would hide precisely the ownership bugs the
crate is intended to prevent.

## Decision

Four mutation shards run weekly and on manual dispatch. Every survivor is
either addressed by a focused test or individually excluded only when the
generated expression is provably identical for all values allowed at that
site. The narrow regular expressions live in `.cargo/mutants.toml`; whole files
and broad mutation classes are never excluded.

Tests that deliberately create a suspended process retain an independent
process handle and perform direct Win32 cleanup before reporting a failed
termination assertion. Cleanup must not call the crate path under mutation:
mutants that disable `Drop`, `Child::kill`, or `TerminateProcess` must not leave
a permanently suspended process behind on the test host.

Both the local full run and the CI shards start `cargo mutants` suspended,
assign it to a kill-on-close Windows Job, and only then resume it. Closing the
runner's last Job handle therefore removes descendants that escape a mutant,
timeout, or aborted test process. Local runs use a unique ignored output
directory; CI keeps `mutants.out` at the workspace root for artifact upload.

## Recorded equivalences

- `MitigationPolicy::replace` clears both destination bits before inserting the
  new value. OR and XOR therefore receive zero on the left at those positions
  and are identical.
- `DUPLICATE_SAME_ACCESS` and `DUPLICATE_CLOSE_SOURCE` occupy disjoint bits, so
  OR and XOR produce the same remote-close option word.
- `PROCESS_CREATE_PROCESS` and `PROCESS_DUP_HANDLE` occupy disjoint bits, so OR
  and XOR request the same minimal alternate-parent rights.
- Public `CreationFlags` cannot contain `CREATE_UNICODE_ENVIRONMENT`, which is
  private and added by `create_process`. OR and XOR therefore produce the same
  word at that insertion point.

## Consequences

Mutation results remain actionable, expensive process tests run off the pull
request path, and every accepted equivalent stays reviewable beside the design
reason that makes it safe.
