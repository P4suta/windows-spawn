# 0007 — Process creation is transactional

Status: accepted (2026-08-02)

## Context

A spawn can acquire pipes, null handles, local and remote duplicates, a private Job, attribute storage, and process and thread handles.
Any acquisition can fail; cleanup state spread across error sites allows leaks and double closes.

## Decision

`SpawnTransaction` exclusively owns temporary resources.
Before commit, its `Drop` terminates a created process and rolls back everything else.
Commit moves only the process handle, public pipe ends, lifecycle policy, and retained Job into `Child`; the thread and temporary handles close.

Private `Running` and `Suspended` markers parameterize the plan and the transaction.
Each state exposes only its own commit, so a wrong commit does not compile.
`HandleTransfer` keeps the effective parent, local or remote duplicates, and the inheritance list together.

`SuspendedChild` is the suspended state.
Its consuming `resume` is the only transition to `Child`; dropping it first terminates the process or its private Job.
Resume succeeds only if `ResumeThread` reports a previous suspend count of exactly one; otherwise the process rolls back.

## Consequences

- Raw handles have no shared owners.
- No partially initialized state is public.
- The type system selects running and suspended commits.
- Cleanup follows ownership, not error-site flags.
- Failure-injection and handle-count tests verify rollback without exposing internals.
