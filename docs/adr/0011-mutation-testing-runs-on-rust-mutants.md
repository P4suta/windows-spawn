# 0011 — Mutation testing runs on rust-mutants

Status: accepted (2026-09-25); supersedes ADR 0008

## Context

cargo-mutants reports a hanging mutant as a timeout, which a gate can count as caught.
Under ADR 0010 a hang proves nothing, and tests are written to fail fast instead.
The njutest engine, rust-mutants, classifies a hang as `waited`, never as a detection, and gates on audited expectations rather than a score.
`njutest verify` cannot yet complete on Windows, and this crate compiles only there.

## Decision

Run `rust-mutants run` on Windows through `cargo xtask mutation`, under a private kill-on-close Job.
`.rust-mutants.toml` fixes the scope: the `windows-spawn` package, the `strong` tier for the bitwise rules, serial test threads, and no doctests.
The xtask rejects arguments that change the scope.

A survivor is killed by a new test or recorded as `[[mutation.expect]]` with a reason naming the equivalence or the test that holds the invariant.
A property observable only as a hang is recorded as `[[mutation.skip]]` with a reason.
Skips and expectations live in `.rust-mutants.toml`, not in source comments.

The weekly workflow measures four shards and gates on the merged exit code.
The engine is pinned to an njutest commit until njutest publishes a release.
Mutation builds cap lints to warnings, because the instrumented tree trips `unused_qualifications`.

## Consequences

- A run is clean only when every mutant is killed, expected, or skipped with a reason.
- A hang never counts as a detection.
- Move to `njutest verify` once it supports Windows.
