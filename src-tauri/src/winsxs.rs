//! WinSxS component-store analysis via DISM (read-only, elevated).
//!
//! `AnalyzeComponentStore` reports sizes and a cleanup recommendation without
//! changing anything. DISM output follows the OS display language, so the
//! invocation forces `/English` and parsing matches the stable English labels.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStoreAnalysis {
    /// DISM's own verdict on whether StartComponentCleanup would pay off.
    pub cleanup_recommended: bool,
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
                out.cleanup_recommended = value.eq_ignore_ascii_case("yes");
            }
            "size of component store in explorer" => {
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

/// Run the read-only analysis. Requires elevation; the caller checks.
pub fn analyze_component_store() -> Result<ComponentStoreAnalysis, String> {
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
    let output = cmd
        .output()
        .map_err(|e| format!("failed to launch dism.exe: {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_OUTPUT: &str = "\r\nDeployment Image Servicing and Management tool\r\nVersion: 10.0.26100.1\r\n\r\nImage Version: 10.0.26100.2\r\n\r\n[==========================100.0%==========================]\r\n\r\nComponent Store (WinSxS) Information:\r\n\r\nWindows Explorer Size: (removed for brevity)\r\n\r\nActual Size of Component Store: 8.45 GB\r\n\r\nShared with Windows: 6.12 GB\r\nBackups and Disabled Features: 1.03 GB\r\nCache and Temporary Data: 1.30 GB\r\nDate of Last Cleanup: 2026-09-30 09:00\r\n\r\nSize of Component Store in Explorer: 12.1 GB\r\n\r\nComponent Store Cleanup Recommended : Yes\r\n\r\nThe operation completed successfully.\r\n";

    #[test]
    fn parses_real_dism_output() {
        let a = parse_dism_analyze(REAL_OUTPUT);
        assert!(a.cleanup_recommended);
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
        assert!(!a.cleanup_recommended);
        assert_eq!(a.actual_size_bytes, None);
    }

    #[test]
    fn size_parsing_covers_units_and_thousands() {
        assert_eq!(parse_dism_size("512.0 KB"), Some(524_288));
        assert_eq!(parse_dism_size("1,234 MB"), Some(1_293_942_784));
        assert_eq!(parse_dism_size("2 TB"), Some(2_199_023_255_552));
        assert_eq!(parse_dism_size("0 B"), Some(0));
        assert_eq!(parse_dism_size("n/a"), None);
    }
}
