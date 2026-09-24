# 0012 — Win32 failures are injected in tests

Status: accepted (2026-09-25)

## Context

The first mutation run left about a hundred `?` propagations and error branches unverified: no test could make a Win32 call fail at a chosen point.

## Decision

`sys::fault`, compiled only for tests, fails the Nth fallible wrapper call on the current thread.
Each fallible wrapper checks it on entry with a `#[cfg(test)]` statement, so non-test builds contain nothing of it.

`failure_tests` records the wrapper calls of each public operation, then fails each call in turn and requires exactly the injected error back; a panic, success, or other error fails the test.
An isolated run repeats every failure twice and requires an unchanged process handle count.

Logic after a Win32 call is tested with real failures instead: handles without the needed access, and invalid inputs.

## Consequences

- Every error propagation of the public operations runs in tests.
- A new fallible wrapper needs a `Call` and an operation that reaches it.
- Reader threads are outside the plan, which is thread-local; their errors are tested directly.
