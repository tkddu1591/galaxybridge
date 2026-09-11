#![cfg(target_os = "macos")]

use galaxybridge::macos::{
    process::Worker,
    route::{Recovery, Snapshot},
};
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn fallback_uses_the_current_gateway_and_never_replays_an_expired_snapshot() {
    let previous = Snapshot::parse("gateway: 192.0.2.1\ninterface: en0").unwrap();
    let fresh = Snapshot::parse("gateway: 192.0.2.254\ninterface: en0").unwrap();
    assert_eq!(
        Recovery::select(Some(&previous), Some(fresh.clone()), true),
        Some(fresh)
    );
    assert!(Recovery::select(Some(&previous), None, true).is_none());
    assert!(Recovery::select(None, Some(previous), true).is_none());
}

#[test]
fn fallback_preserves_new_network_choices_and_vpn_defaults() {
    assert!(Recovery::permits(None, "feth11"));
    let owned = Snapshot::parse("gateway: 198.51.100.1\ninterface: feth11").unwrap();
    assert!(Recovery::permits(Some(&owned), "feth11"));
    for text in [
        "gateway: 192.0.2.1\ninterface: en0",
        "gateway: 192.0.2.2\ninterface: en7",
        "gateway: link#25\ninterface: utun2",
        "gateway: 198.51.100.1\ninterface: feth12",
    ] {
        let current = Snapshot::parse(text).unwrap();
        assert!(!Recovery::permits(Some(&current), "feth11"), "{text}");
    }
}

#[test]
fn fallback_requires_a_live_matching_physical_service() {
    let previous = Snapshot::parse("gateway: 192.0.2.1\ninterface: en0").unwrap();
    assert!(Recovery::select(Some(&previous), Some(previous.clone()), false).is_none());
    let unrelated = Snapshot::parse("gateway: 192.0.2.1\ninterface: en7").unwrap();
    assert!(Recovery::select(Some(&previous), Some(unrelated), true).is_none());
    for text in [
        "gateway: 192.0.2.1\ninterface: feth9",
        "gateway: 192.0.2.1\ninterface: utun3",
        "gateway: link#25\ninterface: en0",
        "gateway: 0.0.0.0\ninterface: en0",
        "gateway: 127.0.0.1\ninterface: en0",
        "gateway: 224.0.0.1\ninterface: en0",
        "gateway: 255.255.255.255\ninterface: en0",
    ] {
        let route = Snapshot::parse(text).unwrap();
        assert!(
            Recovery::select(Some(&route), Some(route.clone()), true).is_none(),
            "{text}"
        );
    }
}

#[test]
fn worker_cleanup_reaps_its_child_before_returning() {
    let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let pid = child.id() as libc::pid_t;
    let started = Instant::now();
    drop(Worker { child });
    assert!(started.elapsed() < Duration::from_secs(5));
    // ECHILD establishes that Worker reaped the process, rather than merely
    // sending a signal and leaving a zombie for the long-running supervisor.
    let result = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
    assert_eq!(result, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn worker_cleanup_escalates_and_reaps_a_child_that_ignores_termination() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "trap '' TERM; printf 'ready\\n'; exec /bin/sleep 30"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    let pid = child.id() as libc::pid_t;
    let started = Instant::now();
    drop(Worker { child });
    assert!(started.elapsed() < Duration::from_secs(5));
    let result = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
    assert_eq!(result, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}
