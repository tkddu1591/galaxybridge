use crate::{Result, usb::devices::Filter};
use std::{
    os::{
        fd::{AsRawFd, RawFd},
        unix::{net::UnixDatagram, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn gb_worker_prepare(source: libc::c_int) -> libc::c_int;
}

pub struct Worker {
    pub child: Child,
}
impl Worker {
    fn command(mode: &str, filter: &Filter, fd: RawFd) -> Result<Command> {
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.arg(mode)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(product) = filter.product {
            cmd.args(["--product", &format!("{product:04x}")]);
        }
        if let Some(serial) = &filter.serial {
            cmd.args(["--serial", serial]);
        }
        // SAFETY: the child callback uses only descriptor/credential syscalls, with no allocation.
        unsafe {
            cmd.pre_exec(move || {
                if gb_worker_prepare(fd) != 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        Ok(cmd)
    }
    pub fn probe(filter: &Filter) -> Result<bool> {
        let mut child = Self::command("probe", filter, -1)?.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.success());
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err("USB discovery timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn spawn(filter: &Filter, socket: &UnixDatagram) -> Result<Self> {
        Ok(Self {
            child: Self::command("worker", filter, socket.as_raw_fd())?.spawn()?,
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        // The owned Child remains unreaped, so its PID cannot be reused here.
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
