# windows-spawn

[![CI](https://github.com/P4suta/windows-spawn/actions/workflows/ci.yml/badge.svg)](https://github.com/P4suta/windows-spawn/actions/workflows/ci.yml)

Windows process creation with the API of `std::process::Command`, plus what `CreateProcessW` offers beyond it:
process trees that end with their owner, Jobs, pseudoconsoles, handle handoff, suspended inspection, alternate parents, priority classes, and mitigation policies.

Use `std::process::Command` for portable child processes.
Use this crate when a Windows program needs one of these, or a spawn that passes the child no stray handle and releases everything when a step fails.

Requires Windows 10 version 1809 or later and Rust 1.84 or later.
Non-Windows targets expose no API.

Not affiliated with Microsoft or `windows-rs`; `windows-` names the target platform.

## Installation

```console
cargo add windows-spawn
```

## Example

Run a tool, collect its output, and end every process it started:

```rust
use windows_spawn::Command;

let output = Command::new("cmd.exe")
    .args(["/D", "/C", "echo hello"])
    .own_process_tree()
    .output()?;
assert!(output.status.success());
# Ok::<(), windows_spawn::SpawnError>(())
```

The crate documentation leads with one example for each common task:
ending a process tree, running a console tool from a GUI program, running a batch script, attaching a pseudoconsole, and more.

## Documentation

- [API, common tasks, and contracts](https://docs.rs/windows-spawn)
- [Migrating from 0.1](https://github.com/P4suta/windows-spawn/blob/main/docs/migration-0.2.md)
- [Examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decisions](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Verification layers](https://github.com/P4suta/windows-spawn/blob/main/proofs/README.md)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)

## License

Apache-2.0 OR MIT.
