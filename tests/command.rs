#![cfg(target_os = "macos")]

use galaxybridge::macos::command;
use std::time::{Duration, Instant};

#[test]
fn simultaneous_large_stdout_and_stderr_do_not_fill_the_pipes() {
    let output = command::run(
        "/bin/sh",
        &[
            "-c",
            "/usr/bin/head -c 262144 /dev/zero & /usr/bin/head -c 262144 /dev/zero >&2 & wait",
        ],
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, vec![0; 262144]);
    assert_eq!(output.stderr, vec![0; 262144]);
}

#[test]
fn output_limit_is_combined_and_never_returns_truncated_success() {
    let exact = command::run("/usr/bin/head", &["-c", "1048576", "/dev/zero"]).unwrap();
    assert!(exact.status.success());
    assert_eq!(exact.stdout.len(), 1048576);
    let error = command::run(
        "/bin/sh",
        &[
            "-c",
            "/usr/bin/head -c 600000 /dev/zero & /usr/bin/head -c 600000 /dev/zero >&2 & wait",
        ],
    )
    .unwrap_err();
    assert!(error.to_string().contains("combined 1 MiB limit"));
}

#[test]
fn output_is_drained_after_child_exit_until_all_writers_close() {
    let output = command::run(
        "/bin/sh",
        &["-c", "(/bin/sleep 0.1; printf delayed) & exit 0"],
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"delayed");
}

#[test]
fn hung_child_is_stopped_at_the_wall_clock_deadline() {
    let start = Instant::now();
    let error = command::run("/bin/sleep", &["30"]).unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(start.elapsed() >= Duration::from_secs(8));
    assert!(start.elapsed() < Duration::from_secs(12));
}

#[test]
fn exit_status_and_stderr_are_preserved() {
    let output = command::run(
        "/bin/sh",
        &["-c", "printf result; printf failure >&2; exit 7"],
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"result");
    assert_eq!(output.stderr, b"failure");
    let error = command::text("/bin/sh", &["-c", "printf failure >&2; exit 7"]).unwrap_err();
    assert!(error.to_string().ends_with("failure"));
}
