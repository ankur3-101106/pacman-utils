// ──────────────────────────────────────────────────────────────────────
// cmd.rs — Centralized Command Execution Layer
//
// Screens specify WHAT command should run via CommandSpec, while
// CommandEngine controls HOW it runs (Capture vs Interactive PTY).
// ──────────────────────────────────────────────────────────────────────

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::pty::{self, PtyChild, PtyEvent};

/// Execution mode for a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandMode {
    Capture,
    Interactive,
}

/// Specification for a command to execute. UI rendering concerns
/// are deliberately kept out of this structure.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub mode: CommandMode,
    pub stdin_file: Option<PathBuf>,
    pub stdin_data: Option<Vec<u8>>,
    /// Routing key delivered back via [`Screen::on_ext_done`].
    pub tag: String,
    /// Note/title describing the command.
    pub note: String,
    /// Optional result feedback displayed when finished.
    pub ok_msg: Option<String>,
    pub fail_msg: Option<String>,
    pub done_log: Option<String>,
    /// Whether command lifecycle should be written to activity log.
    pub log: bool,
}

/// Backward compatibility alias for the previous ExtCmd type.
pub type ExtCmd = CommandSpec;

impl CommandSpec {
    /// Create a new interactive command spec, matching previous ExtCmd::new behavior.
    pub fn new(tag: &str, program: &str, args: &[String]) -> Self {
        Self {
            tag: tag.to_string(),
            program: program.to_string(),
            args: args.to_vec(),
            mode: CommandMode::Interactive,
            stdin_file: None,
            stdin_data: None,
            note: format!("{program} {}", args.join(" ")),
            ok_msg: None,
            fail_msg: None,
            done_log: None,
            log: true,
        }
    }

    /// Construct a Capture-mode command spec (logging disabled by default for routine queries).
    pub fn capture(program: impl Into<String>, args: &[&str]) -> Self {
        let p = program.into();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let note = if a.is_empty() {
            p.clone()
        } else {
            format!("{p} {}", a.join(" "))
        };
        Self {
            tag: String::new(),
            program: p,
            args: a,
            mode: CommandMode::Capture,
            stdin_file: None,
            stdin_data: None,
            note,
            ok_msg: None,
            fail_msg: None,
            done_log: None,
            log: false,
        }
    }

    /// Construct a Capture-mode command spec from owned strings (logging disabled by default for routine queries).
    pub fn capture_owned(program: impl Into<String>, args: &[String]) -> Self {
        let p = program.into();
        let a = args.to_vec();
        let note = if a.is_empty() {
            p.clone()
        } else {
            format!("{p} {}", a.join(" "))
        };
        Self {
            tag: String::new(),
            program: p,
            args: a,
            mode: CommandMode::Capture,
            stdin_file: None,
            stdin_data: None,
            note,
            ok_msg: None,
            fail_msg: None,
            done_log: None,
            log: false,
        }
    }

    /// Construct an Interactive-mode command spec.
    pub fn interactive(tag: impl Into<String>, program: impl Into<String>, args: &[&str]) -> Self {
        let t = tag.into();
        let p = program.into();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let note = if a.is_empty() {
            p.clone()
        } else {
            format!("{p} {}", a.join(" "))
        };
        Self {
            tag: t,
            program: p,
            args: a,
            mode: CommandMode::Interactive,
            stdin_file: None,
            stdin_data: None,
            note,
            ok_msg: None,
            fail_msg: None,
            done_log: None,
            log: true,
        }
    }

    pub fn mode(mut self, mode: CommandMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = tag.into();
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn stdin_file(mut self, path: PathBuf) -> Self {
        self.stdin_file = Some(path);
        self
    }

    pub fn stdin_data(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin_data = Some(data.into());
        self
    }

    pub fn log(mut self, enabled: bool) -> Self {
        self.log = enabled;
        self
    }

    /// Result feedback for commands that outlive their originating screen.
    pub fn result(
        mut self,
        ok_msg: impl Into<String>,
        fail_msg: impl Into<String>,
        done_log: impl Into<String>,
    ) -> Self {
        self.ok_msg = Some(ok_msg.into());
        self.fail_msg = Some(fail_msg.into());
        self.done_log = Some(done_log.into());
        self
    }

    pub fn display_cmd(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

// ── Structured Command Result and Error Model ───────────────────────

/// Exit status of an executed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Success,
    Exited(i32),
    Signaled(i32),
}

impl CommandStatus {
    pub fn is_success(&self) -> bool {
        matches!(self, CommandStatus::Success)
    }

    pub fn code(&self) -> Option<i32> {
        match self {
            CommandStatus::Success => Some(0),
            CommandStatus::Exited(c) => Some(*c),
            CommandStatus::Signaled(_) => None,
        }
    }

    pub fn signal(&self) -> Option<i32> {
        match self {
            CommandStatus::Signaled(s) => Some(*s),
            _ => None,
        }
    }
}

impl std::fmt::Display for CommandStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandStatus::Success => write!(f, "success"),
            CommandStatus::Exited(c) => write!(f, "exit code {c}"),
            CommandStatus::Signaled(s) => write!(f, "signal {s}"),
        }
    }
}

/// Output and status produced by a successfully launched command.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub status: CommandStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandResult {
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    pub fn stdout_trimmed(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim_end().to_string()
    }

    pub fn stderr_trimmed(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim_end().to_string()
    }

    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }

    /// Check if the result was successful; return Err(status) if non-zero or signaled.
    pub fn check_success(&self) -> Result<(), CommandStatus> {
        if self.is_success() {
            Ok(())
        } else {
            Err(self.status)
        }
    }
}

/// Failure to execute a command (spawn or transport failure).
/// Note: A normal non-zero exit from the child process is represented
/// as Ok(CommandResult { status: Exited(code), .. }), NOT a CommandError.
#[derive(Debug)]
pub enum CommandError {
    SpawnFailure(io::Error),
    PtyFailure(io::Error),
    IoFailure(io::Error),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::SpawnFailure(e) => write!(f, "Failed to spawn process: {e}"),
            CommandError::PtyFailure(e) => write!(f, "PTY error: {e}"),
            CommandError::IoFailure(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandError::SpawnFailure(e) => Some(e),
            CommandError::PtyFailure(e) => Some(e),
            CommandError::IoFailure(e) => Some(e),
        }
    }
}

// ── Interactive Child Wrapper ───────────────────────────────────────

pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Wrapper around a running PTY child process. Exposes stream reception,
/// keystroke forwarding, termination, and exit translation.
pub struct InteractiveChild {
    pty: PtyChild,
    spec: CommandSpec,
    logger: Option<LogFn>,
    logged_exit: std::sync::atomic::AtomicBool,
}

impl InteractiveChild {
    /// Write raw keystrokes/bytes into the child's controlling terminal.
    /// Safe after process termination: no-op if child has exited.
    pub fn write_input(&self, bytes: &[u8]) {
        self.pty.write_all(bytes);
    }

    /// Safely signal the child process.
    /// Returns true if delivered, false if process was already reaped.
    pub fn signal(&self, sig: libc::c_int) -> bool {
        self.pty.signal(sig)
    }

    /// Forcibly terminate the child process with SIGKILL if still running.
    /// Idempotent and safe: never signals a recycled PID.
    pub fn kill(&self) {
        self.pty.kill();
    }

    /// Resize the pseudo-terminal window to `rows x cols`.
    /// Safe after child exit.
    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        self.pty.resize(rows, cols).map_err(Into::into)
    }

    /// Non-blocking poll for child events (output chunks or exit code).
    pub fn try_recv(&self) -> Result<PtyEvent, std::sync::mpsc::TryRecvError> {
        self.pty.rx.try_recv()
    }

    /// Blocking poll with timeout for child events.
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<PtyEvent, std::sync::mpsc::RecvTimeoutError> {
        self.pty.rx.recv_timeout(timeout)
    }

    pub fn pid(&self) -> i32 {
        self.pty.pid
    }

    pub fn is_reaped(&self) -> bool {
        self.pty.is_reaped()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.pty.exit_code()
    }

    pub fn spec(&self) -> &CommandSpec {
        &self.spec
    }

    /// Convert raw PTY exit code to structured CommandStatus.
    pub fn exit_status_from_code(code: i32) -> CommandStatus {
        if code == 0 {
            CommandStatus::Success
        } else if code > 0 {
            CommandStatus::Exited(code)
        } else {
            CommandStatus::Signaled(-code)
        }
    }

    /// Log command exit if logging is enabled on this spec.
    /// Guaranteed to log at most once per InteractiveChild.
    pub fn log_exit(&self, status: CommandStatus) {
        if self
            .logged_exit
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        if let Some(logger) = &self.logger {
            if self.spec.log {
                let msg = match status {
                    CommandStatus::Success => {
                        format!("CMD: SUCCESS (interactive) {}", self.spec.display_cmd())
                    }
                    CommandStatus::Exited(c) => format!(
                        "CMD: FAILED (interactive exit {c}) {}",
                        self.spec.display_cmd()
                    ),
                    CommandStatus::Signaled(s) => format!(
                        "CMD: SIGNAL (interactive signal {s}) {}",
                        self.spec.display_cmd()
                    ),
                };
                logger(&msg);
            }
        }
    }
}

// ── Centralized Command Engine ──────────────────────────────────────

/// Centralized execution engine for external commands.
#[derive(Clone)]
pub struct CommandEngine {
    logger: Option<LogFn>,
}

impl Default for CommandEngine {
    fn default() -> Self {
        Self {
            logger: Some(Arc::new(|msg| {
                crate::settings::log_if_enabled(msg);
            })),
        }
    }
}

impl CommandEngine {
    /// Create a new command engine with no logger.
    pub fn new() -> Self {
        Self { logger: None }
    }

    /// Attach a custom logging callback.
    pub fn with_logger<F>(mut self, logger: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.logger = Some(Arc::new(logger));
        self
    }

    /// Log an informational lifecycle message through the engine's logger.
    pub fn log(&self, msg: &str) {
        if let Some(logger) = &self.logger {
            logger(msg);
        }
    }

    /// Execute a command in Capture mode. Captures stdout and stderr,
    /// returning Ok(CommandResult) for both 0 and non-zero exit codes.
    pub fn run_capture(&self, spec: &CommandSpec) -> Result<CommandResult, CommandError> {
        if spec.log {
            self.log(&format!("CMD: START {}", spec.display_cmd()));
        }

        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);

        // Setup stdin: file path, direct byte buffer, or null.
        if let Some(stdin_path) = &spec.stdin_file {
            let file = std::fs::File::open(stdin_path).map_err(CommandError::IoFailure)?;
            cmd.stdin(Stdio::from(file));
        } else if spec.stdin_data.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(CommandError::SpawnFailure)?;

        // Write inline stdin if provided. Passwords / sensitive inputs
        // are written directly to pipe and never logged.
        if let Some(data) = &spec.stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(data);
            }
        }

        let output = child
            .wait_with_output()
            .map_err(CommandError::SpawnFailure)?;

        let status = if output.status.success() {
            CommandStatus::Success
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(code) = output.status.code() {
                    CommandStatus::Exited(code)
                } else if let Some(sig) = output.status.signal() {
                    CommandStatus::Signaled(sig)
                } else {
                    CommandStatus::Exited(-1)
                }
            }
            #[cfg(not(unix))]
            {
                CommandStatus::Exited(output.status.code().unwrap_or(-1))
            }
        };

        if spec.log {
            match status {
                CommandStatus::Success => {
                    self.log(&format!("CMD: SUCCESS {}", spec.display_cmd()));
                }
                CommandStatus::Exited(code) => {
                    self.log(&format!("CMD: FAILED (exit {code}) {}", spec.display_cmd()));
                }
                CommandStatus::Signaled(sig) => {
                    self.log(&format!("CMD: SIGNAL ({sig}) {}", spec.display_cmd()));
                }
            }
        }

        Ok(CommandResult {
            status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// Spawn an interactive command on a PTY with dimensions rows x cols.
    pub fn spawn_interactive(
        &self,
        spec: &CommandSpec,
        rows: u16,
        cols: u16,
    ) -> Result<InteractiveChild, CommandError> {
        if spec.log {
            self.log(&format!("CMD: START (interactive) {}", spec.display_cmd()));
        }

        let pty_child = pty::spawn(&spec.program, &spec.args, rows.max(1), cols.max(1)).map_err(
            |e| match e {
                pty::PtyError::Spawn(err) => CommandError::SpawnFailure(err),
                pty::PtyError::OpenPty(err)
                | pty::PtyError::Dup(err)
                | pty::PtyError::Ioctl(err) => CommandError::PtyFailure(err),
            },
        )?;

        Ok(InteractiveChild {
            pty: pty_child,
            spec: spec.clone(),
            logger: self.logger.clone(),
            logged_exit: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Run an interactive command to completion on a PTY. Useful for headless/unit testing.
    pub fn run_interactive(
        &self,
        spec: &CommandSpec,
        rows: u16,
        cols: u16,
    ) -> Result<CommandResult, CommandError> {
        let child = self.spawn_interactive(spec, rows, cols)?;

        if let Some(stdin_path) = &spec.stdin_file {
            if let Ok(data) = std::fs::read(stdin_path) {
                child.write_input(&data);
                child.write_input(b"\n");
            }
        } else if let Some(data) = &spec.stdin_data {
            child.write_input(data);
            child.write_input(b"\n");
        }

        let mut stdout = Vec::new();
        let mut status = CommandStatus::Exited(-1);

        loop {
            match child.pty.rx.recv() {
                Ok(PtyEvent::Output(bytes)) => {
                    stdout.extend(bytes);
                }
                Ok(PtyEvent::Exited(code)) => {
                    status = InteractiveChild::exit_status_from_code(code);
                    child.log_exit(status);
                    break;
                }
                Err(_) => {
                    if let Some(code) = child.exit_code() {
                        status = InteractiveChild::exit_status_from_code(code);
                    }
                    child.log_exit(status);
                    break;
                }
            }
        }

        Ok(CommandResult {
            status,
            stdout,
            stderr: Vec::new(),
        })
    }

    /// Execute a command according to its mode.
    pub fn execute(&self, spec: &CommandSpec) -> Result<CommandResult, CommandError> {
        match spec.mode {
            CommandMode::Capture => self.run_capture(spec),
            CommandMode::Interactive => self.run_interactive(spec, 24, 80),
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // ── Capture Tests ──

    #[test]
    fn capture_successful_command() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("echo", &["archman-test-ok"]);
        let res = engine.run_capture(&spec).expect("spawn failed");
        assert!(res.is_success());
        assert_eq!(res.status, CommandStatus::Success);
        assert_eq!(res.stdout_trimmed(), "archman-test-ok");
    }

    #[test]
    fn capture_nonzero_exit() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("sh", &["-c", "exit 42"]);
        let res = engine.run_capture(&spec).expect("spawn failed");
        assert!(!res.is_success());
        assert_eq!(res.status, CommandStatus::Exited(42));
    }

    #[test]
    fn capture_missing_executable() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("non_existent_binary_xyz_archman", &[]);
        let err = engine.run_capture(&spec);
        assert!(matches!(err, Err(CommandError::SpawnFailure(_))));
    }

    #[test]
    fn capture_stdout_multiline() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("sh", &["-c", "echo line1; echo line2"]);
        let res = engine.run_capture(&spec).unwrap();
        let out = res.stdout_lossy();
        assert!(out.contains("line1"));
        assert!(out.contains("line2"));
    }

    #[test]
    fn capture_stderr() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("sh", &["-c", "echo error-msg >&2"]);
        let res = engine.run_capture(&spec).unwrap();
        assert_eq!(res.stderr_trimmed(), "error-msg");
    }

    #[test]
    fn capture_stdin_data() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("cat", &[]).stdin_data(b"hello from stdin".to_vec());
        let res = engine.run_capture(&spec).unwrap();
        assert_eq!(res.stdout_trimmed(), "hello from stdin");
    }

    // ── Interactive Tests ──

    #[test]
    fn interactive_pty_launch_and_output() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::interactive("test", "echo", &["hello-pty"]);
        let res = engine.run_interactive(&spec, 24, 80).unwrap();
        assert!(res.is_success());
        assert_eq!(res.status, CommandStatus::Success);
        assert!(res.stdout_lossy().contains("hello-pty"));
    }

    #[test]
    fn interactive_nonzero_exit() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::interactive("test", "sh", &["-c", "exit 5"]);
        let res = engine.run_interactive(&spec, 24, 80).unwrap();
        assert!(!res.is_success());
        assert_eq!(res.status, CommandStatus::Exited(5));
    }

    #[test]
    fn interactive_keyboard_input_forwarding() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::interactive("test", "cat", &[]);
        let child = engine.spawn_interactive(&spec, 24, 80).unwrap();

        // Feed input to cat
        child.write_input(b"key-stream\r");
        std::thread::sleep(Duration::from_millis(150));
        // Send EOF (Ctrl-D)
        child.write_input(&[0x04]);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        let mut exited = None;

        while Instant::now() < deadline {
            match child.recv_timeout(Duration::from_millis(50)) {
                Ok(PtyEvent::Output(bytes)) => out.extend(bytes),
                Ok(PtyEvent::Exited(code)) => {
                    exited = Some(InteractiveChild::exit_status_from_code(code));
                    break;
                }
                Err(_) => {}
            }
        }

        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("key-stream"), "output was: {out_str}");
        assert_eq!(exited, Some(CommandStatus::Success));
    }

    #[test]
    fn interactive_process_cancellation_kill() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::interactive("test", "sh", &["-c", "sleep 60"]);
        let child = engine.spawn_interactive(&spec, 24, 80).unwrap();

        // Terminate child
        child.kill();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = false;

        while Instant::now() < deadline {
            match child.recv_timeout(Duration::from_millis(50)) {
                Ok(PtyEvent::Exited(_)) => {
                    exited = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        assert!(exited, "Process was not killed in time");
    }

    #[test]
    fn interactive_ctrl_c_interrupt() {
        let engine = CommandEngine::new();
        // sh in interactive PTY handles Ctrl-C (0x03 byte) as SIGINT
        let spec = CommandSpec::interactive("test", "cat", &[]);
        let child = engine.spawn_interactive(&spec, 24, 80).unwrap();

        // Send Ctrl-C (0x03)
        child.write_input(&[0x03]);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = false;

        while Instant::now() < deadline {
            match child.recv_timeout(Duration::from_millis(50)) {
                Ok(PtyEvent::Exited(_)) => {
                    exited = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        assert!(exited, "Process did not exit on Ctrl-C");
    }

    #[test]
    fn interactive_exit_logged_exactly_once() {
        let logs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let logs_clone = Arc::clone(&logs);
        let engine = CommandEngine::new().with_logger(move |msg| {
            logs_clone.lock().unwrap().push(msg.to_string());
        });

        let spec = CommandSpec::interactive("test", "true", &[]).log(true);
        let child = engine.spawn_interactive(&spec, 24, 80).unwrap();

        // Calling log_exit multiple times must only invoke the logger once
        child.log_exit(CommandStatus::Success);
        child.log_exit(CommandStatus::Success);
        child.log_exit(CommandStatus::Exited(1));

        let recorded = logs.lock().unwrap().clone();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[0].contains("CMD: START"));
        assert!(recorded[1].contains("CMD: SUCCESS"));
    }

    #[test]
    fn interactive_resize_propagates() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::interactive("test", "cat", &[]);
        let child = engine.spawn_interactive(&spec, 24, 80).unwrap();

        let res = child.resize(30, 90);
        assert!(res.is_ok());

        child.kill();
    }

    #[test]
    fn capture_signaled_process() {
        let engine = CommandEngine::new();
        // Process self-terminates via SIGKILL (signal 9)
        let spec = CommandSpec::capture("sh", &["-c", "kill -9 $$"]);
        let res = engine.run_capture(&spec).expect("spawn should succeed");
        assert!(!res.is_success());
        assert_eq!(res.status.signal(), Some(9));
    }

    #[test]
    fn capture_empty_output() {
        let engine = CommandEngine::new();
        let spec = CommandSpec::capture("true", &[]);
        let res = engine.run_capture(&spec).expect("spawn should succeed");
        assert!(res.is_success());
        assert!(res.stdout.is_empty());
        assert!(res.stderr.is_empty());
    }

    #[test]
    fn capture_large_output() {
        let engine = CommandEngine::new();
        // Generate 128 KiB of data to verify pipes do not deadlock
        let spec = CommandSpec::capture("sh", &["-c", "head -c 131072 /dev/zero | tr '\\0' 'X'"]);
        let res = engine.run_capture(&spec).expect("spawn should succeed");
        assert!(res.is_success());
        assert_eq!(res.stdout.len(), 131072);
    }
}
