# CI selection

`CI` runs on pull requests and merge groups.
One Ubuntu 24.04 job always runs formatting, Clippy, repository laws, unit and integration tests available on Linux, the empty non-Windows API check, dependency policy, REUSE, typos, and actionlint.
`mise.toml` pins its tools.
Windows-only doctests run with the native Windows tests; Linux still executes all supported unit and integration targets.

[The path filters](../.github/filters/ci.yml) select additional contracts.

| Contract | Selected verification |
| --- | --- |
| Native source, tests, build inputs, or replay vectors | Windows Clippy, x64 tests on Rust 1.84.1 and 1.99.1, i686 ConPTY regression, native ARM64 tests |
| Native ownership or memory boundary | Windows Application Verifier, including its misuse controls |
| Library API or snapshot toolchain | Windows public API snapshot |
| Package contents, documentation, or packaging command | Windows documentation, doctests, and package contents and build check |
| A decision core or proof input | Only the corresponding Kani, Lean, Verus, TLA+, Quint, Alloy, or Prolog layer |

The `required` job accepts only a successful fast job and the exact expected success or skip for every selected contract.
Missing results, malformed flags, unknown or duplicate proof layers, cancellation, failure, and skipping a selected job refuse the gate.
Proof commands retain their complete expected inventories, source links, and negative controls.
Changing selection or workflow configuration requires checking those filters and the aggregate, without repeating unrelated proof models.

Run commands through `mise x -- just COMMAND` locally.
`just ci` remains the complete ordinary local suite on Windows.
Install Rust 1.84 with rustup for `just msrv`, the three Windows MSVC targets for `just cross-targets`, and the toolchain in `public-api/rust-toolchain` plus cargo-public-api 0.52.0 for `just public-api`.
`just reuse` needs the pinned REUSE environment: `mise x -- uv run --no-project --with 'reuse[charset-normalizer]==6.2.0' cargo xtask reuse`.
Windows execution requires Windows; Application Verifier additionally requires its SDK tool and administrative rights.

Coverage, allocation poisoning, broad cleanup stress, fuzzing, and mutation support a specific local investigation.
They have no recurring hosted campaign.
The protected release workflow retains reproducible candidates and publication evidence.
Rust 1.84 compatibility remains explicit; sha2 stays below 0.11 while that workspace contract applies.
The public API nightly has one declaration shared by installation and snapshot generation, which Renovate tracks as a Rust toolchain file.
