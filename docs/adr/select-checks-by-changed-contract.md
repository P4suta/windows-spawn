---
date: 2026-10-21
status: accepted
---

# Select checks by changed contract

## Status

Accepted.
This partly supersedes [targeted assurance campaigns](targeted-assurance-campaigns.md).
The proposed [release-only heavy checks](release-only-heavy-checks.md) decision partly supersedes the hosted cadence and path selection below.
The native contracts, proof inventories, negative controls, and strict aggregation remain required.

## Context

Before path selection, every pull request repeated two Windows generations, two Rust versions, all seven proof layers, stress, allocation poisoning, native `appverif.exe` checks, coverage, and reproducible release candidates.
Documentation and tooling changes don't alter every native or modeled contract.
Push verification repeats the pull request checks, while scheduled CodeQL scans repeat unchanged source.

## Decision

Run the deterministic Linux gate on every pull request and merge group, and select extra checks using `.github/filters/ci.yml`.
Keep native Rust 1.84.1 and current-toolchain execution, 32-bit execution, ARM64 execution, API snapshots, native `appverif.exe` checks, packaging, and the individual proof layers for their affected inputs.
Require the exact expected job results and reject missing, invalid, failed, or cancelled work, and skips that selection doesn't permit.
Keep complete proof inventories and their negative controls inside each selected layer.

Keep broad stress, coverage, and allocation poisoning as local investigation tools.
Reproducible release evidence belongs to the protected release workflow.
Remove duplicate main verification and scheduled CodeQL, and add no recurring or manual hosted campaign.
Keep the existing Rust 1.84 workspace contract and use one Renovate-managed file for the API snapshot nightly.

Keeping every check unconditional would spend runners on unrelated contracts.
Deleting the proof and native checks would lose evidence for the contracts that need them.
Requiring private machines would make a contributor's merge verdict depend on unavailable infrastructure.

## Consequences

Contributors keep the same project commands and supported native contracts.
Changes to contract boundaries must update their filters and verify both selection and the strict summary.
The OS, GitHub event, and external tool adapters still require native or hosted verification.
Selection checks don't prove Win32 behavior.
Reassess removed instrumentation when a concrete defect or new boundary needs it.
