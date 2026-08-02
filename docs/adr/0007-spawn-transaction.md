# 0007 — Process creation is transactional

Status: accepted (2026-08-02)

## Context

A single spawn can acquire pipes, null handles, local inheritable duplicates,
remote duplicates, a private Job, attribute storage, and process/thread handles.
Failures can happen between any two acquisitions. Distributed cleanup flags
make double-close and leak states representable.

## Decision

`SpawnTransaction` is the sole owner of temporary resources. Before commit, its
Drop implementation rolls everything back and terminates any created process.
After a successful create and Job setup, `commit` moves only the process handle,
public pipe endpoints, cached lifecycle policy, and any required Job ownership
into `Child`. Thread and temporary handles remain transaction-owned and close
immediately.

`SuspendedChild` is a separate state. Its consuming `resume` is the only normal
transition to `Child`; Drop before that transition always terminates the process
or its private Job.

## Consequences

- No raw handle has shared ownership.
- Partial initialization is not observable through the public API.
- Cleanup follows ownership rather than error-site bookkeeping.
- Failure-injection and handle-count integration tests can verify the invariant
  without exposing transaction internals.
