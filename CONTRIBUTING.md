# Contributing

Thank you for helping improve `windows-spawn`. Contributions that make Windows
process creation safer, more predictable, or better documented are welcome.

## Before opening a change

Use the bug, feature, or question issue form when discussion would help define
the expected behavior. Report suspected vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new),
not a public issue.

Keep pull requests focused. Changes to ownership, handle inheritance, Job
lifetime, ConPTY, mitigation, quoting, or suspended-process behavior should
explain the safety invariant they preserve.

## Development environment

Development and integration tests require Windows 10 version 1809 or later.
The crate supports Rust 1.75 and later. Install the tool versions used by CI,
then run:

```powershell
just ci
just coverage
```

Before submitting, also run:

```powershell
actionlint
git diff --check
gitleaks git . --redact --no-banner
gitleaks dir . --redact --no-banner
```

`just release-candidate` validates packaging and reproducibility locally; it
does not publish anything.

## Code and documentation expectations

- Preserve the documented ownership and cleanup behavior, including on errors.
- Give every `unsafe` block a specific safety justification.
- Add deterministic tests for behavior changes and avoid timing-only assertions.
- Keep the public API snapshot unchanged unless the pull request intentionally
  changes the public API and explains the compatibility impact.
- Update the crate documentation, ADRs, or security boundary when contracts
  change.
- Keep dependencies minimal and compatible with the MSRV.

All required GitHub checks must pass and review conversations must be resolved
before merge. The repository uses squash merges so each pull request becomes
one focused commit on `main`.
