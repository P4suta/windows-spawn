# 0004 — Job attachment: `JOB_LIST` attribute vs `AssignProcessToJobObject`

Status: accepted (2026-08-01)

## Context

There are two ways to put a new process into a job object.

`AssignProcessToJobObject` after `CreateProcessW` is what `win32job` and
`process-wrap` do, and it is the only option when the job is chosen after the
fact. It has a window: between creation and assignment the child is outside the
job, and even a `CREATE_SUSPENDED` child has already had the loader map its
image. If the child was created inside another job by something else, or if it
has already spawned, the assignment can fail outright.

`PROC_THREAD_ATTRIBUTE_JOB_LIST` puts the process in the job as part of
creation. There is no window at all. The cost is that the job must exist and be
chosen before the spawn, and the attribute takes an array of job handles whose
lifetime must span the `CreateProcessW` call (ADR 0003).

## Decision

Support both, and make the attribute path the recommended one.
`WindowsCommand::attach_to_job(&'a Job)` uses the attribute;
`Job::assign(&Child)` is the post-hoc escape hatch.
`kill_tree_on_drop()` is built on the attribute path plus
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, not on walking the process tree by PID —
PID walking races against PID reuse and misses re-parented grandchildren.

## Consequences

- `Job::from_handle(OwnedHandle)` adopts a job created by `win32job` or
  `process-wrap`, so those crates keep owning the limit configuration and
  `spawnkit` only contributes atomic attachment.
- `spawnkit` deliberately exposes almost none of the job API: only creation,
  adoption, `kill_on_close` and `assign`. CPU/IO rate control, completion ports
  and notification limits stay out of scope (ADR 0001).
- A child can be in a job via the attribute *and* be assigned to another later;
  nested jobs are legal on Windows 8+, and the error cases are the caller's to
  handle.
