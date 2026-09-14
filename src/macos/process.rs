use super::{
    access::Descriptors,
    command::Runner,
    identity::{self, Account, Caller},
    worker_app::{self, Bundle},
};
use crate::{Result, usb::devices::Filter};
use std::{
    io::{ErrorKind, Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{net::UnixDatagram, process::CommandExt},
    },
    process::{Child, ChildStderr, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn gb_inspection_prepare(ceiling: libc::c_int) -> libc::c_int;
    fn gb_worker_prepare(
        source: libc::c_int,
        ceiling: libc::c_int,
        uid: libc::uid_t,
        gid: libc::gid_t,
    ) -> libc::c_int;
}

pub struct Worker {
    pub child: Child,
    diagnostic: Option<Diagnostic>,
}
impl Worker {
    pub fn adopt(child: Child) -> Result<Self> {
        let mut worker = Self {
            child,
            diagnostic: None,
        };
        if let Some(pipe) = worker.child.stderr.take() {
            let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
            if flags < 0
                || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) }
                    < 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            worker.diagnostic = Some(Diagnostic {
                pipe,
                remaining: 8192,
                exceeded: false,
            });
        }
        Ok(worker)
    }

    pub fn diagnostics(&mut self) -> Result<()> {
        if let Some(diagnostic) = &mut self.diagnostic {
            diagnostic.drain()?;
        }
        Ok(())
    }

    pub fn poll(&mut self) -> Result<Option<ExitStatus>> {
        self.diagnostics()?;
        let status = self.child.try_wait()?;
        if status.is_some() {
            // At most 8 KiB can remain in our lifetime allowance. Read one
            // bounded surplus chunk too, so an exited probe cannot hide a
            // diagnostic overflow behind its otherwise expected exit code 2.
            for _ in 0..3 {
                self.diagnostics()?;
            }
        }
        Ok(status)
    }

    fn create(mode: &str, filter: &Filter) -> Command {
        let mut cmd = Command::new(worker_app::EXECUTABLE);
        cmd.arg(mode)
            .env_clear()
            .env("LC_ALL", "C")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(vendor) = filter.vendor {
            cmd.args(["--vendor", &format!("{vendor:04x}")]);
        }
        if let Some(product) = filter.product {
            cmd.args(["--product", &format!("{product:04x}")]);
        }
        if let Some(serial) = &filter.serial {
            cmd.args(["--serial", serial]);
        }
        cmd
    }

    fn command(mode: &str, filter: &Filter, fd: RawFd, account: &Account) -> Result<Command> {
        let ceiling = Descriptors::ceiling()?;
        let (uid, gid) = (account.uid, account.gid);
        let mut cmd = Self::create(mode, filter);
        cmd.env("HOME", identity::HOME).stdout(Stdio::null());
        // SAFETY: the child callback uses only descriptor/credential syscalls, with no allocation.
        unsafe {
            cmd.pre_exec(move || {
                if gb_worker_prepare(fd, ceiling, uid, gid) != 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        Ok(cmd)
    }
    pub fn inspect(mode: &str, filter: &Filter, show_serial: bool) -> Result<()> {
        if unsafe { libc::geteuid() } == 0 {
            return Err("run USB inspection without sudo".into());
        }
        if !matches!(mode, "devices" | "handshake") || (show_serial && mode != "devices") {
            return Err("invalid USB inspection mode".into());
        }
        Bundle::check()?;
        let ceiling = Descriptors::ceiling()?;
        let mut cmd = Self::create(mode, filter);
        cmd.env("HOME", Caller::home()?);
        if show_serial {
            cmd.arg("--show-serial");
        }
        // SAFETY: this child-only callback changes descriptors/rlimits using
        // fixed syscall arguments and performs no allocation or user lookup.
        unsafe {
            cmd.pre_exec(move || {
                if gb_inspection_prepare(ceiling) != 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let output = Runner::capture(cmd, Duration::from_secs(10))?;
        std::io::stdout().write_all(Console::escape(&output.stdout).as_bytes())?;
        std::io::stderr().write_all(Console::escape(&output.stderr).as_bytes())?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!("USB inspection exited ({})", output.status).into())
    }
    pub fn probe(filter: &Filter, account: &Account) -> Result<bool> {
        let mut worker = Self::adopt(Self::command("probe", filter, -1, account)?.spawn()?)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = worker.poll()? {
                return Discovery::check(status);
            }
            if Instant::now() >= deadline {
                worker.stop()?;
                return Err("USB discovery timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn spawn(filter: &Filter, socket: &UnixDatagram, account: &Account) -> Result<Self> {
        Self::adopt(Self::command("worker", filter, socket.as_raw_fd(), account)?.spawn()?)
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

struct Diagnostic {
    pipe: ChildStderr,
    remaining: usize,
    exceeded: bool,
}
impl Diagnostic {
    fn drain(&mut self) -> Result<()> {
        if self.exceeded {
            return Err("USB worker diagnostic output exceeded its 8 KiB lifetime limit".into());
        }
        let mut bytes = [0u8; 1024];
        for _ in 0..4 {
            match self.pipe.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    if count > self.remaining {
                        self.exceeded = true;
                        return Err(
                            "USB worker diagnostic output exceeded its 8 KiB lifetime limit".into(),
                        );
                    }
                    self.remaining -= count;
                    // Debug quoting prevents terminal escapes and forged log
                    // lines in output from a compromised parsing process.
                    eprintln!(
                        "GalaxyBridge: USB worker: {:?}",
                        String::from_utf8_lossy(&bytes[..count])
                    );
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

pub struct Console;
impl Console {
    pub fn escape(bytes: &[u8]) -> String {
        let mut result = String::new();
        for character in String::from_utf8_lossy(bytes).chars() {
            if matches!(character, '\n' | '\t' | '\'' | '"' | '\\') {
                result.push(character);
            } else {
                result.extend(character.escape_debug());
            }
        }
        result
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("GalaxyBridge: worker cleanup: {error}");
        }
        // IPC can close before the supervisor's next poll. Preserve the last
        // bounded diagnostics after the worker has been stopped/reaped.
        if let Err(error) = self.diagnostics() {
            eprintln!("GalaxyBridge: final worker diagnostics: {error}");
        }
    }
}

pub struct Discovery;
impl Discovery {
    pub fn check(status: ExitStatus) -> Result<bool> {
        match status.code() {
            Some(0) => Ok(true),
            Some(2) => Ok(false),
            _ => Err(format!("USB discovery worker exited ({status})").into()),
        }
    }
}
