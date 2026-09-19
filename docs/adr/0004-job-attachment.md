# 0004 — Jobs are ordered process-creation capabilities

Status: accepted (2026-08-02)

## Context

Post-creation `AssignProcessToJobObject` leaves the child outside the Job for a
time. `PROC_THREAD_ATTRIBUTE_JOB_LIST` attaches Jobs during creation in
root-to-innermost order. Tree teardown must not change a caller-owned Job.

## Decision

Repeated `SpawnOptions::job` calls preserve root-to-innermost order.
`Job::assign` remains an explicit post-creation operation. Job limit updates
query existing limits and change only the requested flag.

`JobClosePolicy::TerminateProcesses` appends a private, crate-owned innermost
Job. `JobClosePolicy::PreserveProcesses` remains the default. `Child` owns the
private Job and process together in the owner that enforces their close order.

## Consequences

- Atomic attachment is the default and supports multiple Jobs.
- `wait_with_output` can terminate the private tree after root exit, ensuring
  EOF when descendants retain pipe handles.
- `Job::assign` exposes the weaker post-creation path and may fail under host
  Job restrictions.
