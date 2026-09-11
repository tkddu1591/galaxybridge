#![cfg(target_os = "macos")]
use galaxybridge::macos::{access::Descriptors, identity::Account};
use std::{
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{net::UnixDatagram, process::CommandExt},
    },
    process::Command,
    time::Duration,
};

unsafe extern "C" {
    fn gb_worker_descriptors(source: libc::c_int, ceiling: libc::c_int) -> libc::c_int;
    fn gb_inspection_prepare(ceiling: libc::c_int) -> libc::c_int;
    fn gb_worker_prepare(
        source: libc::c_int,
        ceiling: libc::c_int,
        uid: libc::uid_t,
        gid: libc::gid_t,
    ) -> libc::c_int;
}

// Child-only branch of the real exec regression; the normal test invocation
// does nothing. All rlimit changes below are confined to isolated child tasks.
#[test]
fn descriptor_child() {
    let Ok(role) = std::env::var("GALAXYBRIDGE_DESCRIPTOR_TEST") else {
        return;
    };
    if role == "receiver" {
        for fd in std::env::var("GALAXYBRIDGE_TEST_FDS").unwrap().split(',') {
            let fd: i32 = fd.parse().unwrap();
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EBADF)
            );
        }
        assert_eq!(
            unsafe { libc::send(3, b"isolated".as_ptr().cast(), 8, 0) },
            8
        );
        return;
    }
    if role == "limits" {
        assert_eq!(unsafe { libc::getsid(0) }, unsafe { libc::getpid() });
        assert!(
            File::open("/dev/tty").is_err(),
            "prepared child must not retain a controlling terminal"
        );
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
        assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0
        );
        assert_eq!((limit.rlim_cur, limit.rlim_max), (256, 256));
        return;
    }
    if role == "terminal" {
        let mut master = -1;
        let mut slave = -1;
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let _master = unsafe { OwnedFd::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        assert_eq!(unsafe { libc::setsid() }, unsafe { libc::getpid() });
        assert_eq!(
            unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY as libc::c_ulong, 0) },
            0
        );
        // PTY closure may hang up this isolated fixture's session; it has no
        // relationship to any user terminal or installed service.
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
        }
        assert!(File::open("/dev/tty").is_ok());
        let ceiling = Descriptors::ceiling().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "descriptor_child", "--nocapture"])
            .env("GALAXYBRIDGE_DESCRIPTOR_TEST", "limits");
        unsafe {
            child.pre_exec(move || {
                if gb_inspection_prepare(ceiling) != 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    assert_eq!(role, "sender");
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    limit.rlim_cur = limit.rlim_cur.max(1100);
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let file = File::open("/dev/null").unwrap();
    let normal = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 50) };
    let high = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 1000) };
    assert!(normal >= 50 && high >= 1000);
    let normal = unsafe { OwnedFd::from_raw_fd(normal) };
    let high = unsafe { OwnedFd::from_raw_fd(high) };
    assert_eq!(
        unsafe { libc::fcntl(high.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    limit.rlim_cur = 64;
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let ceiling = Descriptors::ceiling().unwrap();
    assert!(
        ceiling > high.as_raw_fd(),
        "existing descriptors above the new soft limit must be included"
    );
    let (parent, child) = UnixDatagram::pair().unwrap();
    let source = child.as_raw_fd();
    parent
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "descriptor_child", "--nocapture"])
        .env("GALAXYBRIDGE_DESCRIPTOR_TEST", "receiver")
        .env(
            "GALAXYBRIDGE_TEST_FDS",
            format!("{},{}", normal.as_raw_fd(), high.as_raw_fd()),
        );
    unsafe {
        command.pre_exec(move || {
            if gb_worker_descriptors(source, ceiling) != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = [0u8; 16];
    let received = parent.recv(&mut bytes).unwrap();
    assert_eq!(&bytes[..received], b"isolated");
}

#[test]
fn exec_closes_inheritable_capabilities_even_above_a_lowered_fd_limit() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "descriptor_child", "--nocapture"])
        .env("GALAXYBRIDGE_DESCRIPTOR_TEST", "sender")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cloexec_sweep_keeps_rusts_spawn_failure_channel_working() {
    let ceiling = Descriptors::ceiling().unwrap();
    let mut command = Command::new("/usr/bin/true");
    unsafe {
        command.pre_exec(move || {
            if gb_worker_descriptors(-1, ceiling) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Err(std::io::Error::from_raw_os_error(libc::EPERM))
        });
    }
    assert_eq!(
        command.spawn().unwrap_err().raw_os_error(),
        Some(libc::EPERM)
    );
    let mut missing =
        Command::new("/Library/PrivilegedHelperTools/galaxybridge-missing-test-executable");
    unsafe {
        missing.pre_exec(move || {
            if gb_worker_descriptors(-1, ceiling) != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    assert_eq!(
        missing.spawn().unwrap_err().raw_os_error(),
        Some(libc::ENOENT)
    );
}

#[test]
fn inspection_child_has_no_core_dump_and_bounded_descriptors() {
    let ceiling = Descriptors::ceiling().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "descriptor_child", "--nocapture"])
        .env("GALAXYBRIDGE_DESCRIPTOR_TEST", "limits");
    unsafe {
        command.pre_exec(move || {
            if gb_inspection_prepare(ceiling) != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prepared_child_detaches_from_an_actual_controlling_pty() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "descriptor_child", "--nocapture"])
        .env("GALAXYBRIDGE_DESCRIPTOR_TEST", "terminal")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires root and the installed dedicated identity; no USB or network changes"]
fn installed_worker_identity_drops_real_effective_and_saved_root_credentials() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let account = Account::load().unwrap();
    let ceiling = Descriptors::ceiling().unwrap();
    for (argument, expected) in [
        ("-u", account.uid),
        ("-ru", account.uid),
        ("-g", account.gid),
        ("-rg", account.gid),
    ] {
        let (uid, gid) = (account.uid, account.gid);
        let mut command = Command::new("/usr/bin/id");
        command.arg(argument).env_clear().current_dir("/");
        // gb_worker_prepare also verifies no unrelated supplementary groups
        // and refuses exec if root UID/GID can be restored after dropping.
        unsafe {
            command.pre_exec(move || {
                if gb_worker_prepare(-1, ceiling, uid, gid) != 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            expected.to_string()
        );
    }
}
