# Release procedure

Only the maintainer's signed tag starts a release, and nothing is published until the maintainer approves its deployment.

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

- The `release` environment deploys from `main` only.
  It holds the `RELEASE_PLZ_APP_CLIENT_ID` variable and the `RELEASE_PLZ_APP_PRIVATE_KEY` secret of the `p4suta-release-plz` App, which needs **Contents** and **Pull requests** read/write.
  The App token is needed because a pull request opened with `GITHUB_TOKEN` starts no CI.
- The `crates-io` environment deploys from `v*` tags only, requires the maintainer's review, and lets no administrator bypass it.
- The crate's trusted publisher on crates.io is this repository, `release.yml`, and the `crates-io` environment, and the crate accepts trusted publishing only.
- Two rulesets guard `v*` tags, because a bypass actor skips every rule of its ruleset.
  One restricts creation, and only the maintainer bypasses it.
  The other forbids moving or deleting a tag and requires signatures, and nobody bypasses it.
- `release.yml` also refuses a tag that is not annotated and verified as signed, but it runs as the tagged commit has it, so the creation rule and the approval are what guard a release.

Every action is pinned to a full commit SHA.

## Release flow

1. `release-plz.yml` runs on every push to `main`, and keeps a draft pull request that bumps the version and updates `CHANGELOG.md`.
   It never tags and never publishes.
2. Mark the pull request ready, review it, and merge it.
3. Tag the merge commit with a signed tag, and push it:

   ```powershell
   git switch main
   git pull --ff-only
   git tag -s v0.2.0 -m v0.2.0
   git push origin v0.2.0
   ```

4. `release.yml` verifies that the tag is annotated and signed, and names a commit on `main` whose Cargo version it matches.
   It builds the release candidate, attests SLSA v1 provenance and the CycloneDX SBOM, and waits in the `crates-io` environment.
5. Check that the run is for the tag you pushed, on the commit `main` holds, and that `candidate` passed; then approve the deployment.
   The job takes a short-lived crates.io token through OpenID Connect, and publishes only if the archive `cargo package` makes is the attested candidate.
   It then requires crates.io to serve that same archive.
6. The last job uploads the crate, SBOMs, and checksums to a draft GitHub release, and publishes it.

Rejecting the deployment leaves the tag and the attestations, but publishes nothing.
A failure after publishing can leave a draft release; remove it before re-running the failed jobs.

## Verification

Download the release assets, check `SHA256SUMS`, and verify provenance:

```powershell
gh attestation verify .\windows-spawn-0.1.0.crate --repo P4suta/windows-spawn
```

VEX is added only for a concrete vulnerability status.
GitHub Artifact Attestations provide the signing identity; no GPG or Cosign signatures are produced.
