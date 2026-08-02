# 0004 — Jobs are ordered process-creation capabilities

Status: accepted (2026-08-02)

## Context

Post-creation `AssignProcessToJobObject` leaves a window in which the child is
outside the Job. `PROC_THREAD_ATTRIBUTE_JOB_LIST` attaches Jobs as part of
creation and accepts an ordered list from root to innermost.

Tree teardown is also a separate semantic choice. A boolean "kill on drop"
would obscure whether it changes a caller-owned Job or creates library-owned
state.

## Decision

`SpawnOptions::job` may be called repeatedly and preserves root-to-inner order.
`Job::assign` remains the explicit post-creation escape hatch. Job limit
updates first query the existing extended limits and replace only the requested
flag.

`DropPolicy::KillTree` creates a private windows-spawn-owned Job and appends it as
the innermost Job. `DropPolicy::Detach` is the default and matches std. A Child
never shares raw ownership of a Job handle; it owns a duplicate when continued
ownership is required.

## Consequences

- Atomic attachment is the normal route and multiple Jobs remain composable.
- `wait_with_output` can terminate the private tree after root exit so inherited
  pipe handles held by descendants cannot prevent EOF indefinitely.
- Calling `Job::assign` after spawn is visibly weaker and may fail under host
  Job restrictions.
