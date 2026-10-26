# Contributing

Changes must preserve the crate's ownership and cleanup contracts.

## Before a change

Open an issue with the bug, feature, or question form when the expected behavior needs discussion.
Report vulnerabilities through [private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new), not an issue.

Keep pull requests focused.
A change to ownership, handle inheritance, Job lifetime, ConPTY, mitigation, quoting, or suspended processes states the invariant it preserves.

## Development

Native tests require Windows 10 version 1809 or later.
The crate supports Rust 1.84 and later.
With the pinned tools installed, run the ordinary host checks:

```powershell
mise x -- uv run --no-project --with 'reuse[charset-normalizer]==6.2.0' just check
```

The command includes strict library Clippy and the native smoke test on Windows.
`just windows-smoke` exposes those Windows checks for a focused investigation.
`just ci` remains a broader local suite for deliberate verification.
Coverage supports a specific investigation rather than every change.
Also check the diff and secrets:

```powershell
actionlint
git diff --check
gitleaks git . --redact --no-banner
gitleaks dir . --redact --no-banner
```

`just release-candidate` checks packaging and reproducibility without publishing.

A change to a verified decision updates its proof, model, or vectors in the same change, as [proofs/README.md](proofs/README.md) and ADR 0014 describe.
With Kani, `elan`, Java, Node.js, and `swipl` installed, run the layers on any platform:

```sh
just proofs
just proofs kani lean
```

`just proof-traces` regenerates the Quint traces after the lifecycle model changes.

On Windows, `just verifier` runs every test under `appverif.exe`, which stops a process on handle misuse, heap corruption, lock misuse, or a leak.
It needs an administrator and the `appverif.exe` of the Windows SDK, which the hosted Windows runners of CI also have.
Three tests misuse a handle on purpose, and each must stop, which shows the verifier watches the run.

Deeper checks need no change to the code, and ADR 0017 says what each covers:

```powershell
$env:WINDOWS_SPAWN_STRESS_CASES = "1300"
cargo test --test windows_spawn spawn_cleanup_stress -- --ignored --exact
cargo +nightly careful test --workspace --all-targets -- --test-threads=1
```

```sh
cargo +nightly miri test --package windows-spawn --lib
cargo +nightly fuzz run command-line -- -max_total_time=600 -max_len=16384
cargo +nightly fuzz run environment-block -- -max_total_time=600 -max_len=16384
```

`WINDOWS_SPAWN_RANDOM_SCALE` multiplies the number of random command lines and environments for a local investigation.
Ordinary hosted checks run the Linux repository gate and Windows x64 smoke check on every pull request and merge group.
The signed-tag release workflow qualifies the exact commit with the full native, MSRV, architecture, API, package, and proof gates before building its candidate.
See [release-only heavy checks](docs/adr/release-only-heavy-checks.md) for the cadence and [CI selection](docs/ci.md) for commands and evidence boundaries.

## Expectations

Keep the documented ownership and cleanup behavior, including on errors.
Give every `unsafe` block a specific safety justification.
Write only doc comments and `// SAFETY:` comments.
The reason for a change goes in its commit message, as [ADR 0013](docs/adr/0013-repository-laws-are-enforced-mechanically.md) requires.

Write a lint exception as `#[expect(lint, reason = "…")]` on the one item it concerns.
`just gates` refuses every `allow` and checks the other repository laws in [ADR 0018](docs/adr/0018-lint-exceptions-expire-and-the-msrv-is-rust-1-84.md).
Add deterministic tests for behavior changes.
Tests wait for events, never for time, as [ADR 0010](docs/adr/0010-time-decides-nothing.md) requires.

Change the public API snapshot only with an API change, and state its compatibility impact.
Update the crate docs, ADRs, or security boundary when a contract changes.
Keep dependencies few and within the MSRV.

Pass required checks and resolve review threads before merge.
Pull requests are squash-merged into `main`.

## Check selection

See [CI selection](docs/ci.md) for the ordinary gate, release qualification, and local setup.
