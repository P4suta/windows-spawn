# 0008 — Mutation testing is a scheduled audited gate

Status: superseded by ADR 0011 (2026-09-25)

## Context

Process-tree, suspended-process, and pipe-EOF tests make a full mutation run too slow for every pull request.
Broad exclusions would hide ownership and cleanup faults.

## Decision

Run four shards weekly and on manual dispatch.
Kill each survivor with a test, or exclude its exact expression only if it is equivalent for every value possible at that site.
`.cargo/mutants.toml` may not exclude a whole file or mutation class.

Suspended-process tests keep an independent process handle and clean up with direct Win32 calls, so a mutated `Drop`, `Child::kill`, or `TerminateProcess` cannot leave a process on the runner.

The xtask starts `cargo mutants` through windows-spawn with `DropPolicy::KillTree`, whose Job contains descendants on exit, timeout, failure, or runner termination.
Local runs write to a unique ignored directory; CI uploads `mutants.out`.

The four exclusions are equivalent because:

- `MitigationPolicy::replace` clears the destination bits before OR or XOR.
- `DUPLICATE_SAME_ACCESS` and `DUPLICATE_CLOSE_SOURCE` are disjoint bits.
- `PROCESS_CREATE_PROCESS` and `PROCESS_DUP_HANDLE` are disjoint bits.
- Public `CreationFlags` cannot contain the private `CREATE_UNICODE_ENVIRONMENT` bit that `create_process` adds.

## Consequences

Results stay actionable, the slow suite stays off pull requests, and each exclusion has a reviewable equivalence proof.
