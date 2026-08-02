# Security policy

## Supported versions

Security fixes target the latest published minor release line. Before the first
release, reports may target `main`. Older minor lines are not guaranteed fixes.

## Reporting a vulnerability

Please use
[GitHub private vulnerability reporting](https://github.com/P4suta/windows-spawn/security/advisories/new).
Do not open a public issue for a suspected vulnerability.

Include the affected version or commit, supported Windows version and
architecture, a minimal reproduction, the impact you believe is possible, and
whether the report involves handle inheritance, Job ownership, process
mitigations, ConPTY, or command-line construction. Avoid including secrets or
unnecessary personal data.

The maintainer will acknowledge the report, investigate it, and coordinate
disclosure and a release when warranted. Response and remediation time depend
on severity and maintainer availability; this project does not promise a fixed
service-level objective.

## Security boundary

`windows-spawn` is a process-creation primitive, not a sandbox. Tokens, ACLs,
AppContainer, LPAC, capability SIDs, and supervision policy remain the caller's
responsibility. The public crate documentation describes the handle-inheritance
race and ownership contracts that are especially relevant to security reviews.
