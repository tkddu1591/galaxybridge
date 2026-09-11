use crate::Result;
use std::{
    io::{Error, ErrorKind, Read},
    os::fd::AsRawFd,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 1024 * 1024;

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        // Child caches a reaped exit status; kill never targets a reused PID.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Capture<T> {
    pipe: T,
    bytes: Vec<u8>,
    closed: bool,
}

impl<T: Read + AsRawFd> Capture<T> {
    fn new(pipe: T) -> Result<Self> {
        let fd = pipe.as_raw_fd();
        // SAFETY: fd belongs to the live, owned pipe; these fcntl commands take
        // only integer arguments and retain all existing descriptor flags.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(Error::last_os_error().into());
        }
        Ok(Self {
            pipe,
            bytes: Vec::new(),
            closed: false,
        })
    }

    fn drain(&mut self, remaining: &mut usize) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let mut buffer = [0u8; 8192];
        // Bound work per stream so a prolific writer cannot starve stderr or
        // prevent the outer loop from checking its wall-clock deadline.
        for _ in 0..8 {
            match self.pipe.read(&mut buffer) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => {
                    if n > *remaining {
                        return Err("command output exceeded the combined 1 MiB limit".into());
                    }
                    self.bytes.extend_from_slice(&buffer[..n]);
                    *remaining -= n;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn descriptor(&self) -> libc::pollfd {
        libc::pollfd {
            fd: if self.closed {
                -1
            } else {
                self.pipe.as_raw_fd()
            },
            events: libc::POLLIN,
            revents: 0,
        }
    }
}

pub fn run(path: &str, args: &[&str]) -> Result<Output> {
    let mut child = Process(
        Command::new(path)
            .args(args)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut stdout = Capture::new(child.0.stdout.take().ok_or("missing command stdout")?)?;
    let mut stderr = Capture::new(child.0.stderr.take().ok_or("missing command stderr")?)?;
    let mut remaining = OUTPUT_LIMIT;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if Instant::now() >= deadline {
            return Err(format!("{path} timed out").into());
        }
        stdout.drain(&mut remaining)?;
        stderr.drain(&mut remaining)?;
        if let Some(status) = child.0.try_wait()? {
            // Descendants can still hold inherited pipes after the immediate
            // child exits. Keep draining, subject to the same bounded deadline.
            if stdout.closed && stderr.closed {
                return Ok(Output {
                    status,
                    stdout: std::mem::take(&mut stdout.bytes),
                    stderr: std::mem::take(&mut stderr.bytes),
                });
            }
        }
        let mut descriptors = [stdout.descriptor(), stderr.descriptor()];
        // SAFETY: descriptors is a live two-element pollfd array. Closed pipes
        // use -1, which poll explicitly ignores.
        if unsafe { libc::poll(descriptors.as_mut_ptr(), 2, 20) } < 0 {
            let error = Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }
}
pub fn text(path: &str, args: &[&str]) -> Result<String> {
    let output = run(path, args)?;
    if !output.status.success() {
        return Err(format!("{path}: {}", String::from_utf8_lossy(&output.stderr).trim()).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
