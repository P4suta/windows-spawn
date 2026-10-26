---
date: 2026-10-26
status: proposed
---

# Release-only heavy checks

## Status

Proposed.
This partly supersedes the hosted cadence and path selection in [select checks by changed contract](select-checks-by-changed-contract.md).
The native contracts and proof obligations from the earlier decisions continue to apply.

## Context

Path selection avoids some unrelated verification, but workflow maintenance still selects native matrices and every proof layer.
Repeating those checks on each pull request makes a runner-label or workflow change trigger release-scale verification.
The native Windows boundary still needs an ordinary check because Linux exposes no Windows API.
The release must bind complete native and formal evidence to the same source that produces its candidate.

## Decision

Run the existing Linux repository checks and a Windows x64 smoke check on every pull request and merge group.
The Windows check uses strict library Clippy and `args_environment_cwd_and_wait_cache_work`.
The smoke test checks process creation, exit code 37, and repeated wait results after configuring arguments, environment, and working directory.
Its assertions establish that bounded process lifecycle rather than the full native contract.

Run the full native suite on Rust 1.99.1 and 1.84.1, the i686 regression, and ARM64 tests at release qualification.
The same qualification requires the native `appverif.exe` checks, the public API snapshot, documentation, package checks, and all seven proof layers.
The current-Rust native job runs documentation, package, and license metadata checks together.
The native test command already runs Windows doctests.
Keep each proof layer's complete inventory, implementation correspondence checks, and negative controls.
The [proof guide](../../proofs/README.md) preserves the distinction between bounded code checks, models, refinement checks, and native observations.

Resolve the annotated release tag only after checking its signature, GitHub's verification result, commit on main, and Cargo version.
Require every qualification checkout's `HEAD` to match the resolver's full 40-character commit SHA.
The release workflow runs Linux quality and all six heavy roles on the resolved source.
The full native job covers the ordinary Windows smoke test and its compilation checks, so release qualification omits the separate smoke job.
The qualification summary requires successful results from exactly those seven roles and the complete seven-layer proof inventory.
Require successful qualification before building the release candidate.
Preserve the existing candidate, publication, and draft GitHub Release boundaries.
Missing, failed, cancelled, or skipped evidence must stop the release.
The ordinary workflow contains only Linux quality, Windows smoke verification, and `required`.
Its summary requires successful results from exactly the two verification jobs.
Move the heavy job definitions into the release workflow without duplicating them or adding a reusable or manual campaign entry point.

The workflow tests parse the production YAML and execute its jq contracts.
Keep `yaml-rust2` at 0.11.1 because versions 0.12 and 0.13 require Rust 1.85 while the workspace supports Rust 1.84.
The dependency update rule excludes those incompatible versions and protects the release workflow's MSRV declaration.
The parser's transitive `foldhash` 0.2.0 dependency uses the Zlib license.
Accept that exact dependency in `deny.toml` while keeping the general `Apache-2.0` and `MIT` policy and checking development dependencies.

Keep `just check` as the ordinary host command, including the Windows smoke check on Windows.
Keep `just windows-smoke` for the native job and focused local checks, and keep `just ci` as a broader explicit local suite.
Coverage, broad stress, allocation poisoning, fuzzing, and mutation remain tools for a specific investigation.
Add no scheduled campaign.
Changes to a verified decision still update its proof, model, or vectors in the same change.

Keeping path-selected heavy jobs on pull requests would preserve the repeated release-scale cost for workflow maintenance.
Deleting those jobs would lose evidence for supported native contracts.
Leaving them to unrelated manual runs would fail to bind their results to the released commit.

## Consequences

Every change retains Linux verification and a native Windows signal.
The release waits for the full platform and proof inventory on one source commit.
Defects outside the ordinary smoke check may emerge during focused local verification or release qualification.
Contributors must choose focused tests when changing the affected native or modeled contract.

The smoke test doesn't prove argument, environment, or working-directory round trips.
The proof layers keep the limits stated in their existing models and implementation links.
Source-contract tests can check job dependencies, source binding, and aggregation, but native and hosted adapters still need execution evidence.
An unrun qualification or publication stage supplies no passing result.
