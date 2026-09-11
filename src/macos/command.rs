use crate::Result;
use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub fn run(path: &str, args: &[&str]) -> Result<Output> {
    let mut child = Command::new(path)
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{path} timed out").into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(child.wait_with_output()?)
}
pub fn text(path: &str, args: &[&str]) -> Result<String> {
    let output = run(path, args)?;
    if !output.status.success() {
        return Err(format!("{path}: {}", String::from_utf8_lossy(&output.stderr).trim()).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
