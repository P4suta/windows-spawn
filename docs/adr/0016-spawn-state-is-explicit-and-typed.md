# 0016 — Spawn state is explicit and typed

Status: accepted (2026-10-13); moves the rollback of ADR 0007 into the process handle, and extends the trait-object rule of ADR 0013

## Context

A suspended child and an uncommitted transaction kept their process in an `Option`, so that a consuming method could move it out past the type's `Drop`.
Each read of it needed an `expect`, and the lint registry excused both.
One function acquired every creation resource, and the registry excused its length too.

Handles travelled as `BorrowedHandle`, and handle values as `isize`.
Nothing stopped a thread handle from reaching a call for processes, or a value of the creator's table from joining the values an alternate parent's child inherits.
The attribute list held pointers to values that the transaction kept alive by convention.

PR #14 recorded in its ADR 0009 that trait objects, runtime reflection, shared interior mutability, global state, and hash-ordered collections hide what the compiler needs to check a spawn.
Main dropped #14, and with it those rules.

## Decision

- The process handle of a spawn carries its rollback.
  Until the main thread runs, dropping the handle terminates the process.
  A transaction, a `SuspendedChild`, and a `Child` own their parts directly, and `resume` and the commits move them out without an `Option`.
  A running spawn commits by resuming a `SuspendedChild`, so both paths share one resume.
- The creation splits into functions for the standard I/O, the private Job, the Job list, the working directory, and the attribute list.
- Each handle names its object type.
  `Owned<K>` and `Borrowed<'a, K>` name a process, a thread, a Job, a pipe end, a standard stream, or a handle the caller supplied, and each wrapper in `sys` takes the type its Win32 call needs.
- Handle values carry the table they belong to.
  `Value<Creator>` names the alternate parent and the Jobs, and `Value<Inheritor>` names the inherited handles and the standard handles.
- `AttributeList<'a>` borrows the values its attributes point to, so the compiler keeps them alive and in place until `DeleteProcThreadAttributeList` runs.
- `cargo xtask gates` refuses a trait object in every file, except as the return type of `source` in an implementation of `Error`.
  In library code it also refuses `Rc`, `Arc`, `Weak`, cell types, locks, atomics, `static` items, one-time initialization, `thread_local`, `Any`, `TypeId`, type names, and hash-based maps and sets.
  Items under `cfg(test)` or `cfg(kani)`, and `src/failure_tests.rs`, keep their thread-local state, counters, and downcasts.
- `deny.toml` bans the crates #14 banned, which erase errors, erase serialization or futures, register items at run time, or keep implicit global state.

## Consequences

- The registry holds no `expect_used` or `too_many_lines` exception for `src/child.rs` or `src/transaction.rs`.
- A handle of the wrong kind, a value of the wrong table, or an attribute value that dies before its list fails to compile.
- Concurrency in the library needs a typed boundary and a model before it can pass the gate.
- The output readers of `Child::wait_with_output` stay threads that own what they read, so they share no state.
