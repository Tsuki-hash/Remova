//! Bounded execution for external tools.
//!
//! Several scanners shell out to `where.exe`, `vswhere`, `dotnet`, `rustup`,
//! `java`, `reg.exe` and friends. A plain `Command::output()` waits forever and
//! buffers every byte, so one hung or chatty tool pins a blocking worker (and,
//! for the toolchain audit, one of the eight scan slots) with unbounded memory.
//!
//! This module gives those call sites a deadline, a byte budget, and a
//! cancellation poll. A timed-out child is killed and reaped; the caller gets a
//! distinct outcome so it can report "timed out" instead of pretending the tool
//! answered.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Default ceiling for a read-only probe. Long enough for a cold `vswhere` on a
/// busy machine, short enough that a hung tool cannot hold a scan slot.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// Per-stream byte budget. Output beyond this is discarded, not buffered.
pub const PROBE_OUTPUT_CAP: usize = 256 * 1024;
/// Registry tools run inside the cleanup lock. A longer ceiling is fine for a
/// large export, but it still has to end so the lock is released.
pub const REG_TOOL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedOutput {
    /// The process exited within the deadline. `truncated` means the cap hit.
    Finished {
        status_code: Option<i32>,
        success: bool,
        stdout: String,
        stderr: String,
        truncated: bool,
    },
    /// The deadline expired; the child was killed and reaped.
    TimedOut,
    /// The process could not be started or reaped.
    Failed(String),
}

impl BoundedOutput {
    /// True only when the process exited successfully inside the deadline.
    pub fn succeeded(&self) -> bool {
        matches!(self, Self::Finished { success: true, .. })
    }
}

fn read_capped<R: Read + Send + 'static>(
    mut reader: R,
    cap: usize,
    tx: mpsc::Sender<(Vec<u8>, bool)>,
) {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let room = cap.saturating_sub(kept.len());
                    if room == 0 {
                        truncated = true;
                        // Keep draining so the child never blocks on a full pipe.
                        continue;
                    }
                    let take = n.min(room);
                    kept.extend_from_slice(&chunk[..take]);
                    if take < n {
                        truncated = true;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send((kept, truncated));
    });
}

/// Run `cmd` with a deadline, a per-stream byte cap and a cancellation poll.
pub fn run_bounded(mut cmd: Command, timeout: Duration, cap: usize) -> BoundedOutput {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return BoundedOutput::Failed(e.to_string()),
    };
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        read_capped(stdout, cap, out_tx);
    }
    if let Some(stderr) = child.stderr.take() {
        read_capped(stderr, cap, err_tx);
    }

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return BoundedOutput::Failed(e.to_string());
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return BoundedOutput::TimedOut;
        }
        // Cancellation must not wait for the deadline: a cancelled scan should
        // release its slot promptly.
        if crate::scan_task::cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return BoundedOutput::Failed("cancelled".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let Some(status) = status else {
        return BoundedOutput::Failed("no exit status".into());
    };
    // The readers finish once the pipes close; a short wait avoids blocking a
    // probe on a detached grandchild that inherited the handle.
    let (stdout, out_truncated) = out_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    let (stderr, err_truncated) = err_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or_default();
    BoundedOutput::Finished {
        status_code: status.code(),
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        truncated: out_truncated || err_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut cmd = Command::new(crate::regops::sys_tool("cmd.exe"));
        cmd.args(["/c", script]);
        cmd
    }

    #[test]
    fn a_fast_command_returns_its_output() {
        let out = run_bounded(shell("echo hello"), Duration::from_secs(10), 4096);
        match out {
            BoundedOutput::Finished {
                success, stdout, ..
            } => {
                assert!(success);
                assert!(stdout.contains("hello"), "{stdout}");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn a_hung_command_is_killed_at_the_deadline() {
        let started = Instant::now();
        // `ping -n 30` sleeps for roughly 29 seconds without external tools.
        let out = run_bounded(
            shell("ping -n 30 127.0.0.1 > nul"),
            Duration::from_millis(300),
            4096,
        );
        assert_eq!(out, BoundedOutput::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the deadline must end the wait, not the child"
        );
    }

    #[test]
    fn output_beyond_the_cap_is_truncated_not_buffered() {
        let out = run_bounded(
            shell("for /L %i in (1,1,20000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
            Duration::from_secs(20),
            1024,
        );
        match out {
            BoundedOutput::Finished {
                stdout, truncated, ..
            } => {
                assert!(truncated, "the cap must be reported");
                assert!(stdout.len() <= 1024 + 64, "kept {} bytes", stdout.len());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn a_missing_program_fails_without_hanging() {
        let mut cmd = Command::new("definitely-not-a-real-program-remova");
        cmd.arg("--version");
        let out = run_bounded(cmd, Duration::from_secs(5), 1024);
        assert!(matches!(out, BoundedOutput::Failed(_)), "{out:?}");
    }
}
