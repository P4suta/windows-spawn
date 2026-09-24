# 0001 — Why a new crate rather than `firehazard`

Status: accepted (2026-08-01)

## Context

`firehazard` has a safe RAII `ThreadAttributeList` builder for these attributes.
It is published only as `0.0.0` and also covers tokens, ACLs, AppContainers, and debugging.
Job libraries do not integrate process attributes.

## Decision

Build a crate limited to process creation, with stable releases and release gates.

## Consequences

- Part of `firehazard` is duplicated; reconsider if it ships a stable process-creation API with regular releases.
- Token and ACL features are out of scope; see `rappct` or `firehazard`.
- Foreign Job handles can be adopted (ADR 0004).
