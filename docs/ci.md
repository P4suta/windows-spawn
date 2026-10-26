# CI selection

`CI` runs on pull requests and merge groups.
Every change runs the Linux repository checks and a Windows x64 smoke check.
The Linux job runs formatting, Clippy, repository laws, supported unit and integration tests, the empty non-Windows API check, dependency policy, license metadata checks, Typos, and Actionlint.
`mise.toml` pins its tools.
The pinned mise action verifies each platform's binary with the release's signed checksums.

The Windows job runs strict library Clippy and the existing [`args_environment_cwd_and_wait_cache_work` test](../tests/windows_spawn.rs).
The test configures arguments, an environment value, and a working directory before spawning a child.
Its assertions check exit code 37 and the same result from repeated waits.
The full native suite supplies broader Windows verification at release qualification.

The `required` job requires both ordinary jobs to succeed.
Its dependency set contains exactly those two jobs.
Missing results, cancellation, failure, and skips refuse the gate.

## Local checks

Run the ordinary checks with the pinned Python environment:

```powershell
mise x -- uv run --no-project --with 'reuse[charset-normalizer]==6.2.0' just check
```

`just check` runs the repository checks available on the host and includes the native smoke check on Windows.
`just windows-smoke` runs the Windows job's checks for a focused native investigation.
`just ci` remains a broader local suite for deliberate verification.
Use `mise x -- just COMMAND` for other project recipes.
Install Rust 1.84 with rustup for `just msrv` and the three Windows Microsoft Visual C++ targets for `just cross-targets`.
`just public-api` needs the toolchain in `public-api/rust-toolchain` and cargo-public-api 0.52.0.
`just reuse` needs the pinned Python environment: `mise x -- uv run --no-project --with 'reuse[charset-normalizer]==6.2.0' cargo xtask reuse`.
Native execution requires Windows.
`just verifier` also needs the Windows SDK tool `appverif.exe` and administrative rights.

## Release qualification

The [release workflow](../.github/workflows/release.yml) validates the annotated tag, GitHub's signature-verification result, the commit's ancestry on main, and the Cargo version.
Its resolver emits the full 40-character commit SHA.
Every qualification checkout verifies that `HEAD` matches that SHA.
The release runs Linux quality and six heavy qualification roles.
The full native job includes the ordinary Windows smoke test and its compilation checks.
The `qualify` job requires successful results from exactly those seven roles and the complete seven-layer proof inventory.
Missing results, invalid proof-layer data, unknown or duplicate layers, cancellation, failure, and skips refuse qualification.
The candidate waits for both the resolver and qualification, then checks out the resolver's commit.
The existing publication and draft-release stages follow the candidate.

Full qualification runs native tests on Rust 1.99.1 and 1.84.1, the i686 regression, ARM64 tests, the native verifier, and the public API snapshot.
The current-Rust native job also checks documentation, package contents and build, and license metadata through its pinned environment.
The native test command includes its Windows doctests.
All seven proof layers run: Kani, Lean, Verus, `TLA+`, Quint, Alloy, and Prolog.
Each layer keeps its complete expected inventory, source correspondence checks, and negative controls.
The [proof guide](../proofs/README.md) states each model's scope and connection to the implementation.

Each candidate must supply evidence from every required gate.
Source-contract checks can't establish Windows behavior, model refinement, or GitHub's hosted permissions and event handling.
The [release-only heavy-check decision](adr/release-only-heavy-checks.md) records the cadence and its tradeoffs.

Coverage, allocation poisoning, broad cleanup stress, fuzzing, and mutation support a specific local investigation.
They have no recurring hosted campaign.
Rust 1.84 compatibility remains explicit.
sha2 stays below 0.11 while that workspace contract applies.
The public API nightly has one declaration shared by installation and snapshot generation, which Renovate tracks as a Rust toolchain file.
