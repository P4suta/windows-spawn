# Contributing

Contributions must preserve the crate's ownership and cleanup contracts.

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
then run. The first command builds the pinned OComment revision from source:

```powershell
just ocomment
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

## Machine-enforced change contract

`cargo xtask ci` begins with the OComment-backed comment policy and the typed
invariant registry. It then checks formatting, lints, tests, documentation,
MSRV, targets, supply chain, spelling, public API, and packaging. A pull request
links the resulting evidence artifact; it does not carry a manual correctness
checklist. Pull requests are squash-merged into `main` only after required
checks succeed.
