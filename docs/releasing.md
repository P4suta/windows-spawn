# Release procedure

Pushing a branch or tag does not release.

## Local candidate

Install stable Rust, `just` 1.57.0, `cargo-cyclonedx` 0.5.9, and REUSE 6.2.0, then run:

```powershell
just reuse
just sbom
just release-candidate
just release-verify v0.1.0
```

`release-candidate` requires a clean worktree and runs `cargo package --locked` twice.
It checks that both `.crate` files have the same SHA-256 and runs `reuse lint` in the expanded package.
It generates and validates a CycloneDX 1.5 JSON SBOM of normal dependencies for all Cargo targets and the REUSE SPDX SBOM.
It writes `target/release-candidate/SHA256SUMS`.

## Repository setup

Create a GitHub environment named `release` with the repository's release branch restrictions.
It holds the `RELEASE_PLZ_APP_CLIENT_ID` variable and the `RELEASE_PLZ_APP_PRIVATE_KEY` secret of the `p4suta-release-plz` App, which needs **Contents** and **Pull requests** read/write.

The App token is needed because `GITHUB_TOKEN` cannot trigger workflows, so CI would not run on a release pull request.
Every action is pinned to a full commit SHA.

## Release flow

`release-plz.yml` runs on every push to `main`.
It publishes only when the manifest version is ahead of the registry, so **merging a reviewed release pull request is what authorizes a publish.** Let release-plz bump the version; a manual bump still publishes but skips that review.

release-plz publishes the crate and creates `vX.Y.Z`.
`git_release_enable = false` leaves the GitHub release to `release-finalize.yml`, called by the same run, because the crate, SBOMs, checksums, and attestations must be attached before publication and draft releases fire no event.

The finalizer verifies the tag, rebuilds the release candidate, and requires the rebuilt crate's SHA-256 to equal the one crates.io serves.
It then creates SLSA v1 provenance and CycloneDX SBOM attestations, uploads the crate, SBOMs, and checksums to a draft release, and publishes it.
A failure can leave a draft; remove it before re-running.

## crates.io credentials

A trusted publisher can be registered only for an existing crate, so the first publication needs a token:

1. Store a short-lived crates.io token as the `CRATES_IO_BOOTSTRAP_TOKEN` secret of the `release` environment.
2. Release normally; `cargo xtask crates-io-auth-mode` reports the credential used.
3. Register this repository, `.github/workflows/release-plz.yml`, and the `release` environment as the crate's trusted publisher.
4. Delete the secret and revoke the token; later releases use OpenID Connect only.

## Verification

Download the release assets, check `SHA256SUMS`, and verify provenance:

```powershell
gh attestation verify .\windows-spawn-0.1.0.crate --repo P4suta/windows-spawn
```

VEX is added only for a concrete vulnerability status.
GitHub Artifact Attestations provide the signing identity; no GPG or Cosign signatures are produced.
