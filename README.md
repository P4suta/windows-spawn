# windows-spawn

Ownership-oriented Windows process creation for the parts of `CreateProcessW`
that stable `std::process` cannot express safely: explicit handle transfer,
ordered Job attachment, process mitigations, ConPTY, and suspended creation.

## Supported environment

`windows-spawn` is Windows-only. Its core process-creation and ConPTY baseline
is Windows 10 version 1809. It uses Rust 2021 and supports Rust 1.75 and later.
Non-Windows targets expose no public API, so cross-platform dependency graphs
can still be checked.

## Installation

```toml
[dependencies]
windows-spawn = "0.1"
```

## Minimal example

This captures output and owns the descendant tree, ensuring that a grandchild
cannot keep the output pipe open after the root process exits:

```rust,no_run
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use windows_spawn::{Command, DropPolicy, SpawnOptions};

    let mut command = Command::new(r"C:\Windows\System32\cmd.exe");
    command.args(["/D", "/S", "/C"]).raw_arg("echo hello");

    let output = command.output_with(
        SpawnOptions::new().drop_policy(DropPolicy::KillTree),
    )?;
    assert!(output.status.success());
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
```

## Safety and ownership

- `DropPolicy::KillTree` owns and terminates the whole descendant tree;
  dropping a normal child detaches by default.
- Handle-handoff methods transfer private duplicates. Child-visible decimal
  handle values belong to the child process and may differ from source values.
- Windows has a process-wide reverse inheritance race while temporary
  inheritable duplicates exist. Do not concurrently perform broad-inheritance
  spawns when transferred handles are sensitive.
- This crate is a process-creation primitive, not a sandbox or process
  supervisor. Review the documented security boundary before using it as part
  of an isolation design.

## Documentation

- [API documentation and behavioral contracts](https://docs.rs/windows-spawn)
- [Compile-checked examples](https://github.com/P4suta/windows-spawn/tree/main/examples)
- [Architecture decision records](https://github.com/P4suta/windows-spawn/tree/main/docs/adr)
- [Security policy](https://github.com/P4suta/windows-spawn/security/policy)

## License

Licensed under either Apache-2.0 or MIT, at your option.
