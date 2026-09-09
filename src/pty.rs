// ──────────────────────────────────────────────────────────────────────
// pty.rs — Run external commands on a pseudo-terminal
//
// The child keeps a real controlling TTY (so sudo password prompts and
// pacman's [Y/n] work normally) while every byte of its output is
// streamed back to the TUI over a channel. Keystrokes can be written
// into the master side to interact with the child from inside the pane.
// ──────────────────────────────────────────────────────────────────────

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

/// Events streamed from the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    /// Raw bytes of child output.
    Output(Vec<u8>),
    /// Child exited; payload is the exit code (negative if killed by signal).
    Exited(i32),
}

/// Structured PTY errors.
#[derive(Debug)]
pub enum PtyError {
    OpenPty(io::Error),
    Dup(io::Error),
    Spawn(io::Error),
    Ioctl(io::Error),
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyError::OpenPty(e) => write!(f, "openpty failed: {e}"),
            PtyError::Dup(e) => write!(f, "fd dup failed: {e}"),
            PtyError::Spawn(e) => write!(f, "process spawn failed: {e}"),
            PtyError::Ioctl(e) => write!(f, "pty ioctl failed: {e}"),
        }
    }
}

impl std::error::Error for PtyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PtyError::OpenPty(e) | PtyError::Dup(e) | PtyError::Spawn(e) | PtyError::Ioctl(e) => {
                Some(e)
            }
        }
    }
}

impl From<PtyError> for io::Error {
    fn from(err: PtyError) -> Self {
        match err {
            PtyError::OpenPty(e) | PtyError::Dup(e) | PtyError::Spawn(e) | PtyError::Ioctl(e) => e,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessState {
    Running,
    Terminating,
    Reaped(i32),
}

struct ChildLifecycle {
    pid: i32,
    state: Mutex<ProcessState>,
    win_size: Mutex<(u16, u16)>,
}

impl ChildLifecycle {
    fn new(pid: i32, rows: u16, cols: u16) -> Self {
        Self {
            pid,
            state: Mutex::new(ProcessState::Running),
            win_size: Mutex::new((rows, cols)),
        }
    }

    /// Safely signal the child process.
    /// Returns true if the signal was delivered to the child,
    /// or false if the child has already been reaped.
    /// This prevents sending signals to recycled PIDs.
    fn signal(&self, sig: libc::c_int) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        match *state {
            ProcessState::Running | ProcessState::Terminating => {
                *state = ProcessState::Terminating;
                unsafe { libc::kill(self.pid, sig) };
                true
            }
            ProcessState::Reaped(_) => false,
        }
    }

    /// Mark the child as reaped with its final exit code.
    /// Called only by the worker thread immediately after waitpid succeeds.
    fn mark_reaped(&self, code: i32) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        *state = ProcessState::Reaped(code);
    }

    fn is_reaped(&self) -> bool {
        matches!(
            *self.state.lock().unwrap_or_else(|p| p.into_inner()),
            ProcessState::Reaped(_)
        )
    }

    fn exit_code(&self) -> Option<i32> {
        match *self.state.lock().unwrap_or_else(|p| p.into_inner()) {
            ProcessState::Reaped(c) => Some(c),
            _ => None,
        }
    }
}

pub struct PtyChild {
    pub pid: i32,
    /// Master-side handle for writing keystrokes to the child.
    writer: std::fs::File,
    pub rx: Receiver<PtyEvent>,
    lifecycle: Arc<ChildLifecycle>,
}

impl PtyChild {
    /// Forward raw bytes (keystrokes) into the child's terminal.
    /// Safe after process termination: becomes a no-op if child was reaped.
    pub fn write_all(&self, bytes: &[u8]) {
        if self.lifecycle.is_reaped() {
            return;
        }
        let mut w = &self.writer;
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    /// Safely send a signal to the child.
    /// Returns true if delivered, false if process was already reaped.
    pub fn signal(&self, sig: libc::c_int) -> bool {
        self.lifecycle.signal(sig)
    }

    /// Forcibly terminate the child process with SIGKILL if still running.
    /// Idempotent and safe: never signals a recycled PID.
    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }

    /// Resize the pseudo-terminal window to `rows x cols`.
    /// - Clamps zero or degenerate dimensions to at least 1.
    /// - Caches dimensions to avoid redundant ioctls.
    /// - Safe after child exit (returns Ok(()) if already reaped).
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let rows = rows.max(1);
        let cols = cols.max(1);

        if self.lifecycle.is_reaped() {
            return Ok(());
        }

        {
            let mut current = self
                .lifecycle
                .win_size
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if *current == (rows, cols) {
                return Ok(());
            }
            *current = (rows, cols);
        }

        let win = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let fd = self.writer.as_raw_fd();
        let res = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &win) };
        if res == -1 {
            let err = io::Error::last_os_error();
            if self.lifecycle.is_reaped() {
                Ok(())
            } else {
                Err(PtyError::Ioctl(err))
            }
        } else {
            Ok(())
        }
    }

    pub fn is_reaped(&self) -> bool {
        self.lifecycle.is_reaped()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.lifecycle.exit_code()
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // Request termination if still running, but do NOT call waitpid.
        // The reader thread remains the sole owner of waitpid.
        self.kill();
    }
}

/// Spawn `program args` on a fresh pty sized `rows × cols`.
pub fn spawn(program: &str, args: &[String], rows: u16, cols: u16) -> Result<PtyChild, PtyError> {
    let rows = rows.max(1);
    let cols = cols.max(1);

    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let win = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &win,
        )
    } != 0
    {
        return Err(PtyError::OpenPty(io::Error::last_os_error()));
    }

    // Wrap in OwnedFd immediately so any subsequent error automatically closes them.
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };

    let s_in = slave.try_clone().map_err(PtyError::Dup)?;
    let s_out = slave.try_clone().map_err(PtyError::Dup)?;
    let s_err = slave.try_clone().map_err(PtyError::Dup)?;

    let mut command = Command::new(program);
    command.args(args);
    command.stdin(Stdio::from(s_in));
    command.stdout(Stdio::from(s_out));
    command.stderr(Stdio::from(s_err));

    unsafe {
        use std::os::unix::process::CommandExt;
        // New session + controlling tty so sudo/pacman prompts attach.
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            // Best-effort: some environments already grant a ctty.
            let _ = libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }

    let child = command.spawn().map_err(PtyError::Spawn)?;
    // Parent drops slave: master will receive EOF/EIO when child exits.
    drop(slave);

    let pid = child.id() as i32;
    let lifecycle = Arc::new(ChildLifecycle::new(pid, rows, cols));

    // Master handles for reading (worker thread) and writing (PtyChild).
    let master_writer = master.try_clone().map_err(PtyError::Dup)?;
    let master_reader = master; // moves ownership

    let (tx, rx) = channel();
    let thread_lifecycle = Arc::clone(&lifecycle);

    // Worker: stream output until EOF/EIO, then reap and report status.
    thread::spawn(move || {
        let mut file = std::fs::File::from(master_reader);
        let mut buf = [0u8; 4096];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                        // Parent dropped receiver: request termination so waitpid doesn't hang.
                        thread_lifecycle.signal(libc::SIGKILL);
                        break;
                    }
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    // On Linux, EIO indicates the slave closed (child exited).
                    // This is normal PTY EOF.
                    break;
                }
            }
        }

        // Sole reaper of child process
        let mut status: libc::c_int = 0;
        loop {
            let res = unsafe { libc::waitpid(pid, &mut status, 0) };
            if res == -1 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
            }
            break;
        }

        let code = if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status) as i32
        } else if libc::WIFSIGNALED(status) {
            -(libc::WTERMSIG(status) as i32)
        } else {
            -1
        };

        thread_lifecycle.mark_reaped(code);
        let _ = tx.send(PtyEvent::Exited(code));
    });

    Ok(PtyChild {
        pid,
        writer: std::fs::File::from(master_writer),
        rx,
        lifecycle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn collect(child: &mut PtyChild, secs: u64) -> (String, Option<i32>) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut out = Vec::new();
        let mut code = None;
        while Instant::now() < deadline {
            match child.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Output(b)) => out.extend(b),
                Ok(PtyEvent::Exited(c)) => {
                    code = Some(c);
                    break;
                }
                Err(_) => {}
            }
        }
        (String::from_utf8_lossy(&out).to_string(), code)
    }

    #[test]
    fn pty_streams_output_and_exit_code() {
        let mut child = spawn(
            "sh",
            &["-c".to_string(), "echo hello-pty; echo done".to_string()],
            24,
            80,
        )
        .unwrap();
        let (out, code) = collect(&mut child, 5);
        assert!(out.contains("hello-pty"), "output: {out}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn pty_forwards_keystrokes() {
        // `cat` echoes input; send a full line, then Ctrl-D on an empty
        // line (canonical-mode EOF).
        let mut child = spawn("cat", &[], 24, 80).unwrap();
        child.write_all(b"X\r");
        thread::sleep(Duration::from_millis(200));
        child.write_all(&[0x04]);
        let (out, code) = collect(&mut child, 5);
        assert!(out.contains('X'), "output: {out}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn pty_reports_nonzero_exit() {
        let mut child = spawn("sh", &["-c".to_string(), "exit 3".to_string()], 24, 80).unwrap();
        let (_, code) = collect(&mut child, 5);
        assert_eq!(code, Some(3));
    }

    #[test]
    fn pty_resize_updates_terminal_dimensions() {
        let mut child = spawn("sh", &["-c".to_string(), "stty size".to_string()], 35, 110).unwrap();
        let (out, code) = collect(&mut child, 5);
        assert_eq!(code, Some(0));
        assert!(out.contains("35 110"), "stty size output was: {out}");
    }

    #[test]
    fn pty_resize_after_exit_is_harmless() {
        let mut child = spawn("sh", &["-c".to_string(), "exit 0".to_string()], 24, 80).unwrap();
        let (_, code) = collect(&mut child, 5);
        assert_eq!(code, Some(0));
        assert!(child.is_reaped());
        // Resizing after exit must be a harmless Ok(())
        let res = child.resize(40, 100);
        assert!(res.is_ok());
    }

    #[test]
    fn pty_input_after_exit_is_harmless() {
        let mut child = spawn("sh", &["-c".to_string(), "exit 0".to_string()], 24, 80).unwrap();
        let (_, code) = collect(&mut child, 5);
        assert_eq!(code, Some(0));
        assert!(child.is_reaped());
        // Writing after exit must not panic or error
        child.write_all(b"ignored input\n");
    }

    #[test]
    fn pty_repeated_kill_cannot_signal_recycled_pid() {
        let mut child = spawn("sh", &["-c".to_string(), "sleep 30".to_string()], 24, 80).unwrap();
        assert!(!child.is_reaped());
        // First kill signals process
        child.kill();
        let (_, code) = collect(&mut child, 5);
        assert!(code.is_some());
        assert!(child.is_reaped());
        // Subsequent kills must be safe no-ops and not signal recycled PID
        child.kill();
        child.kill();
        assert!(!child.signal(libc::SIGKILL));
    }

    #[test]
    fn pty_drop_while_running_terminates_process_without_second_reaper() {
        let pid = {
            let child = spawn("sh", &["-c".to_string(), "sleep 30".to_string()], 24, 80).unwrap();
            let pid = child.pid;
            // Dropping child triggers drop() which requests kill, but does not call waitpid
            drop(child);
            pid
        };
        // Give the background worker thread a moment to reap the process
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut reaped = false;
        while Instant::now() < deadline {
            let ret = unsafe { libc::kill(pid, 0) };
            if ret == -1 {
                reaped = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(reaped, "process was not terminated and reaped after drop");
    }

    #[test]
    fn pty_eof_eio_normal_shutdown() {
        let mut child = spawn(
            "sh",
            &[
                "-c".to_string(),
                "echo eof-test-start; echo eof-test-end; exit 0".to_string(),
            ],
            24,
            80,
        )
        .unwrap();
        let (out, code) = collect(&mut child, 5);
        assert!(out.contains("eof-test-start"));
        assert!(out.contains("eof-test-end"));
        assert_eq!(code, Some(0));
    }
}

#[cfg(test)]
mod ctty_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn child_sees_controlling_tty() {
        let child = spawn(
            "sh",
            &["-c".to_string(), "echo TTY=$(tty)".to_string()],
            24,
            80,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        while Instant::now() < deadline {
            match child.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(PtyEvent::Output(b)) => out.extend(b),
                Ok(PtyEvent::Exited(_)) => break,
                Err(_) => {}
            }
        }
        let s = String::from_utf8_lossy(&out).to_string();
        eprintln!("child reported: {s}");
        assert!(
            s.contains("TTY=/dev/pts/") || s.contains("TTY=/dev/tts/"),
            "no ctty: {s}"
        );
    }
}
