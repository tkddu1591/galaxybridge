#![cfg(target_os = "macos")]

use galaxybridge::macos::command::Runner;
use std::{os::unix::process::CommandExt, process::Command, time::Duration};

#[test]
fn asuser_preserves_current_user_pid_and_bootstrap_on_this_host() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "bootstrap_child", "--nocapture"])
        .env_clear()
        .env("LC_ALL", "C")
        .env("GALAXYBRIDGE_CONTEXT_TEST", "launch");
    let output = Runner::capture(command, Duration::from_secs(10)).unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("bootstrap PID and UID preserved"));
}

#[test]
fn bootstrap_child() {
    match std::env::var("GALAXYBRIDGE_CONTEXT_TEST").as_deref() {
        Ok("launch") => {
            let error = Command::new("/bin/launchctl")
                .args(["asuser", &unsafe { libc::getuid() }.to_string()])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "bootstrap_child", "--nocapture"])
                .env_clear()
                .env("LC_ALL", "C")
                .env("GALAXYBRIDGE_CONTEXT_TEST", "target")
                .env(
                    "GALAXYBRIDGE_CONTEXT_TEST_PID",
                    std::process::id().to_string(),
                )
                .exec();
            panic!("asuser exec failed: {error}");
        }
        Ok("target") => {
            assert_eq!(
                std::env::var("GALAXYBRIDGE_CONTEXT_TEST_PID").unwrap(),
                std::process::id().to_string()
            );
            let output = Runner::capture(
                {
                    let mut command = Command::new("/bin/launchctl");
                    command.arg("manageruid");
                    command
                },
                Duration::from_secs(3),
            )
            .unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap().trim(),
                unsafe { libc::getuid() }.to_string()
            );
            println!("bootstrap PID and UID preserved");
        }
        _ => (),
    }
}
