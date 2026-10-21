# Windows-spawn

Windows process creation with explicit handle ownership, Jobs, and pseudoconsoles.

## Installation

```sh
cargo add windows-spawn
```

## Usage

```rust
use windows_spawn::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("cmd.exe")
        .args(["/D", "/C", "echo hello"])
        .output()?;
    assert!(output.status.success());
    Ok(())
}
```

## Documentation

[Read the documentation](docs/README.md).

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [Massachusetts Institute of Technology (MIT) license](LICENSE-MIT), at your option.
You license each contribution you submit under the same two licenses, without other terms, unless you state otherwise.
