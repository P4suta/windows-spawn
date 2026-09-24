# windows-spawn

[![CI](https://github.com/P4suta/windows-spawn/actions/workflows/ci.yml/badge.svg)](https://github.com/P4suta/windows-spawn/actions/workflows/ci.yml)

Windows process creation with explicit handle transfer, ordered Job attachment, mitigation policies, ConPTY, and suspended inspection.

Use `std::process::Command` for portable child processes.
Use this crate when `CreateProcessW` features need explicit ownership and rollback.

Requires Windows 10 version 1809 or later and Rust 1.75 or later.
Non-Windows targets expose no API.

Not affiliated with Microsoft or `windows-rs`; `windows-` names the target platform.

## Installation

```console
cargo add windows-spawn
```

## Example

```rust
use windows_spawn::{Command, DropPolicy, SpawnOptions};

let mut command = Command::new(r"C:\Windows\System32\cmd.exe");
command.args(["/D", "/S", "/C"]).raw_arg("echo hello");

let output = command.output_with(
    SpawnOptions::new().drop_policy(DropPolicy::KillTree),
)?;
assert!(output.status.success());
# Ok::<(), std::io::Error>(())
```

## Documentation

- [API and contracts](https://docs.rs/windows-spawn)
- [Examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decisions](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)

## License

Apache-2.0 OR MIT.
