# Release procedure

Releases are deliberately manual. Pushing a branch or tag never starts the
release workflow. The workflow accepts an existing `v`-prefixed SemVer tag,
validates that its version matches `Cargo.toml` and that it resolves to the
checked-out commit, then waits for approval from GitHub's protected `release`
environment.

## Local candidate

Install stable Rust, `just` 1.57.0, `cargo-cyclonedx` 0.5.9, and REUSE 6.2.0.
The developer-facing commands are:

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

Before adding any publishing credential, create a GitHub environment named
`release` and configure at least one required reviewer. Keep deployment branch
rules as restrictive as the repository's release policy permits.

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

For every later publication, leave `CRATES_IO_BOOTSTRAP_TOKEN` absent. The
workflow then obtains a short-lived OIDC token with the official crates.io
authentication action and revokes it automatically when the job completes.

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

VEX is added only when there is a concrete vulnerability status to communicate.
Separate GPG and Cosign signatures are intentionally omitted while they would
not add an independently managed identity or policy beyond GitHub Artifact
Attestations.
