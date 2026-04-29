use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;

pub struct PtySession {
    pub(crate) master_fd: RawFd,
    pub(crate) child_pid: libc::pid_t,
}

pub fn spawn_pty(cmd: &str, env: &[(&str, &str)]) -> io::Result<PtySession> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;

    let ret = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    for (k, v) in env {
        std::env::set_var(k, v);
    }

    let pid = unsafe { libc::fork() };
    match pid {
        -1 => {
            unsafe { libc::close(master); libc::close(slave); }
            Err(io::Error::last_os_error())
        }
        0 => {
            // child
            unsafe {
                libc::setsid();
                libc::ioctl(slave, libc::TIOCSCTTY.into(), 0);
                libc::dup2(slave, 0);
                libc::dup2(slave, 1);
                libc::dup2(slave, 2);
                if slave > 2 { libc::close(slave); }
                libc::close(master);
            }
            let cmd_c = CString::new(cmd).expect("cmd nul");
            let args: &[*const libc::c_char] = &[cmd_c.as_ptr(), std::ptr::null()];
            unsafe { libc::execvp(cmd_c.as_ptr(), args.as_ptr()) };
            std::process::exit(1);
        }
        child_pid => {
            // parent
            unsafe { libc::close(slave); }
            Ok(PtySession { master_fd: master, child_pid })
        }
    }
}

impl PtySession {
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::write(self.master_fd, buf.as_ptr() as *const libc::c_void, buf.len())
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::read(self.master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        let ret = unsafe { libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws) };
        if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    pub fn kill(&self) -> io::Result<()> {
        unsafe {
            libc::kill(self.child_pid, libc::SIGTERM);
            libc::waitpid(self.child_pid, std::ptr::null_mut(), 0);
            libc::close(self.master_fd);
        }
        Ok(())
    }
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_pty_sh_echo() {
        let session = spawn_pty("sh", &[]).expect("spawn_pty failed");
        session.write(b"echo hello\n").expect("write failed");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut buf = [0u8; 256];

        while Instant::now() < deadline {
            match session.read(&mut buf) {
                Ok(n) if n > 0 => {
                    output.extend_from_slice(&buf[..n]);
                    let s = String::from_utf8_lossy(&output);
                    if s.contains("hello") {
                        session.kill().ok();
                        return;
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        session.kill().ok();
        panic!("PTY did not yield 'hello' within 5s; got: {:?}", String::from_utf8_lossy(&output));
    }
}
