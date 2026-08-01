# 0003 — Attribute values are borrowed, and the borrow is in the type

Status: accepted (2026-08-01)

## Context

`UpdateProcThreadAttribute` stores the *pointer* it is given, not the value
behind it. Every attribute value — the handle array, the parent process handle,
the policy word, the job handle, the `HPCON` — must still be alive, and at the
same address, when `CreateProcessW` later walks the list. The C API gives no
help: passing the address of a stack temporary compiles, and reads freed memory
at spawn time.

Options considered:

1. **Copy every value into the list's own allocation.** Safe, but wrong for
   `HANDLE_LIST`, whose size is dynamic, and it hides that the values are
   genuinely shared with the kernel call.
2. **Store `Arc`/owned copies.** Forces allocation and ownership transfer for
   handles the caller usually wants to keep using.
3. **Borrow, and encode the borrow as a lifetime parameter.**

## Decision

`AttributeList<'a>` and `WindowsCommand<'a>` carry a lifetime that is the
intersection of every borrowed attribute value. The builder methods take
`&'a [RawHandleRef<'a>]`, `&'a ParentProcess`, `&'a Job`, `&'a Pcon`, so the
borrow checker rejects a list that outlives anything it points at. The buffer
itself is a `Box<[u8]>`, never a `Vec`, so it cannot be reallocated out from
under the pointers the kernel was given.

## Consequences

- The classic use-after-free of this API becomes `error[E0597]` at compile time.
- Callers must hoist their handle array into a binding before the builder chain,
  which is slightly less fluent than the C style — an acceptable price, and the
  README example shows the shape.
- `AttributeList` cannot be stored in a long-lived struct alongside its values
  without self-referential gymnastics. That is intentional: an attribute list is
  meant to live for one spawn.
- `Drop` calls `DeleteProcThreadAttributeList` before the box is freed, so the
  two-phase allocation is invisible to callers.
