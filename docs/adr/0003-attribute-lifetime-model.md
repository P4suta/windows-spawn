# 0003 — Attribute storage belongs to the spawn transaction

Status: accepted (2026-08-02)

## Context

`UpdateProcThreadAttribute` retains pointers until `CreateProcessW` consumes
the list. The list backing store must be suitably aligned, and every pointed-to
value must stay at a stable address. A stack temporary or a reallocating vector
can therefore turn otherwise plausible Rust into invalid FFI.

The pseudoconsole attribute is exceptional: Microsoft requires the `HPCON`
value itself as `lpValue`, whereas the other supported attributes receive the
address of stable storage.

## Decision

The attribute list is private and cannot outlive one `SpawnTransaction`. Its
backing allocation is word-aligned. Every normal attribute value has its own
stable owned allocation, and the transaction retains those allocations until
after `DeleteProcThreadAttributeList`. The pseudoconsole path passes its
borrowed value according to the documented exception.

`SpawnOptions<'a>` carries the lifetime of borrowed Jobs, parent process, and
pseudoconsole capability. `Command` does not borrow them and remains reusable.

## Consequences

- There is no public raw attribute API and no self-referential public builder.
- Attribute pointers cannot outlive their values.
- The list is deleted before either its values or aligned backing storage are
  released.
- The unsafe lifetime proof remains local to the private `sys` module.
