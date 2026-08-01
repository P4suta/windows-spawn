# 0001 — Why a new crate rather than `firehazard`

Status: accepted (2026-08-01)

## Context

`firehazard` already solves this problem, and solves it well: a safe RAII
`ThreadAttributeList` builder covering 20+ `PROC_THREAD_ATTRIBUTE_*` values,
including every attribute `spawnkit` targets. Writing a new crate in the
presence of working prior art needs a reason better than "I want to".

Three facts shape the decision. It has been published as version `0.0.0` with
no crates.io release since September 2022 (5,441 downloads total). Its scope is
a sandboxing research toolkit — tokens, ACLs, AppContainers, debugging — of
which process creation is one corner. And the surrounding ecosystem has not
picked it up: `process-wrap` (9.9M downloads) and `win32job` (1.1M) handle job
objects without ever touching an attribute list, and applications keep writing
their own (854 GitHub hits for `InitializeProcThreadAttributeList`).

## Decision

Write a new crate scoped to process creation only, and treat release discipline
as a feature rather than an afterthought.

## Consequences

- Duplicated effort against `firehazard`, knowingly. If `firehazard` ships a
  stable 0.1 with a release cadence, `spawnkit` has lost its reason to exist and
  the README should say so.
- The narrow scope is a constraint, not just a description: token and ACL
  features get rejected, and users are pointed at `rappct`/`firehazard`.
- `spawnkit` must be adoptable *next to* the incumbents, not instead of them —
  hence adopting foreign job handles rather than insisting on its own (ADR 0004).
