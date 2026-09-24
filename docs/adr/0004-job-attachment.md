# 0004 — Jobs are ordered process-creation capabilities

Status: accepted (2026-08-02)

## Context

`AssignProcessToJobObject` after creation leaves the child outside the Job for a while.
`PROC_THREAD_ATTRIBUTE_JOB_LIST` attaches Jobs during creation, root first.
Tree teardown must not alter a caller-owned Job.

## Decision

Repeated `SpawnOptions::job` calls keep root-to-innermost order.
`Job::assign` remains an explicit post-creation operation.
Job limit updates read the current limits and change only the requested flag.

`DropPolicy::KillTree` appends a private innermost Job; `DropPolicy::Detach` is the default.
`Child` owns a duplicate of any Job handle it keeps.

## Consequences

- Attachment is atomic by default and supports several Jobs.
- `wait_with_output` can terminate the private tree after the root exits, so pipes reach EOF.
- `Job::assign` is the weaker path and may fail under host Job restrictions.
