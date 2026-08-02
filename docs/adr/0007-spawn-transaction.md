# 0007 — Process creation is transactional

Status: accepted (2026-08-02)

## Context

A spawn can acquire pipes, null handles, local and remote duplicates, a private
Job, attribute storage, and process/thread handles. Any acquisition can fail;
distributed cleanup state permits leaks and double closes.

## Decision

`SpawnTransaction` exclusively owns temporary resources. Before commit, its
`Drop` implementation terminates a created process and rolls back all state.
Commit moves only the process handle, public pipe endpoints, lifecycle policy,
and retained Job ownership into `Child`; thread and temporary handles close.

`SuspendedChild` represents the suspended state. Its consuming `resume` is the
only normal transition to `Child`; dropping it first terminates the process or
its private Job.

## Consequences

- No shared ownership of raw handles.
- No public partial-initialization state.
- Cleanup follows ownership instead of error-site flags.
- Failure-injection and handle-count tests can verify rollback without exposing
  transaction internals.
