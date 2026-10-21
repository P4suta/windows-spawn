# Documentation

Use `std::process::Command` for portable child processes.
This crate adds Windows process trees, Jobs, ConPTY, explicit handle transfer, suspended inspection, alternate parents, priorities, and mitigation policies.
It requires Windows 10 version 1809 or later.
The current source supports Rust 1.84 or later and exposes no API on other platforms.
It is not affiliated with Microsoft or `windows-rs`.

- [Published API and examples](https://docs.rs/windows-spawn)
- [Examples in the current source](../examples)
- [Migration from 0.1 to the proposed 0.2 API](migration-0.2.md)
- [Contributing and local checks](../CONTRIBUTING.md)
- [CI selection](ci.md)
- [Verification layers](../proofs/README.md)
- [Architecture decisions](adr)
- [Security policy](../SECURITY.md)
