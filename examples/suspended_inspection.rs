//! Inspects the process and main-thread handles before resuming the child.

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use std::os::windows::io::AsHandle;

    use windows_spawn::Command;

    let mut command = Command::new(r"C:\Windows\System32\cmd.exe");
    command.args(["/D", "/C", "exit /b 0"]);

    let suspended = command.spawn_suspended()?;
    println!("created suspended process {}", suspended.id());
    let _process = suspended.as_handle();
    let _main_thread = suspended.main_thread_handle();

    let mut child = suspended.resume()?;
    assert!(child.wait()?.success());
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
