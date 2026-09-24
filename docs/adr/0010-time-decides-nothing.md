# 0010 — Time decides nothing

Status: accepted (2026-09-25)

## Context

Tests kept processes alive with `ping` and `Start-Sleep`, polled with sleeps, bounded waits with deadlines, and asserted elapsed times.
A slow host turned these into failures, and a fast one could hide faults.

## Decision

No code or test uses time as evidence.
Every wait ends on an event: a process exit, a pipe byte, or pipe EOF.
The only time bounds belong to the harness: CI `timeout-minutes` and the mutation tool's own limit.

`WaitForSingleObject(h, 0)` in `Child::try_wait` is a non-blocking state query, not a timed wait.

Tests use these patterns:

- A probe reports ready on a pipe before the test acts on it.
- A probe blocks on a gate the test holds; after a kill, the test releases the gate and requires the terminator's exit code.
- Every probe in a tree reports on gate EOF, so an empty report after the release proves the tree died.
- After a termination under test, the test resumes the primary thread; a surviving process then runs and fails fast.
- A resumed thread's suspend count is read directly.
- `try_wait` is checked after the test's own wait has returned.
- Where the property is that a call returns at all, a broken implementation hangs, and the harness bound stops it.

Tests start processes only through this crate, so the reverse inheritance race of ADR 0005 cannot hold another test's pipe open.
Leaked processes are cleaned up by calling Win32 directly on a duplicated handle.

## Consequences

- A slow host only makes the suite slower.
- A broken implementation fails an assertion, or, for a liveness property, hangs until the harness stops it.
- The suite is safe to run in parallel.
