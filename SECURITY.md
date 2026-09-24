# Security policy

## Supported versions

Fixes target the latest published minor release.
Older minor releases are not guaranteed fixes.

## Reporting a vulnerability

Use [private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new).
Do not open a public issue.

Include:

- the affected version or commit;
- the Windows version and architecture;
- a minimal reproduction;
- the expected impact;
- whether it involves handle inheritance, Jobs, mitigations, ConPTY, or command-line construction.

Omit secrets and unneeded personal data.

The maintainer acknowledges the report, investigates, and coordinates disclosure and a release when warranted.
There is no fixed response time.

## Security boundary

The crate is a process-creation primitive, not a sandbox.
Tokens, ACLs, AppContainer, LPAC, capability SIDs, and supervision are the caller's responsibility.
The crate docs describe the handle-inheritance race and the ownership contracts.
