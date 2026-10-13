//! Captures a tool's output while owning its process tree, so no descendant outlives the run.

#[cfg(windows)]
fn main() -> windows_spawn::Result<()> {
    use windows_spawn::Command;

    let output = Command::new(r"C:\Windows\System32\cmd.exe")
        .args(["/D", "/S", "/C"])
        .raw_arg("echo managed output")
        .own_process_tree()
        .output()?;

    assert!(output.status.success());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
