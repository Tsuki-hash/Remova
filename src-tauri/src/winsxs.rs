//! WinSxS component-store analysis via DISM (read-only, elevated).
//!
//! `AnalyzeComponentStore` reports sizes and a cleanup recommendation without
//! changing anything. DISM output follows the OS display language, so the
//! invocation forces `/English` and parsing matches the stable English labels.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::async_runtime::{channel, Sender};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStoreAnalysis {
    /// DISM's own verdict on whether StartComponentCleanup would pay off.
    pub cleanup_recommended: Option<bool>,
    /// Size shown in Explorer (hardlinks make it overcount).
    pub explorer_size_bytes: Option<u64>,
    /// Real on-disk size of the component store.
    pub actual_size_bytes: Option<u64>,
}

/// Parse the fixed-format block DISM prints after a successful analysis.
/// Pure so the label matching is unit-testable against captured output.
pub fn parse_dism_analyze(stdout: &str) -> ComponentStoreAnalysis {
    let mut out = ComponentStoreAnalysis::default();
    for line in stdout.lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let label = label.trim().to_lowercase();
        let value = value.trim();
        match label.as_str() {
            "component store cleanup recommended" => {
                out.cleanup_recommended = match value.to_ascii_lowercase().as_str() {
                    "yes" => Some(true),
                    "no" => Some(false),
                    _ => None,
                };
            }
            "size of component store in explorer"
            | "windows explorer reported size of component store" => {
                out.explorer_size_bytes = parse_dism_size(value);
            }
            "actual size of component store" => {
                out.actual_size_bytes = parse_dism_size(value);
            }
            _ => {}
        }
    }
    out
}

/// DISM prints sizes as `7.87 GB` / `1,234 MB` / `512.0 KB` / `0 B`.
pub fn parse_dism_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number = parts.next()?;
    let unit = parts.next()?.to_lowercase();
    let number: f64 = number.replace(',', "").parse().ok()?;
    let multiplier = match unit.as_str() {
        "b" => 1.0,
        "kb" => 1024.0,
        "mb" => 1024.0 * 1024.0,
        "gb" => 1024.0 * 1024.0 * 1024.0,
        "tb" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}

type AnalysisResult = Result<ComponentStoreAnalysis, String>;

#[derive(Default)]
struct SharedAnalysis(Mutex<Option<Vec<Sender<AnalysisResult>>>>);

impl SharedAnalysis {
    async fn run(
        &'static self,
        work: impl FnOnce() -> AnalysisResult + Send + 'static,
    ) -> AnalysisResult {
        let (sender, mut result) = channel(1);
        {
            let mut active = self
                .0
                .lock()
                .map_err(|_| "component store analysis unavailable")?;
            if let Some(waiters) = active.as_mut() {
                waiters.retain(|waiter| !waiter.is_closed());
                waiters.push(sender);
            } else {
                *active = Some(vec![sender]);
                tauri::async_runtime::spawn(async move {
                    let result = tauri::async_runtime::spawn_blocking(work)
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result);
                    let waiters = self
                        .0
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take()
                        .unwrap_or_default();
                    for waiter in waiters {
                        let _ = waiter.try_send(result.clone());
                    }
                });
            }
        }
        result
            .recv()
            .await
            .ok_or_else(|| "component store analysis interrupted".to_string())?
    }
}

pub async fn analyze_shared() -> AnalysisResult {
    static ACTIVE: SharedAnalysis = SharedAnalysis(Mutex::new(None));
    ACTIVE.run(analyze_component_store).await
}

/// Run the read-only analysis. Requires elevation; the caller checks.
#[cfg(windows)]
fn analyze_component_store() -> AnalysisResult {
    let mut cmd = std::process::Command::new(crate::regops::sys_tool("dism.exe"));
    cmd.args([
        "/Online",
        "/Cleanup-Image",
        "/AnalyzeComponentStore",
        "/English",
    ]);
    // The analysis takes a while and runs from the panel; keep its console
    // window out of the user's face.
    crate::regops::hide_console(&mut cmd);
    let output = bounded_output(&mut cmd, std::time::Duration::from_secs(180), 128 * 1024)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "dism analyze failed with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    Ok(parse_dism_analyze(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn analyze_component_store() -> AnalysisResult {
    Err("component store analysis is only available on Windows".into())
}

#[cfg(windows)]
fn bounded_output(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
    limit: usize,
) -> Result<std::process::Output, String> {
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant};

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to launch dism.exe: {error}"))?;
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let start = Instant::now();
    let result = (|| {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        loop {
            if start.elapsed() >= timeout {
                return Err("dism:timeout".into());
            }
            let status = child.try_wait().map_err(|error| error.to_string())?;
            let read_out = drain_available(&mut stdout_pipe, &mut stdout, limit - stderr.len())?;
            let read_err = drain_available(&mut stderr_pipe, &mut stderr, limit - stdout.len())?;
            if !read_out && !read_err {
                if let Some(status) = status {
                    return Ok(Output {
                        status,
                        stdout,
                        stderr,
                    });
                }
                std::thread::sleep(
                    Duration::from_millis(20).min(timeout.saturating_sub(start.elapsed())),
                );
            }
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(windows)]
fn drain_available(
    pipe: &mut (impl std::io::Read + std::os::windows::io::AsRawHandle),
    output: &mut Vec<u8>,
    limit: usize,
) -> Result<bool, String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::PeekNamedPipe;

    let mut available = 0;
    // The dedicated reader owns the pipe, so no other reader can consume the bytes after peeking.
    let result = unsafe {
        PeekNamedPipe(
            HANDLE(pipe.as_raw_handle()),
            None,
            0,
            None,
            Some(&mut available),
            None,
        )
    };
    if let Err(error) = result {
        if error.code() == windows::core::HRESULT::from_win32(109) {
            return Ok(false);
        }
        return Err(error.to_string());
    }
    if available == 0 {
        return Ok(false);
    }
    let mut buf = [0u8; 4096];
    let bytes = (available as usize).min(buf.len());
    if bytes > limit - output.len() {
        return Err("dism:output_limit".into());
    }
    let bytes = pipe
        .read(&mut buf[..bytes])
        .map_err(|error| error.to_string())?;
    output.extend_from_slice(&buf[..bytes]);
    Ok(bytes != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const REAL_OUTPUT: &str = "\r\nDeployment Image Servicing and Management tool\r\nVersion: 10.0.26100.1\r\n\r\nImage Version: 10.0.26100.2\r\n\r\n[==========================100.0%==========================]\r\n\r\nComponent Store (WinSxS) Information:\r\n\r\nWindows Explorer Size: (removed for brevity)\r\n\r\nActual Size of Component Store: 8.45 GB\r\n\r\nShared with Windows: 6.12 GB\r\nBackups and Disabled Features: 1.03 GB\r\nCache and Temporary Data: 1.30 GB\r\nDate of Last Cleanup: 2026-09-30 09:00\r\n\r\nSize of Component Store in Explorer: 12.1 GB\r\n\r\nComponent Store Cleanup Recommended : Yes\r\n\r\nThe operation completed successfully.\r\n";

    #[test]
    fn parses_real_dism_output() {
        let a = parse_dism_analyze(REAL_OUTPUT);
        assert_eq!(a.cleanup_recommended, Some(true));
        assert_eq!(
            a.actual_size_bytes,
            Some((8.45 * 1024.0 * 1024.0 * 1024.0) as u64)
        );
        assert_eq!(
            a.explorer_size_bytes,
            Some((12.1 * 1024.0 * 1024.0 * 1024.0) as u64)
        );
    }

    #[test]
    fn parses_a_no_recommendation_run() {
        let a = parse_dism_analyze("Component Store Cleanup Recommended : No\r\n");
        assert_eq!(a.cleanup_recommended, Some(false));
        assert_eq!(a.actual_size_bytes, None);
    }

    #[test]
    fn parses_official_explorer_label_and_keeps_unknown_recommendations() {
        let a = parse_dism_analyze("Windows Explorer Reported Size of Component Store : 512.0 KB");
        assert_eq!(a.explorer_size_bytes, Some(524_288));
        assert_eq!(a.cleanup_recommended, None);
        assert_eq!(
            parse_dism_analyze("Component Store Cleanup Recommended: Pending").cleanup_recommended,
            None
        );
    }

    #[test]
    fn size_parsing_covers_units_and_thousands() {
        assert_eq!(parse_dism_size("512.0 KB"), Some(524_288));
        assert_eq!(parse_dism_size("1,234 MB"), Some(1_293_942_784));
        assert_eq!(parse_dism_size("2 TB"), Some(2_199_023_255_552));
        assert_eq!(parse_dism_size("0 B"), Some(0));
        assert_eq!(parse_dism_size("n/a"), None);
    }

    #[test]
    fn four_callers_share_one_analysis_and_can_retry_after_error() {
        let shared = Box::leak(Box::new(SharedAnalysis::default()));
        let (started, starting) = std::sync::mpsc::channel();
        let (finish, finishing) = std::sync::mpsc::channel();
        let first = tauri::async_runtime::spawn(shared.run(move || {
            started.send(()).unwrap();
            finishing.recv_timeout(Duration::from_secs(10)).unwrap();
            Err("dism:timeout".into())
        }));
        starting.recv_timeout(Duration::from_secs(10)).unwrap();
        let mut callers = vec![first];
        for _ in 0..3 {
            callers.push(tauri::async_runtime::spawn(
                shared.run(|| panic!("separate analysis")),
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while shared.0.lock().unwrap().as_ref().map_or(0, Vec::len) < 4 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        finish.send(()).unwrap();
        tauri::async_runtime::block_on(async {
            for caller in callers {
                assert_eq!(caller.await.unwrap(), Err("dism:timeout".into()));
            }
            assert_eq!(
                shared.run(|| Ok(ComponentStoreAnalysis::default())).await,
                Ok(ComponentStoreAnalysis::default())
            );
        });
    }

    #[cfg(windows)]
    #[test]
    fn process_capture_limits_time_and_combined_output() {
        let cmd = |args: &str| {
            let mut cmd = std::process::Command::new(crate::regops::sys_tool("cmd.exe"));
            cmd.args(["/D", "/C", args]);
            crate::regops::hide_console(&mut cmd);
            cmd
        };
        let timeout = Duration::from_secs(5);
        let output = bounded_output(&mut cmd("echo success"), timeout, 1024).unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("success"));
        assert_eq!(
            bounded_output(&mut cmd("echo output"), timeout, 4).unwrap_err(),
            "dism:output_limit"
        );
        assert_eq!(
            bounded_output(
                &mut cmd("for /L %i in (1,1,2147483647) do @rem busy"),
                Duration::from_millis(200),
                1024
            )
            .unwrap_err(),
            "dism:timeout"
        );
        assert!(bounded_output(&mut cmd("echo retry"), timeout, 1024)
            .unwrap()
            .status
            .success());
    }
}
