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

Create a GitHub environment named `release` with at least one required reviewer
before adding a publishing credential. Apply the repository's release branch
restrictions.

The workflow needs the repository's default `GITHUB_TOKEN` permissions only;
its job requests `contents: write`, `id-token: write`, `attestations: write`,
and `artifact-metadata: write`. Every third-party or GitHub action is pinned to
a complete commit SHA.

## First crates.io publication

crates.io trusted publishing cannot be configured before the package exists.
For the first publication only:

1. Create a short-lived crates.io API token and store it as the
   `CRATES_IO_BOOTSTRAP_TOKEN` secret on the protected `release` environment.
2. Manually run the **Release** workflow for the existing tag with
   `publish_crates_io` enabled, then approve the environment deployment.
3. Immediately remove the environment secret and revoke the crates.io token.
4. Configure the crate's crates.io trusted publisher for this repository,
   `.github/workflows/release.yml`, and the `release` environment.

For later publications, leave `CRATES_IO_BOOTSTRAP_TOKEN` absent. The workflow
uses the crates.io authentication action to obtain and revoke a short-lived
OIDC token.

## Publishing and verification

After approval, the workflow regenerates the candidate, creates SLSA v1 build
provenance and CycloneDX SBOM attestations, uploads the crate, both SBOMs, and
checksums to a draft GitHub Release, optionally publishes to crates.io, and
only then makes the GitHub Release public. A failure can leave a draft release;
inspect and remove that draft before retrying the same tag.

Consumers can download the release assets, verify `SHA256SUMS`, and verify
provenance with:

```powershell
gh attestation verify .\windows-spawn-0.1.0.crate --repo P4suta/windows-spawn
```

Add VEX only for a concrete vulnerability status. GitHub Artifact Attestations
provide the repository's signing identity and policy; no separate GPG or
Cosign signatures are produced.
