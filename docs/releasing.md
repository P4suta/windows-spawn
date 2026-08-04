# Release procedure

Pushing a branch or tag does not start a release. The manually dispatched
workflow accepts an existing `v`-prefixed SemVer tag, verifies its Cargo
version and commit, then waits for approval from the protected `release`
environment.

## Local candidate

Install stable Rust, `just` 1.57.0, `cargo-cyclonedx` 0.5.9, and REUSE 6.2.0:

```powershell
just reuse
just sbom
just release-candidate
just release-verify v0.1.0
```

`release-candidate` requires a clean worktree and performs `cargo package
--locked` twice. It verifies that both `.crate` files have the same SHA-256,
runs `reuse lint` inside the expanded package, generates a CycloneDX 1.5 JSON
SBOM for normal dependencies on all Cargo targets, generates the REUSE SPDX
SBOM, validates both SBOMs, and writes `target/release-candidate/SHA256SUMS`.

## Repository setup

Create a GitHub environment named `release` and apply the repository's release
branch restrictions. It holds the `RELEASE_PLZ_APP_CLIENT_ID` variable and the
`RELEASE_PLZ_APP_PRIVATE_KEY` secret for the installed `p4suta-release-plz`
App, which needs repository **Contents** and **Pull requests** read/write.

The App token is there because the default `GITHUB_TOKEN` cannot trigger other
workflows, so CI would never run on a release pull request; release-plz
documents this and uses an App itself. Every third-party or GitHub action is
pinned to a complete commit SHA.

## Release flow

`release-plz.yml` runs on every push to `main`. It publishes only when the
manifest version is ahead of the registry, so an unrelated merge cannot
release: **merging a reviewed release pull request is what authorises a
publish.** Let release-plz own the version bump; editing `version` by hand
still reaches the registry but skips the review the release pull request
exists to provide.

release-plz publishes the crate and creates `vX.Y.Z`. `git_release_enable =
false` leaves the GitHub release to `release-finalize.yml`, which the same run
calls: draft releases fire no release event, so there is nothing to hook
instead, and the crate, SBOMs, checksums, and attestations have to be attached
before the release is published.

The finalizer verifies the tag, rebuilds the release candidate, and requires
the rebuilt archive's SHA-256 to equal the crate crates.io actually serves
before attesting anything. It then creates SLSA v1 build provenance and
CycloneDX SBOM attestations, uploads the crate, both SBOMs, and checksums to a
draft GitHub Release, and only then makes it public. A failure can leave a
draft release; inspect and remove that draft before re-running.

## crates.io credentials

A trusted publisher can only be registered against a crate that already exists,
so the first publication of a crate needs a token:

1. Create a short-lived crates.io API token and store it as the
   `CRATES_IO_BOOTSTRAP_TOKEN` secret on the `release` environment.
2. Release normally. `cargo xtask crates-io-auth-mode` reports which credential
   the run selected.
3. Configure the crate's crates.io trusted publisher for this repository,
   `.github/workflows/release-plz.yml`, and the `release` environment.
4. Remove the environment secret and revoke the crates.io token. Later releases
   then use only the short-lived OpenID Connect exchange.

## Verification

Consumers can download the release assets, verify `SHA256SUMS`, and verify
provenance with:

```powershell
gh attestation verify .\windows-spawn-0.1.0.crate --repo P4suta/windows-spawn
```

Add VEX only for a concrete vulnerability status. GitHub Artifact Attestations
provide the repository's signing identity and policy; no separate GPG or
Cosign signatures are produced.
