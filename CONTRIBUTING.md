# Contributing

Changes must preserve the crate's ownership and cleanup contracts.

## Before a change

Open an issue with the bug, feature, or question form when the expected behavior needs discussion.
Report vulnerabilities through [private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new), not an issue.

Keep pull requests focused.
A change to ownership, handle inheritance, Job lifetime, ConPTY, mitigation, quoting, or suspended processes states the invariant it preserves.

## Development

Tests require Windows 10 version 1809 or later.
The crate supports Rust 1.84 and later.
With the tool versions CI uses installed, run:

```powershell
just ci
just coverage
```

Also run:

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
Hosted checks keep bounded native stress tests, careful, `Application Verifier`, proof layers, and the ordinary platform tests.
See [targeted assurance campaigns](docs/adr/targeted-assurance-campaigns.md) for the hosted verification policy.

## Expectations

- Keep the documented ownership and cleanup behavior, including on errors.
- Give every `unsafe` block a specific safety justification.
- Write only doc comments and `// SAFETY:` comments; the reason for a change goes in its commit message (ADR 0013).
- Write a lint exception as `#[expect(lint, reason = "…")]` on the one item it concerns; `just gates` refuses every `allow` and checks the other repository laws (ADR 0018).
- Add deterministic tests for behavior changes; tests wait for events, never for time (ADR 0010).
- Change the public API snapshot only with an API change, and state its compatibility impact.
- Update the crate docs, ADRs, or security boundary when a contract changes.
- Keep dependencies few and within the MSRV.

Required checks and review threads must be resolved before merge.
Pull requests are squash-merged into `main`.

## Check selection

See [CI selection](docs/ci.md) for the always-running gate, affected native and proof contracts, and local setup.
