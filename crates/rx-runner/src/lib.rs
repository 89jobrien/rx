//! Runtime-neutral child-process execution for polling event loops.
//!
//! `rx-runner` tags stdout/stderr lines, bounds retained output, exposes
//! non-blocking polling and cancellation, and reaps unfinished children on
//! drop. It uses only the Rust standard library.
//!
//! ```no_run
//! use std::time::Duration;
//! use rx_runner::CommandSpec;
//!
//! # fn main() -> std::io::Result<()> {
//! let mut process = CommandSpec::new("rustc").arg("--version").spawn()?;
//! loop {
//!     let update = process.poll()?;
//!     for line in update.lines {
//!         println!("{:?}: {}", line.stream, line.text);
//!     }
//!     if update.exit.is_some() {
//!         break;
//!     }
//!     std::thread::sleep(Duration::from_millis(20));
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Default number of output lines retained between polls.
pub const DEFAULT_OUTPUT_CAPACITY: usize = 1_024;
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_millis(50);

/// A program invocation with separate arguments and process options.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandSpec {
    program: OsString,
    arguments: Vec<OsString>,
    working_directory: Option<PathBuf>,
    output_capacity: usize,
}

impl CommandSpec {
    /// Create a specification for `program`.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            working_directory: None,
            output_capacity: DEFAULT_OUTPUT_CAPACITY,
        }
    }

    /// Append one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.arguments.push(arg.into());
        self
    }

    /// Append multiple arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.arguments.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the child working directory.
    #[must_use]
    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Set the maximum number of unread lines retained between polls.
    ///
    /// A capacity of zero discards all lines while still counting them in
    /// [`ProcessUpdate::dropped_lines`].
    #[must_use]
    pub fn output_capacity(mut self, lines: usize) -> Self {
        self.output_capacity = lines;
        self
    }

    /// Return the executable name or path.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Iterate over the separate process arguments.
    pub fn arguments(&self) -> impl Iterator<Item = &OsStr> {
        self.arguments.iter().map(OsString::as_os_str)
    }

    /// Return the configured child working directory.
    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    /// Spawn the child with piped stdout and stderr.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the process or output-reader threads cannot
    /// be spawned.
    pub fn spawn(&self) -> io::Result<RunningProcess> {
        RunningProcess::spawn(self)
    }
}

/// Child output source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// One decoded child-output line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputLine {
    /// Stream that produced the line.
    pub stream: OutputStream,
    /// UTF-8 text, with invalid bytes replaced and line endings removed.
    pub text: String,
}

/// Terminal child status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessExit {
    /// Platform exit code, or `None` when terminated by a signal.
    pub code: Option<i32>,
    /// Whether the platform exit status reports success.
    pub success: bool,
    /// Wall-clock duration since spawn.
    pub elapsed: Duration,
}

/// Output and optional terminal status produced by one poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessUpdate {
    /// Lines retained since the previous poll.
    pub lines: Vec<OutputLine>,
    /// Lines dropped because the configured capacity was exceeded.
    pub dropped_lines: usize,
    /// Terminal status, returned exactly once.
    pub exit: Option<ProcessExit>,
}

#[derive(Debug)]
struct OutputBuffer {
    capacity: usize,
    lines: VecDeque<OutputLine>,
    dropped_lines: usize,
}

impl OutputBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: VecDeque::with_capacity(capacity),
            dropped_lines: 0,
        }
    }

    fn push(&mut self, line: OutputLine) {
        if self.capacity == 0 {
            self.dropped_lines += 1;
            return;
        }
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
            self.dropped_lines += 1;
        }
        self.lines.push_back(line);
    }

    fn drain(&mut self) -> (Vec<OutputLine>, usize) {
        let lines = self.lines.drain(..).collect();
        let dropped_lines = std::mem::take(&mut self.dropped_lines);
        (lines, dropped_lines)
    }
}

/// A running child process with non-blocking output/status polling.
pub struct RunningProcess {
    child: Child,
    output: Arc<Mutex<OutputBuffer>>,
    readers: Vec<JoinHandle<()>>,
    started_at: Instant,
    exit: Option<ProcessExit>,
    exit_detected_at: Option<Instant>,
    exit_reported: bool,
}

impl fmt::Debug for RunningProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunningProcess")
            .field("id", &self.child.id())
            .field("finished", &self.exit.is_some())
            .finish()
    }
}

impl RunningProcess {
    fn spawn(spec: &CommandSpec) -> io::Result<Self> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = &spec.working_directory {
            command.current_dir(path);
        }

        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("child stderr was not piped"))?;
        let output = Arc::new(Mutex::new(OutputBuffer::new(spec.output_capacity)));

        let stdout_reader = match spawn_reader(stdout, OutputStream::Stdout, Arc::clone(&output)) {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let stderr_reader = match spawn_reader(stderr, OutputStream::Stderr, Arc::clone(&output)) {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                return Err(error);
            }
        };

        Ok(Self {
            child,
            output,
            readers: vec![stdout_reader, stderr_reader],
            started_at: Instant::now(),
            exit: None,
            exit_detected_at: None,
            exit_reported: false,
        })
    }

    /// Return the platform process identifier.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Drain retained output and check for process completion without waiting.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when process status cannot be read or the shared
    /// output buffer is poisoned.
    pub fn poll(&mut self) -> io::Result<ProcessUpdate> {
        if self.exit.is_none()
            && let Some(status) = self.child.try_wait()?
        {
            self.finish(status);
        }

        let readers_finished = self.readers.iter().all(JoinHandle::is_finished);
        if readers_finished {
            self.join_readers();
        }
        let (lines, dropped_lines) = self.output()?.drain();
        let drain_grace_elapsed = self
            .exit_detected_at
            .is_some_and(|detected_at| detected_at.elapsed() >= OUTPUT_DRAIN_GRACE);
        let exit = if self.exit_reported || !readers_finished && !drain_grace_elapsed {
            None
        } else {
            self.exit_reported = self.exit.is_some();
            self.exit
        };

        Ok(ProcessUpdate {
            lines,
            dropped_lines,
            exit,
        })
    }

    /// Terminate and reap the child, returning its terminal status.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when process status cannot be read or the child
    /// cannot be terminated and reaped.
    pub fn cancel(&mut self) -> io::Result<ProcessExit> {
        if let Some(exit) = self.exit {
            self.exit_reported = true;
            return Ok(exit);
        }

        let status = match self.child.try_wait()? {
            Some(status) => status,
            None => {
                self.child.kill()?;
                self.child.wait()?
            }
        };
        self.finish(status);
        if self.readers.iter().all(JoinHandle::is_finished) {
            self.join_readers();
        }
        self.exit_reported = true;
        self.exit
            .ok_or_else(|| io::Error::other("cancelled process has no exit status"))
    }

    fn finish(&mut self, status: ExitStatus) {
        self.exit_detected_at = Some(Instant::now());
        self.exit = Some(ProcessExit {
            code: status.code(),
            success: status.success(),
            elapsed: self.started_at.elapsed(),
        });
    }

    fn join_readers(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }

    fn output(&self) -> io::Result<MutexGuard<'_, OutputBuffer>> {
        self.output
            .lock()
            .map_err(|_| io::Error::other("process output buffer is poisoned"))
    }
}

impl Drop for RunningProcess {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if self.readers.iter().all(JoinHandle::is_finished) {
            self.join_readers();
        }
    }
}

fn spawn_reader<R>(
    reader: R,
    stream: OutputStream,
    output: Arc<Mutex<OutputBuffer>>,
) -> io::Result<JoinHandle<()>>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(format!("rx-runner-{stream:?}"))
        .spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                let count = match reader.read_until(b'\n', &mut bytes) {
                    Ok(count) => count,
                    Err(_) => break,
                };
                if count == 0 {
                    break;
                }
                while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                    bytes.pop();
                }
                let line = OutputLine {
                    stream,
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                };
                let Ok(mut buffer) = output.lock() else {
                    break;
                };
                buffer.push(line);
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn fixture(script: &str) -> CommandSpec {
        CommandSpec::new("sh").args(["-c", script])
    }

    #[cfg(windows)]
    fn fixture(script: &str) -> CommandSpec {
        CommandSpec::new("cmd").args(["/C", script])
    }

    fn collect(mut process: RunningProcess) -> io::Result<ProcessUpdate> {
        let mut lines = Vec::new();
        let mut dropped_lines = 0;
        for _ in 0..200 {
            let update = process.poll()?;
            lines.extend(update.lines);
            dropped_lines += update.dropped_lines;
            if update.exit.is_some() {
                return Ok(ProcessUpdate {
                    lines,
                    dropped_lines,
                    exit: update.exit,
                });
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "fixture process did not exit",
        ))
    }

    #[test]
    fn command_spec_preserves_separate_arguments_and_directory() {
        let spec = CommandSpec::new("tool")
            .args(["one", "two words"])
            .current_dir("workspace")
            .output_capacity(12);

        assert_eq!(spec.program(), OsStr::new("tool"));
        assert_eq!(
            spec.arguments().collect::<Vec<_>>(),
            vec![OsStr::new("one"), OsStr::new("two words")]
        );
        assert_eq!(spec.working_directory(), Some(Path::new("workspace")));
        assert_eq!(spec.output_capacity, 12);
    }

    #[test]
    fn captures_stdout_and_stderr_lines() {
        #[cfg(unix)]
        let spec = fixture("printf 'out\\n'; printf 'err\\n' >&2");
        #[cfg(windows)]
        let spec = fixture("echo out & echo err 1>&2");

        let update = collect(spec.spawn().expect("spawn fixture")).expect("collect fixture");

        assert!(update.exit.is_some_and(|exit| exit.success));
        assert!(update.lines.contains(&OutputLine {
            stream: OutputStream::Stdout,
            text: "out".to_string(),
        }));
        assert!(update.lines.contains(&OutputLine {
            stream: OutputStream::Stderr,
            text: "err".to_string(),
        }));
    }

    #[test]
    fn bounded_output_reports_dropped_lines() {
        #[cfg(unix)]
        let spec = fixture("printf '1\\n2\\n3\\n4\\n'").output_capacity(2);
        #[cfg(windows)]
        let spec = fixture("(echo 1 & echo 2 & echo 3 & echo 4)").output_capacity(2);

        let update = collect(spec.spawn().expect("spawn fixture")).expect("collect fixture");

        assert_eq!(update.lines.len(), 2);
        assert_eq!(update.dropped_lines, 2);
    }

    #[cfg(unix)]
    #[test]
    fn cancel_terminates_and_reaps_running_process() {
        let mut process = fixture("exec sleep 30").spawn().expect("spawn fixture");

        let exit = process.cancel().expect("cancel fixture");

        assert!(!exit.success);
        assert!(
            process
                .poll()
                .expect("poll cancelled process")
                .exit
                .is_none()
        );
    }

    #[test]
    fn terminal_exit_is_reported_once() {
        #[cfg(unix)]
        let spec = fixture("exit 7");
        #[cfg(windows)]
        let spec = fixture("exit /B 7");
        let mut process = spec.spawn().expect("spawn fixture");

        let update = collect_until_exit(&mut process).expect("collect terminal exit");

        assert_eq!(update.exit.and_then(|exit| exit.code), Some(7));
        assert!(process.poll().expect("poll after exit").exit.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn exit_is_reported_when_descendant_keeps_output_pipe_open() {
        let mut process = fixture("(sleep 1) &").spawn().expect("spawn fixture");

        for _ in 0..50 {
            if process.poll().expect("poll fixture").exit.is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }

        panic!("direct child exit was hidden by a descendant output pipe");
    }

    fn collect_until_exit(process: &mut RunningProcess) -> io::Result<ProcessUpdate> {
        for _ in 0..200 {
            let update = process.poll()?;
            if update.exit.is_some() {
                return Ok(update);
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "fixture process did not exit",
        ))
    }
}
