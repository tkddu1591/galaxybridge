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
        let mut worker = Self {
            child: Self::command("probe", filter, -1)?.spawn()?,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = worker.child.try_wait()? {
                return Ok(status.success());
            }
            if Instant::now() >= deadline {
                worker.stop()?;
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

    fn reap(&mut self, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.child.try_wait()?.is_some() {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.reap(Duration::ZERO)? {
            return Ok(());
        }
        // The owned child is still unreaped, so its PID cannot have been reused.
        // A missing child can race with exit; try_wait below performs the reap.
        if unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
        if self.reap(Duration::from_secs(2))? {
            return Ok(());
        }
        self.child.kill()?;
        if self.reap(Duration::from_secs(1))? {
            return Ok(());
        }
        Err("USB worker did not exit within the cleanup deadline".into())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("GalaxyBridge: worker cleanup: {error}");
        }
    }
}
