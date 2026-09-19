# 0008 — Mutation testing is a pull-request gate

Status: accepted (2026-08-02)

## Context

Process-tree, suspended-process, and pipe-EOF behavior contains ownership and
cleanup faults that ordinary examples do not exercise. Mutation analysis must
therefore run against the same revision before merge.

## Decision

Run every generated mutant in four shards on each pull request and on manual
dispatch. Survivors and timeouts fail their shard. An equivalent mutant may be
excluded only after its exact source expression is registered to a successful
Kani proof artifact.

Suspended-process tests retain an independent process handle and use direct
Win32 cleanup before reporting a failed termination assertion. Cleanup bypasses
the mutated crate path so changes to `Drop`, `Child::kill`, or
`TerminateProcess` cannot leave a suspended process on the runner.

The xtask starts `cargo mutants` through `windows-spawn` with
`JobClosePolicy::TerminateProcesses`. The private kill-on-close Job contains
descendants on normal exit, timeout, test failure, or runner termination.
Local runs use a unique ignored output directory; CI writes the upload under
`mutants.out`.

## Consequences

Mutation results are part of the merge decision and no unproved exclusion can
silently remove production expressions from analysis.
