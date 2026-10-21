---
date: 2026-10-20
status: accepted
---

# Targeted assurance campaigns

The ordinary hosted check selection and aggregation in this decision are partly superseded by [select checks by changed contract](select-checks-by-changed-contract.md).

## Context

The crate's unsafe Win32 boundary compiles on Windows.
The Linux decision code has no unsafe operations, and the crate denies unsafe code outside its explicit native boundary.
Hosted Miri and AddressSanitizer jobs exercise that safe decision code rather than the Win32 boundary.
Two ten-minute fuzz targets run on every pull request, daily jobs repeat larger stress and fuzz campaigns, and weekly mutation uses four four-hour shards.
Repeating those campaigns on unchanged source adds runner work without a new native safety contract.

## Decision

Keep Windows ownership, cleanup, bounded stress, ARM64, careful, `Application Verifier`, proof layers, coverage, dependency policy, and release checks in ordinary verification.
Remove hosted fuzz and mutation campaigns and the Miri and AddressSanitizer jobs over safe Linux code.
Local commands and targets support a specific parser, lifetime, or test-quality investigation.
Promote a discovered counterexample into an ordinary regression test.
Mutation outcomes keep their existing rules, including the distinction between a detected defect and a hang.

This decision replaces the hosted campaign cadence in [ADR 0011](0011-mutation-testing-runs-on-rust-mutants.md) and the hosted fuzz, Miri, sanitizer, and extended nightly runs in [ADR 0017](0017-long-careful-and-fuzzed-runs.md).
Their native safety contracts and local investigation mechanisms continue to apply.

## Consequences

Required aggregation waits for every remaining native and proof check.
No replacement scheduled or manual campaign workflow consumes runners.
Reassess instrumentation when the Linux decision code gains unsafe code or a native memory boundary.
An investigation that needs longer stress or mutation selects its source, invariant, and resource budget explicitly.
