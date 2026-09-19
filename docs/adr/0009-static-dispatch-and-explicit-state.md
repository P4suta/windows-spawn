# ADR 0009: Static dispatch and explicit state

## Status

Accepted.

## Context

Spawn correctness depends on ownership, handle-table identity, resource kind, validation state, and transition order remaining visible to the compiler. Application-defined trait objects, runtime reflection, shared interior mutability, global caches, and nondeterministic collections erase information needed to enforce those invariants.

## Decision

All handwritten Rust uses enums, generics, sealed traits, owned state, and validated newtypes in preference to runtime erasure.

Owned trait objects are forbidden. Borrowed trait objects are also forbidden except for the exact `std::error::Error::source` contract. Error categories remain concrete and typed before that standard diagnostic boundary.

Concrete `Box<T>` remains available when heap ownership has a specific representation purpose. `Pin<Box<T>>` is required where Win32 retains addresses through process creation. This does not permit `Box<dyn Trait>`.

Production library code cannot introduce `Rc`, `Arc`, cell types, locks, atomics, one-time global initialization, runtime reflection, or hash-based maps and sets. Test-only fault injection may use thread-local mutation and counters because it does not enter the production artifact. Any future production concurrency must first introduce a typed synchronization boundary and model-checking artifact.

Dependencies that provide erased errors, erased serialization, runtime registration, asynchronous trait erasure, or implicit global state are forbidden at manifest validation time, including renamed dependencies.

## Enforcement

`cargo xtask source-policy` parses every tracked Rust file and rejects trait objects outside the standard error-source boundary, including trait objects nested in owner types or macro tokens. It separately applies the production ownership and determinism rules to the library source. The same command parses every tracked Cargo manifest and rejects forbidden dependencies by package name.

Positive and negative scanner tests cover generics, the standard error-source boundary, borrowed and owned trait objects, runtime reflection, import aliases, macro tokens, target-specific dependencies, and renamed dependencies.

`quality/invariants.toml` registers this decision as `INV-016`, so the repository cannot represent it as review-only guidance.

## Consequences

Extensibility is expressed through closed enums or monomorphized generic implementations. This can increase compile time and code size, but keeps invalid combinations visible to the type checker and prevents hidden allocation, vtable dispatch, downcasting, and shared mutation.
