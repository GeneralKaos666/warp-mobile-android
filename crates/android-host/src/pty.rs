use std::ffi::CString;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};
use std::thread;
use std::ptr;

pub struct PtySession {
    pub(crate) master_fd: AtomicI32,
    pub(crate) child_pid: libc::pid_t,
    killed: AtomicBool,
}

/// Spawn a PTY-attached child process.
///
/// # Arguments
/// - `program`: **absolute path** to the binary (e.g. "/bin/sh"). No PATH lookup is
///   performed — the caller must resolve the path before fork for AS-safety.
/// - `args`: argv[0..n] passed to execve (first element is conventional argv[0])
/// - `env`: environment variable pairs passed as the full envp to execve
pub fn spawn_pty(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> io::Result<PtySession> {
    let mut master: i32 = -1;
    let mut slave: i32 = -1;

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

    // FD_CLOEXEC on master so it doesn't leak across exec in parent
    unsafe { libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC) };

    // ── pre-build all CStrings BEFORE fork ───────────────────────────────────
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

                // V1-prep defensive hardening: seed a non-zero TTY window size
                // so any TIOCGWINSZ before MainActivity sends its first
                // PTY_RESIZE returns a usable 24×80 instead of 0×0. zsh's ZLE
                // module logs a warning and falls back to a degraded
                // line-editor on 0×0 (per zsh source: Src/Zle/zle_main.c).
                // Real grid size is pushed by MainActivity onResume via the
                // resize() path on the master fd. NOTE: this alone does NOT
                // resolve v1-prep blocker #3 (zsh dies in PTY ~15 ms after
                // spawn even with this seed) — kept because it is correct
                // PTY hygiene and removes a known cause from the suspect list.
                let ws = libc::winsize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                libc::ioctl(slave, libc::TIOCSWINSZ.into(), &ws);

                libc::dup2(slave, 0);
                libc::dup2(slave, 1);
                libc::dup2(slave, 2);
                if slave > 2 {
                    libc::close(slave);
                }
                libc::close(master);

                // envp built in parent — use execve (no PATH lookup, fully AS-safe)
                libc::execve(prog_cstr.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                // execve only returns on error. Surface the errno via stderr
                // (now dup'd to slave → master → logcat tag PtyOutput) so the
                // post-mortem isn't a silent _exit(127). AS-safe: write(2) and
                // _exit(2) are both async-signal-safe per POSIX.1-2017.
                // Android-only: __errno() is the Bionic AS-safe errno accessor;
                // host tests on macOS use __error() which has the same role but
                // a different name. We only need this on-device anyway.
                #[cfg(target_os = "android")]
                {
                    let errno = *libc::__errno();
                    let msg = b"warp-pty: execve failed errno=";
                    libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
                    let mut digits = [0u8; 12];
                    let mut n = errno;
                    let mut i = digits.len();
                    if n == 0 {
                        i -= 1;
                        digits[i] = b'0';
                    } else {
                        let neg = n < 0;
                        if neg { n = -n; }
                        while n > 0 && i > 0 {
                            i -= 1;
                            digits[i] = b'0' + (n % 10) as u8;
                            n /= 10;
                        }
                        if neg && i > 0 { i -= 1; digits[i] = b'-'; }
                    }
                    libc::write(2, digits[i..].as_ptr() as *const libc::c_void, digits.len() - i);
                    libc::write(2, b"\n".as_ptr() as *const libc::c_void, 1);
                }
                libc::_exit(127);
            }
        }
        child_pid => {
            // ── parent ───────────────────────────────────────────────────────
            unsafe { libc::close(slave); }
            Ok(PtySession {
                master_fd: AtomicI32::new(master),
                child_pid,
                killed: AtomicBool::new(false),
            })
        }
    }
}

impl PtySession {
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let fd = self.master_fd.load(Ordering::Acquire);
        if fd < 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        let n = unsafe {
            libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len())
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let fd = self.master_fd.load(Ordering::Acquire);
        if fd < 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        let n = unsafe {
            libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let fd = self.master_fd.load(Ordering::Acquire);
        if fd < 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &ws) };
        if ret < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// Close master_fd immediately so concurrent reads return EBADF.
    /// Called by ptyKill before Arc ref-count decrement.
    pub fn kill_eager(&self) {
        let fd = self.master_fd.swap(-1, Ordering::AcqRel);
        if fd >= 0 {
            unsafe { libc::close(fd) };
        }
    }

    /// SIGTERM + WNOHANG poll + SIGKILL escalation + reap child
    pub fn kill(&self) -> io::Result<()> {
        if self.killed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // Close fd eagerly so any concurrent read unblocks
        self.kill_eager();

        unsafe { libc::kill(self.child_pid, libc::SIGTERM) };

        let deadline = Instant::now() + Duration::from_millis(1000);
        while Instant::now() < deadline {
            let mut status = 0i32;
            let r = unsafe {
                libc::waitpid(self.child_pid, &mut status, libc::WNOHANG)
            };
            if r > 0 { return Ok(()); }
            if r < 0 { return Ok(()); } // ECHILD — already reaped
            thread::sleep(Duration::from_millis(50));
        }

        // Escalate to SIGKILL
        unsafe { libc::kill(self.child_pid, libc::SIGKILL) };
        unsafe { libc::waitpid(self.child_pid, ptr::null_mut(), 0) };
        Ok(())
    }
}

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

    #[test]
    fn test_pty_echo_hello() {
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

    #[test]
    fn test_drop_reaps_child() {
        let session =
            spawn_pty("/bin/sleep", &["sleep", "60"], &[]).expect("spawn_pty failed");
        let pid = session.child_pid;
        drop(session);
        thread::sleep(Duration::from_millis(100));
        let r = unsafe { libc::kill(pid, 0) };
        assert_eq!(r, -1, "process should be gone after drop");
        // errno read: use std::io::Error::last_os_error for cross-platform
        // portability — `libc::__error()` is the macOS-specific symbol
        // (Linux uses `__errno_location`, Bionic uses `__errno`). The std
        // wrapper picks the right one per target. Caught by CI on Linux
        // ubuntu-latest where __error doesn't exist.
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "errno should be ESRCH"
        );
    }

    /// Concurrent read + kill via Arc — no use-after-free
    #[test]
    fn test_arc_concurrent_read_kill() {
        use std::sync::Arc;
        let session = Arc::new(
            spawn_pty("/bin/sleep", &["sleep", "10"], &[]).expect("spawn_pty failed"),
        );

        let readers: Vec<_> = (0..5).map(|_| {
            let s = Arc::clone(&session);
            thread::spawn(move || {
                let mut buf = [0u8; 256];
                let _ = s.read(&mut buf); // may return EBADF after kill — that's OK
            })
        }).collect();

        let killers: Vec<_> = (0..5).map(|_| {
            let s = Arc::clone(&session);
            thread::spawn(move || {
                s.kill_eager();
            })
        }).collect();

        for t in readers { let _ = t.join(); }
        for t in killers { let _ = t.join(); }
        // All threads finished without panic = no use-after-free
    }
}
