#![cfg(target_os = "macos")]
use galaxybridge::macos::{
    command::Runner,
    process::{Console, Discovery, Worker},
};
use std::{
    os::unix::process::ExitStatusExt,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

#[test]
fn console_relay_escapes_terminal_control_sequences_without_losing_text_layout() {
    let text = "serial=\"ABC\"\n한글\ttext\\value";
    assert_eq!(Console::escape(text.as_bytes()), text);
    let hostile = b"\x1b]52;c;ZmFrZQ==\x07\r\x00";
    let escaped = Console::escape(hostile);
    assert!(!escaped.contains(['\x1b', '\x07', '\r', '\0']));
    assert!(!Console::escape("\u{202e}fake".as_bytes()).contains('\u{202e}'));
}

#[test]
fn worker_stderr_flood_hits_a_monotonic_lifetime_limit_without_blocking() {
    let child = Command::new("/bin/sh")
        .args(["-c", "exec /usr/bin/head -c 9000 /dev/zero >&2"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut worker = Worker::adopt(child).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Err(error) = worker.diagnostics() {
            assert!(error.to_string().contains("8 KiB lifetime limit"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "diagnostic drain failed to enforce its bound"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        worker.diagnostics().is_err(),
        "an exhausted log budget must never reset"
    );
}

#[test]
fn inspection_capture_overrides_inherited_stdio_with_write_only_pipes() {
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "test ! -t 0 && test ! -t 1 && test ! -t 2 && printf output && printf error >&2",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let output = Runner::capture(command, Duration::from_secs(2)).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"output");
    assert_eq!(output.stderr, b"error");
}

#[test]
fn discovery_never_hides_a_sandbox_failure_or_signal_as_no_device() {
    assert!(Discovery::check(ExitStatus::from_raw(0)).unwrap());
    assert!(!Discovery::check(ExitStatus::from_raw(2 << 8)).unwrap());
    for raw in [1 << 8, 126 << 8, 127 << 8, libc::SIGABRT, libc::SIGKILL] {
        assert!(Discovery::check(ExitStatus::from_raw(raw)).is_err());
    }
}

#[test]
fn unavailable_exit_code_cannot_hide_excess_diagnostics() {
    let child = Command::new("/bin/sh")
        .args(["-c", "/usr/bin/head -c 9000 /dev/zero >&2; exit 2"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut worker = Worker::adopt(child).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match worker.poll() {
            Err(error) => {
                assert!(error.to_string().contains("8 KiB lifetime limit"));
                break;
            }
            Ok(Some(status)) => panic!("diagnostic overflow was hidden by {status}"),
            Ok(None) => (),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}
