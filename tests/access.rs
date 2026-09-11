#![cfg(target_os = "macos")]
use galaxybridge::macos::access::{PrivateFile, TrustedPath};
use std::{
    fs::{File, OpenOptions, Permissions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn create() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "galaxybridge-access-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn file(&self) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.0.join("state"))
            .unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn private_state_rejects_permissions_wrong_owner_hardlinks_and_special_files() {
    let scratch = Scratch::create();
    let file = scratch.file();
    let owner = unsafe { libc::geteuid() };
    PrivateFile::check(&file, owner).unwrap();
    assert!(PrivateFile::check(&file, owner.wrapping_add(1)).is_err());
    file.set_permissions(Permissions::from_mode(0o644)).unwrap();
    assert!(PrivateFile::check(&file, owner).is_err());
    file.set_permissions(Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(scratch.0.join("state"), scratch.0.join("alias")).unwrap();
    assert!(PrivateFile::check(&file, owner).is_err());
    std::fs::remove_file(scratch.0.join("alias")).unwrap();
    PrivateFile::check(&file, owner).unwrap();
    assert!(PrivateFile::check(&File::open(&scratch.0).unwrap(), owner).is_err());
    let fifo = scratch.0.join("fifo");
    assert!(
        Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let fifo = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(fifo)
        .unwrap();
    assert!(PrivateFile::check(&fifo, owner).is_err());
}

#[test]
fn acl_grants_do_not_hide_behind_mode_0600() {
    let scratch = Scratch::create();
    let file = scratch.file();
    let owner = unsafe { libc::geteuid() };
    assert!(
        Command::new("/bin/chmod")
            .args(["+a", "everyone allow read,write"])
            .arg(scratch.0.join("state"))
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert!(PrivateFile::check(&file, owner).is_err());
    assert!(
        Command::new("/bin/chmod")
            .arg("-N")
            .arg(scratch.0.join("state"))
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("/bin/chmod")
            .args(["+a", "everyone deny execute"])
            .arg(scratch.0.join("state"))
            .status()
            .unwrap()
            .success()
    );
    PrivateFile::check(&file, owner).unwrap();
}

#[test]
fn root_directory_validation_accepts_real_root_but_rejects_untrusted_symlinks() {
    TrustedPath::directory(std::path::Path::new("/")).unwrap();
    let scratch = Scratch::create();
    let link = scratch.0.join("root-link");
    std::os::unix::fs::symlink("/", &link).unwrap();
    assert!(TrustedPath::directory(&link).is_err());
    assert_eq!(
        TrustedPath::directory(&scratch.0).is_ok(),
        unsafe { libc::geteuid() } == 0
    );
}
