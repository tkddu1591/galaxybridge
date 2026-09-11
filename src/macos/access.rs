//! File and inherited-descriptor checks at the privileged process boundary.
use crate::Result;
use std::{
    fs::{File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};

unsafe extern "C" {
    fn gb_file_check(fd: libc::c_int, owner: libc::uid_t, mode: libc::mode_t) -> libc::c_int;
    fn gb_directory_check(fd: libc::c_int) -> libc::c_int;
}

pub struct PrivateFile;
impl PrivateFile {
    pub fn check(file: &File, owner: libc::uid_t) -> Result<()> {
        // SAFETY: the owned descriptor remains valid for this non-owning check.
        if unsafe { gb_file_check(file.as_raw_fd(), owner, 0o600) } != 0 {
            return Err("state file must be a singly linked regular file, mode 0600, owned by the expected user, without ACL grants".into());
        }
        Ok(())
    }
}

pub struct TrustedPath;
impl TrustedPath {
    pub fn file(path: &Path, mode: libc::mode_t) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)?;
        if unsafe { gb_file_check(file.as_raw_fd(), 0, mode) } != 0 {
            return Err("worker bundle file ownership, permissions or ACL is unsafe".into());
        }
        Ok(())
    }

    pub fn directory(path: &Path) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY)
            .open(path)?;
        if unsafe { gb_directory_check(file.as_raw_fd()) } != 0 {
            return Err("worker bundle directory ownership, permissions or ACL is unsafe".into());
        }
        Ok(())
    }
}

pub struct Descriptors;
impl Descriptors {
    pub fn ceiling() -> Result<libc::c_int> {
        let mut limits = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: limits is a live rlimit with sufficient space for the syscall.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut ceiling = limits.rlim_cur.max(4);
        // Lowering RLIMIT_NOFILE does not close existing higher descriptors.
        // Include those in the exec sweep as well. The root supervisor does
        // not create background threads or change this limit after this check.
        for entry in std::fs::read_dir("/dev/fd")? {
            let entry = entry?;
            if let Some(number) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u64>().ok())
            {
                ceiling = ceiling.max(number.checked_add(1).ok_or("descriptor limit overflow")?);
            }
        }
        // Bound the pre-exec syscall loop rather than silently leaving high
        // descriptors open on an unusually configured supervisor.
        if ceiling > 1_048_576 {
            return Err("descriptor table exceeds the bounded worker exec policy".into());
        }
        Ok(ceiling as libc::c_int)
    }
}

pub struct Core;
impl Core {
    pub fn disable() -> Result<()> {
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: limit is initialized and lives through this syscall.
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}
