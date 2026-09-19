//! Captures output while owning the process tree through completion.

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use windows_spawn::{Command, JobClosePolicy, SpawnOptions};

    let mut command = Command::new(r"C:\Windows\System32\cmd.exe");
    command
        .args(["/D", "/S", "/C"])
        .raw_arg("echo managed output");
    let output = command
        .output_with(SpawnOptions::new().job_close_policy(JobClosePolicy::TerminateProcesses))?;

    assert!(output.status.success());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
