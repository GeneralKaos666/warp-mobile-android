use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::thread;

pub struct PtySession {
    pub(crate) master_fd: RawFd,
    pub(crate) child_pid: libc::pid_t,
    killed: AtomicBool,
}

/// Spawn a PTY-attached child process.
///
/// # Arguments
/// - `program`: absolute or PATH-resolved binary (e.g. "/bin/sh")
/// - `args`: argv[0..n] passed to execvp (first element is conventional argv[0])
/// - `env`: extra environment variable pairs set before fork
pub fn spawn_pty(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> io::Result<PtySession> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;

    // ── open PTY pair ────────────────────────────────────────────────────────
    let ret = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    // Fix #2: FD_CLOEXEC on master so it doesn't leak across exec in parent
    unsafe { libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC) };

    // ── Fix #1: pre-build all CStrings BEFORE fork ───────────────────────────
    let prog_cstr = CString::new(program).map_err(|_| {
        unsafe { libc::close(master); libc::close(slave); }
        io::Error::new(io::ErrorKind::InvalidInput, "program contains nul byte")
    })?;

    let arg_cstrs: Vec<CString> = args
        .iter()
        .map(|&a| CString::new(a))
        .collect::<Result<_, _>>()
        .map_err(|_| {
            unsafe { libc::close(master); libc::close(slave); }
            io::Error::new(io::ErrorKind::InvalidInput, "arg contains nul byte")
        })?;

    // argv: args if non-empty, else [prog] as argv[0]
    let mut argv_ptrs: Vec<*const libc::c_char> = if arg_cstrs.is_empty() {
        vec![prog_cstr.as_ptr()]
    } else {
        arg_cstrs.iter().map(|c| c.as_ptr()).collect()
    };
    argv_ptrs.push(ptr::null());

    // env key=value CStrings
    let env_cstrs: Vec<CString> = env
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
        .collect();
    let mut envp_ptrs: Vec<*const libc::c_char> =
        env_cstrs.iter().map(|c| c.as_ptr()).collect();
    envp_ptrs.push(ptr::null());

    // ── fork ─────────────────────────────────────────────────────────────────
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => {
            unsafe { libc::close(master); libc::close(slave); }
            Err(io::Error::last_os_error())
        }
        0 => {
            // ── child: only AS-safe calls after this point ───────────────────
            unsafe {
                libc::setsid();
                libc::ioctl(slave, libc::TIOCSCTTY.into(), 0i32);
                libc::dup2(slave, 0);
                libc::dup2(slave, 1);
                libc::dup2(slave, 2);
                if slave > 2 {
                    libc::close(slave);
                }
                libc::close(master);

                // Apply extra env vars
                if !envp_ptrs.is_empty() && !envp_ptrs[0].is_null() {
                    let mut ep = envp_ptrs.as_ptr();
                    while !(*ep).is_null() {
                        libc::putenv(*ep as *mut libc::c_char);
                        ep = ep.add(1);
                    }
                }

                libc::execvp(prog_cstr.as_ptr(), argv_ptrs.as_ptr());
                // execvp only returns on error — AS-safe exit
                libc::_exit(127);
            }
        }
        child_pid => {
            // ── parent ───────────────────────────────────────────────────────
            unsafe { libc::close(slave); }
            Ok(PtySession {
                master_fd: master,
                child_pid,
                killed: AtomicBool::new(false),
            })
        }
    }
}

impl PtySession {
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::write(
                self.master_fd,
                buf.as_ptr() as *const libc::c_void,
                buf.len(),
            )
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe {
            libc::read(
                self.master_fd,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe { libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws) };
        if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// Fix #3: SIGTERM + WNOHANG poll + SIGKILL escalation
    pub fn kill(&self) -> io::Result<()> {
        if self.killed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        unsafe { libc::kill(self.child_pid, libc::SIGTERM) };

        let deadline = Instant::now() + Duration::from_millis(1000);
        while Instant::now() < deadline {
            let mut status = 0i32;
            let r = unsafe {
                libc::waitpid(self.child_pid, &mut status, libc::WNOHANG)
            };
            if r > 0 {
                unsafe { libc::close(self.master_fd) };
                return Ok(());
            }
            if r < 0 {
                // ECHILD — already reaped
                unsafe { libc::close(self.master_fd) };
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }

        // Escalate to SIGKILL
        unsafe { libc::kill(self.child_pid, libc::SIGKILL) };
        unsafe { libc::waitpid(self.child_pid, ptr::null_mut(), 0) };
        unsafe { libc::close(self.master_fd) };
        Ok(())
    }
}

// Fix #4: Drop impl — reap child on drop
impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Fix #6: spawn_pty with explicit program + args; verify "hello\n" output
    #[test]
    fn test_pty_echo_hello() {
        // Use /bin/echo directly with args — avoids shell injection surface
        let session =
            spawn_pty("/bin/echo", &["echo", "hello"], &[]).expect("spawn_pty failed");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        let mut buf = [0u8; 256];

        while Instant::now() < deadline {
            match session.read(&mut buf) {
                Ok(n) if n > 0 => {
                    output.extend_from_slice(&buf[..n]);
                    if output.windows(6).any(|w| w == b"hello\n")
                        || output.windows(7).any(|w| w == b"hello\r\n")
                    {
                        return;
                    }
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }
        panic!(
            "PTY did not yield 'hello\\n' within 5s; got: {:?}",
            String::from_utf8_lossy(&output)
        );
    }

    /// Fix #4: Drop reaps child — after drop, kill -0 returns ESRCH
    #[test]
    fn test_drop_reaps_child() {
        let session =
            spawn_pty("/bin/sleep", &["sleep", "60"], &[]).expect("spawn_pty failed");
        let pid = session.child_pid;
        drop(session);
        // Give OS a moment
        thread::sleep(Duration::from_millis(100));
        let r = unsafe { libc::kill(pid, 0) };
        assert_eq!(r, -1, "process should be gone after drop");
        assert_eq!(
            unsafe { *libc::__error() },
            libc::ESRCH,
            "errno should be ESRCH"
        );
    }
}
