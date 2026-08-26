// ──────────────────────────────────────────────────────────────────────
// pty.rs — Run external commands on a pseudo-terminal
//
// The child keeps a real controlling TTY (so sudo password prompts and
// pacman's [Y/n] work normally) while every byte of its output is
// streamed back to the TUI over a channel. Keystrokes can be written
// into the master side to interact with the child from inside the pane.
// ──────────────────────────────────────────────────────────────────────

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

/// Events streamed from the worker thread.
pub enum PtyEvent {
    /// Raw bytes of child output.
    Output(Vec<u8>),
    /// Child exited; payload is the exit code (negative if killed).
    Exited(i32),
}

pub struct PtyChild {
    pub pid: i32,
    /// Master-side handle for writing keystrokes to the child.
    writer: std::fs::File,
    pub rx: Receiver<PtyEvent>,
}

impl PtyChild {
    /// Forward raw bytes (keystrokes) into the child's terminal.
    pub fn write_all(&self, bytes: &[u8]) {
        // Ignore errors: after the child exits the write side is gone.
        let mut w = &self.writer;
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    /// Kill the child if it is still running (used when closing early).
    pub fn kill(&self) {
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
    }
}

extern "C" fn dup_raw(fd: RawFd) -> RawFd {
    unsafe { libc::dup(fd) }
}

/// Spawn `program args` on a fresh pty sized `rows × cols`.
pub fn spawn(program: &str, args: &[String], rows: u16, cols: u16) -> std::io::Result<PtyChild> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let win = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &win) }
        != 0
    {
        return Err(std::io::Error::last_os_error());
    }

    // Duplicate the slave for each std fd; originals closed below.
    let s_in = dup_raw(slave);
    let s_out = dup_raw(slave);
    let s_err = dup_raw(slave);

    let mut command = Command::new(program);
    command.args(args);
    command.stdin(unsafe { Stdio::from_raw_fd(s_in) });
    command.stdout(unsafe { Stdio::from_raw_fd(s_out) });
    command.stderr(unsafe { Stdio::from_raw_fd(s_err) });

    unsafe {
        use std::os::unix::process::CommandExt;
        // New session + controlling tty so sudo/pacman prompts attach.
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // Best-effort: some environments already grant a ctty.
            let _ = libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }

    let child = command.spawn()?;
    // Parent must release the slave or master never sees EOF.
    unsafe { libc::close(slave) };

    let pid = child.id() as i32;
    let (tx, rx) = channel();

    // Separate master handles for reading (worker) and writing (parent).
    let rd_fd = dup_raw(master);
    let wr_fd = dup_raw(master);
    unsafe { libc::close(master) };

    // Worker: stream output until EOF/EIO, then reap and report status.
    thread::spawn(move || {
        let mut file = unsafe { std::fs::File::from_raw_fd(rd_fd) };
        let mut buf = [0u8; 4096];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    // EIO on Linux means the slave side closed (child gone).
                    break;
                }
            }
        }
        let _ = file.read(&mut buf); // final drain attempt

        let mut status: libc::c_int = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        let code = if status & 0x7f == 0 {
            ((status >> 8) & 0xff) as i32
        } else {
            -(status & 0x7f) as i32 // terminated by signal −N
        };
        let _ = tx.send(PtyEvent::Exited(code));
    });

    Ok(PtyChild {
        pid,
        writer: unsafe { std::fs::File::from_raw_fd(wr_fd) },
        rx,
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
        let mut child =
            spawn("sh", &["-c".to_string(), "echo hello-pty; echo done".to_string()], 24, 80)
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
        let mut child =
            spawn("sh", &["-c".to_string(), "exit 3".to_string()], 24, 80).unwrap();
        let (_, code) = collect(&mut child, 5);
        assert_eq!(code, Some(3));
    }
}

#[cfg(test)]
mod ctty_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn child_sees_controlling_tty() {
        let child = spawn("sh", &["-c".to_string(), "echo TTY=$(tty)".to_string()], 24, 80).unwrap();
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
        assert!(s.contains("TTY=/dev/pts/") || s.contains("TTY=/dev/tts/"), "no ctty: {s}");
    }
}
