//! Official uninstaller command resolution and launch.

use super::OfficialUninstallResult;

/// Build argv for the official uninstaller (parity with Python `build_uninstall_command`).
/// Store packages (`remova-store:<PackageFullName>`) map to PowerShell Remove-AppxPackage.
pub fn build_uninstall_command(
    uninstall_string: &str,
    quiet_uninstall: &str,
    prefer_quiet: bool,
) -> Option<Vec<String>> {
    let mut raw = String::new();
    if prefer_quiet && !quiet_uninstall.trim().is_empty() {
        raw = quiet_uninstall.trim().to_string();
    }
    if raw.is_empty() {
        raw = uninstall_string.trim().to_string();
    }
    if raw.is_empty() {
        return None;
    }

    if let Some(full) = raw.strip_prefix("remova-store:") {
        let full = full.trim();
        // Package family names are [A-Za-z0-9._-] only — never interpolate into PS unvalidated.
        if full.is_empty()
            || full.len() > 200
            || !full
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return None;
        }
        return Some(vec![
            "powershell.exe".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            // No surrounding quotes: charset already excludes quote/space/metachars.
            format!("Remove-AppxPackage -Package {full} -ErrorAction Stop"),
        ]);
    }

    // MSI product code: only when the string is clearly MSI (msiexec present, or bare {GUID}).
    if let Some(guid) = extract_guid(&raw) {
        let lower = raw.to_lowercase();
        let trimmed = raw.trim();
        let bare_guid = trimmed == guid || trimmed.trim_matches('"').eq_ignore_ascii_case(&guid);
        if lower.contains("msiexec") || bare_guid {
            return Some(vec![
                "msiexec.exe".into(),
                "/x".into(),
                guid,
                "/qn".into(),
                "/norestart".into(),
            ]);
        }
    }

    if let Some(rest) = raw.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = rest[..end].to_string();
        let after = rest[end + 1..].trim();
        let args: Vec<String> = split_win_args(after);
        let mut cmd = vec![exe];
        cmd.extend(args);
        return Some(cmd);
    }

    let parts = split_win_args(&raw);
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

fn extract_guid(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s[start..].find('}')? + start;
    let g = &s[start..=end];
    let body = &g[1..g.len() - 1];
    if body.len() == 36 && body.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        Some(g.to_string())
    } else {
        None
    }
}

/// `CommandLineToArgvW`-compatible splitting: honours doubled quotes (`""` → one
/// literal quote) and backslash-before-quote escapes (`\"`), which a naive quote toggle dropped.
fn split_win_args(s: &str) -> Vec<String> {
    let ch: Vec<char> = s.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut seen = false;
    let mut backs = 0usize;
    let mut i = 0usize;
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' {
            backs += 1;
            i += 1;
            continue;
        }
        if c == '"' {
            if backs > 0 {
                cur.push_str(&"\\".repeat(backs / 2));
                if backs % 2 == 1 {
                    cur.push('"');
                    seen = true;
                } else {
                    in_quotes = !in_quotes;
                    seen = true;
                }
                backs = 0;
                i += 1;
                continue;
            }
            if in_quotes && ch.get(i + 1) == Some(&'"') {
                cur.push('"');
                seen = true;
                i += 2;
                continue;
            }
            in_quotes = !in_quotes;
            seen = true;
            i += 1;
            continue;
        }
        if (c == ' ' || c == '\t') && !in_quotes {
            if backs > 0 {
                cur.push_str(&"\\".repeat(backs));
                backs = 0;
            }
            if seen {
                out.push(std::mem::take(&mut cur));
                seen = false;
            }
            i += 1;
            continue;
        }
        if backs > 0 {
            cur.push_str(&"\\".repeat(backs));
            backs = 0;
        }
        cur.push(c);
        seen = true;
        i += 1;
    }
    if backs > 0 {
        cur.push_str(&"\\".repeat(backs));
    }
    if seen || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Launch the official uninstaller and wait (shared by full cleanup and beginner uninstall).
pub fn run_official_uninstall(app: &crate::apps::InstalledApp) -> OfficialUninstallResult {
    // S-RCE: never execute a command the client invented — must match last server scan.
    if app.uninstall_string.trim().is_empty() && app.quiet_uninstall_string.trim().is_empty() {
        return OfficialUninstallResult {
            ok: false,
            message: "no uninstall string".into(),
            had_command: false,
        };
    }
    if !crate::apps::is_trusted_uninstall_app(app) {
        return OfficialUninstallResult {
            ok: false,
            message: "uninstall command not from latest app scan".into(),
            had_command: false,
        };
    }
    let cmd = build_uninstall_command(&app.uninstall_string, &app.quiet_uninstall_string, true);
    match cmd {
        Some(argv) if !argv.is_empty() => {
            let mut parts = argv.iter();
            let exe = parts.next().unwrap().clone();
            let rest: Vec<String> = parts.cloned().collect();
            match std::process::Command::new(&exe).args(&rest).spawn() {
                Ok(mut child) => {
                    let timeout =
                        std::time::Duration::from_secs(crate::constants::UNINSTALL_TIMEOUT_SECS);
                    let start = std::time::Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let ok = status.success();
                                let message = if ok {
                                    format!("uninstaller finished: {}", argv[0])
                                } else {
                                    format!("uninstaller exited with {:?}", status.code())
                                };
                                return OfficialUninstallResult {
                                    ok,
                                    message,
                                    had_command: true,
                                };
                            }
                            Ok(None) => {
                                if start.elapsed() >= timeout {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return OfficialUninstallResult {
                                        ok: false,
                                        message: format!("uninstaller timed out (5m): {}", argv[0]),
                                        had_command: true,
                                    };
                                }
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            Err(e) => {
                                return OfficialUninstallResult {
                                    ok: false,
                                    message: format!("wait failed: {e}"),
                                    had_command: true,
                                };
                            }
                        }
                    }
                }
                Err(e) => OfficialUninstallResult {
                    ok: false,
                    message: format!("launch failed: {e}"),
                    had_command: true,
                },
            }
        }
        _ => OfficialUninstallResult {
            ok: false,
            message: "no uninstall string".into(),
            had_command: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_only_for_msiexec_or_bare_guid() {
        // MSI product code string
        let bare = build_uninstall_command(r"{9A1B2C3D-1111-2222-3333-444455556666}", "", false);
        assert!(matches!(bare.as_deref(), Some([msi, ..]) if msi == "msiexec.exe"));
        // msiexec with extra args
        let msi = build_uninstall_command(
            r#"C:\Windows\System32\msiexec.exe /x {9A1B2C3D-1111-2222-3333-444455556666}"#,
            "",
            false,
        );
        assert!(msi.is_some());
        // Non-MSI uninstall that merely contains a GUID → quoted exe path
        let other = build_uninstall_command(
            r#""C:\Program Files\Vendor\App\unins000.exe" /GUID={9A1B2C3D-1111-2222-3333-444455556666}"#,
            "",
            false,
        )
        .expect("quoted exe");
        assert_eq!(other[0], r"C:\Program Files\Vendor\App\unins000.exe");
        assert!(!other[0].eq_ignore_ascii_case("msiexec.exe"));
    }

    #[test]
    fn store_uninstall_maps_to_powershell() {
        let cmd = build_uninstall_command(
            "remova-store:Microsoft.WindowsCalculator_10.2210.0.0_x64__8wekyb3d8bbwe",
            "",
            true,
        )
        .unwrap();
        assert_eq!(cmd[0], "powershell.exe");
        assert!(cmd.iter().any(|a| a.contains("Remove-AppxPackage")));
        assert!(cmd
            .iter()
            .any(|a| a.contains("Microsoft.WindowsCalculator_10.2210.0.0_x64__8wekyb3d8bbwe")));
    }

    #[test]
    fn msi_guid() {
        let cmd = build_uninstall_command(
            r"MsiExec.exe /X{12345678-1234-1234-1234-1234567890AB}",
            "",
            true,
        )
        .unwrap();
        assert_eq!(cmd[0].to_lowercase(), "msiexec.exe");
        assert!(
            cmd.contains(&"/x".to_string()) || cmd.iter().any(|x| x.eq_ignore_ascii_case("/x"))
        );
        assert!(cmd
            .iter()
            .any(|x| x.eq_ignore_ascii_case("{12345678-1234-1234-1234-1234567890AB}")));
    }

    #[test]
    fn quoted_exe() {
        let cmd =
            build_uninstall_command(r#""C:\Program Files\App\uninst.exe" /S /foo=bar"#, "", true)
                .unwrap();
        assert_eq!(cmd[0], r"C:\Program Files\App\uninst.exe");
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn split_win_args_follows_windows_quoting_rules() {
        assert_eq!(
            split_win_args("C:\\a\\uninst.exe /S /v"),
            vec!["C:\\a\\uninst.exe", "/S", "/v"]
        );
        // A space inside quotes stays in one token.
        assert_eq!(
            split_win_args("C:\\a.exe --log=\"C:\\My Logs\\x.txt\" --q"),
            vec!["C:\\a.exe", "--log=C:\\My Logs\\x.txt", "--q"]
        );
        // Doubled quotes inside a quoted run collapse to one literal quote.
        assert_eq!(
            split_win_args("\"C:\\a\\un \"\"weird\"\" inst.exe\""),
            vec!["C:\\a\\un \"weird\" inst.exe"]
        );
        // A backslash before a quote escapes it instead of closing the run.
        assert_eq!(split_win_args("\"C:\\p\\\" x\""), vec!["C:\\p\" x"]);
        // An even run of backslashes halves and still closes the quote.
        assert_eq!(
            split_win_args("\"C:\\tools\\\\\" /S"),
            vec!["C:\\tools\\", "/S"]
        );
        // An explicitly empty quoted argument survives.
        assert_eq!(
            split_win_args("C:\\a.exe \"\" /S"),
            vec!["C:\\a.exe", "", "/S"]
        );
        assert!(split_win_args("   ").is_empty());
    }

    #[test]
    fn prefer_quiet() {
        let cmd =
            build_uninstall_command(r"C:\a\uninst.exe", r#""C:\a\uninst.exe" /S"#, true).unwrap();
        assert!(cmd.iter().any(|x| x == "/S"));
    }

    #[test]
    fn empty_none() {
        assert!(build_uninstall_command("", "", true).is_none());
    }

    #[test]
    fn official_uninstall_no_string() {
        let app = crate::apps::InstalledApp {
            name: "Ghost".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let r = run_official_uninstall(&app);
        assert!(!r.had_command);
        assert!(!r.ok);
        assert_eq!(r.message, "no uninstall string");
    }
}
