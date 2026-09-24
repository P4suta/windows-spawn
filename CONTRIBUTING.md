# Contributing

Changes must preserve the crate's ownership and cleanup contracts.

## Before a change

Open an issue with the bug, feature, or question form when the expected behavior needs discussion.
Report vulnerabilities through [private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new), not an issue.

Keep pull requests focused.
A change to ownership, handle inheritance, Job lifetime, ConPTY, mitigation, quoting, or suspended processes states the invariant it preserves.

## Development

Tests require Windows 10 version 1809 or later.
The crate supports Rust 1.75 and later.
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

## Expectations

- Keep the documented ownership and cleanup behavior, including on errors.
- Give every `unsafe` block a specific safety justification.
- Add deterministic tests for behavior changes; tests wait for events, never for time (ADR 0010).
- Change the public API snapshot only with an API change, and state its compatibility impact.
- Update the crate docs, ADRs, or security boundary when a contract changes.
- Keep dependencies few and within the MSRV.

Required checks and review threads must be resolved before merge.
Pull requests are squash-merged into `main`.
