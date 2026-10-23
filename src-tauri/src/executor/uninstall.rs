//! Official uninstaller command resolution and launch.

use super::OfficialUninstallResult;

fn system_uninstall_tool(relative: &str) -> Option<String> {
    let tool = crate::regops::sys_tool(relative);
    (!tool.is_empty()).then_some(tool)
}

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
            system_uninstall_tool(r"WindowsPowerShell\v1.0\powershell.exe")?,
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            // No surrounding quotes: charset already excludes quote/space/metachars.
            format!("Remove-AppxPackage -Package {full} -ErrorAction Stop"),
        ]);
    }

    // Identify the executable, not substrings in vendor names, directories or arguments.
    let parts = split_win_args(&raw);
    let is_msiexec = parts.first().is_some_and(|exe| {
        let leaf = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
        leaf.eq_ignore_ascii_case("msiexec") || leaf.eq_ignore_ascii_case("msiexec.exe")
    });
    if let Some(guid) = crate::shared::first_guid(&raw) {
        let trimmed = raw.trim();
        let bare_guid = trimmed == guid || trimmed.trim_matches('"').eq_ignore_ascii_case(&guid);
        if is_msiexec || bare_guid {
            return Some(vec![
                system_uninstall_tool("msiexec.exe")?,
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

    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

/// Only absolute images may be launched. A bare `unins000.exe /S` is resolved
/// through the process CWD/PATH and can be hijacked by a local user.
fn uninstall_exe_is_absolute(exe: &str) -> bool {
    std::path::Path::new(exe).is_absolute()
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

/// MSI 3010/1641 mean success-with-reboot-required, not failure. Treating them
/// as failure aborted the whole residual cleanup after the product was gone.
fn uninstaller_exit_ok(code: Option<i32>) -> bool {
    matches!(code, Some(0 | 3010 | 1641))
}

/// 1641 means the uninstaller already started the reboot; 3010 merely requests one.
fn uninstaller_reboot_initiated(code: Option<i32>) -> bool {
    code == Some(1641)
}

/// Map one uninstaller exit to the session-wide reboot gate. Kept separate so
/// the producer link (1641 -> pending) is testable without launching a process.
fn record_reboot_exit(code: Option<i32>, app: &crate::apps::InstalledApp) -> (bool, bool) {
    let initiated = uninstaller_reboot_initiated(code);
    let mut persisted = true;
    if initiated {
        persisted = super::reboot::mark_pending(&crate::apps::app_key(app));
        if !persisted {
            // The in-process gate is active, but a restart would lose it. The
            // caller receives `reboot_gate_persisted=false` so the UI can warn.
            eprintln!(
                "reboot gate could not be persisted; cleanup remains blocked in this process"
            );
        }
    }
    (initiated, persisted)
}

fn pending_result() -> OfficialUninstallResult {
    OfficialUninstallResult {
        ok: false,
        message: super::reboot::PENDING_MESSAGE.into(),
        had_command: true,
        reboot_initiated: false,
        reboot_gate_persisted: true,
    }
}

/// Public official-uninstall entry. It shares the destructive-operation lock
/// with full cleanup, broken records and firewall deletion.
pub fn run_official_uninstall(app: &crate::apps::InstalledApp) -> OfficialUninstallResult {
    if super::reboot::any_pending() {
        return pending_result();
    }
    let _guard = super::CLEANUP_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if super::reboot::any_pending() {
        return pending_result();
    }
    run_official_uninstall_locked(app)
}

/// Apply the reboot gate before any destructive official-uninstall work. The
/// closure is intentionally lazy so a pending reboot cannot reach process spawn.
fn with_no_reboot_pending<T>(work: impl FnOnce() -> T, pending: T) -> T {
    if super::reboot::any_pending() {
        pending
    } else {
        work()
    }
}

/// Internal path for full cleanup, which already owns CLEANUP_LOCK.
pub(crate) fn run_official_uninstall_locked(
    app: &crate::apps::InstalledApp,
) -> OfficialUninstallResult {
    with_no_reboot_pending(|| run_official_uninstall_unchecked(app), pending_result())
}

fn run_official_uninstall_unchecked(app: &crate::apps::InstalledApp) -> OfficialUninstallResult {
    if app.uninstall_string.trim().is_empty() && app.quiet_uninstall_string.trim().is_empty() {
        return OfficialUninstallResult {
            ok: false,
            message: "no uninstall string".into(),
            had_command: false,
            reboot_initiated: false,
            reboot_gate_persisted: true,
        };
    }
    // S-RCE: never execute a command the client invented. Resolve the identity
    // to the server-scanned record and execute *its* command pair, so even a
    // caller that keeps a trusted identity but rewrites the command cannot
    // reach spawn. The recorded source — not the client field — drives the
    // elevated-HKCU refusal below.
    let Some(scanned) = crate::apps::trusted_app(app) else {
        // A command exists but we refuse to run it. Reporting `had_command:
        // false` here told the caller there was nothing to run, so cleanup
        // proceeded to delete leftovers of an app that is still installed —
        // leaving it un-uninstallable from Programs & Features.
        return OfficialUninstallResult {
            ok: false,
            message: "uninstall command not from latest app scan".into(),
            had_command: true,
            reboot_initiated: false,
            reboot_gate_persisted: true,
        };
    };
    let scan_source = scanned.source.clone();
    // Per-user uninstall strings are user-writable. Running one from an
    // elevated Remova would execute an arbitrary user-controlled image as
    // administrator; require a non-elevated launch for HKCU entries. The
    // decision uses the recorded scan source, never the client-supplied field.
    if crate::sysops::is_elevated_for_gate() && scan_source.to_ascii_uppercase().starts_with("HKCU")
    {
        return OfficialUninstallResult {
            ok: false,
            message: "elevated HKCU uninstall refused".into(),
            had_command: true,
            reboot_initiated: false,
            reboot_gate_persisted: true,
        };
    }
    let cmd = build_uninstall_command(
        &scanned.uninstall_string,
        &scanned.quiet_uninstall_string,
        true,
    );
    match cmd {
        Some(argv) if !argv.is_empty() => {
            let mut parts = argv.iter();
            let exe = parts.next().unwrap().clone();
            if !uninstall_exe_is_absolute(&exe) {
                // A relative image would be resolved against the process CWD /
                // PATH, which a local user can influence. MSI/Store paths are
                // already resolved to absolute system tools above.
                return OfficialUninstallResult {
                    ok: false,
                    message: "uninstall command uses a relative executable; refused".into(),
                    had_command: true,
                    reboot_initiated: false,
                    reboot_gate_persisted: true,
                };
            }
            let rest: Vec<String> = parts.cloned().collect();
            match std::process::Command::new(&exe).args(&rest).spawn() {
                Ok(mut child) => {
                    let timeout =
                        std::time::Duration::from_secs(crate::constants::UNINSTALL_TIMEOUT_SECS);
                    let start = std::time::Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let code = status.code();
                                let ok = uninstaller_exit_ok(code);
                                let (initiated, persisted) = record_reboot_exit(code, &scanned);
                                let message = if ok && matches!(code, Some(3010 | 1641)) {
                                    format!("uninstaller finished (reboot required): {}", argv[0])
                                } else if ok {
                                    format!("uninstaller finished: {}", argv[0])
                                } else {
                                    format!("uninstaller exited with {:?}", code)
                                };
                                return OfficialUninstallResult {
                                    ok,
                                    message,
                                    had_command: true,
                                    reboot_initiated: initiated,
                                    reboot_gate_persisted: persisted,
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
                                        reboot_initiated: false,
                                        reboot_gate_persisted: true,
                                    };
                                }
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            Err(_e) => {
                                return OfficialUninstallResult {
                                    ok: false,
                                    message: "uninstall wait failed".into(),
                                    had_command: true,
                                    reboot_initiated: false,
                                    reboot_gate_persisted: true,
                                };
                            }
                        }
                    }
                }
                Err(_e) => OfficialUninstallResult {
                    ok: false,
                    message: "uninstall launch failed".into(),
                    had_command: true,
                    reboot_initiated: false,
                    reboot_gate_persisted: true,
                },
            }
        }
        _ => {
            // A command string exists but cannot be turned into argv (dirty
            // vendor data: an unterminated quote, or a `remova-store:`
            // PackageFullName with illegal characters / over 200 chars).
            // Reporting `had_command: false` here was a lie — it told the caller
            // there was nothing to run, so `pipeline`'s `had_command && !ok`
            // abort gate never fired and cleanup deleted the leftovers of an app
            // that is still registered in Programs & Features, leaving it
            // un-uninstallable from there.
            OfficialUninstallResult {
                ok: false,
                message: "unusable uninstall command".into(),
                had_command: true,
                reboot_initiated: false,
                reboot_gate_persisted: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboot_required_exit_codes_count_as_success() {
        for code in [0, 3010, 1641] {
            assert!(
                uninstaller_exit_ok(Some(code)),
                "{code} must not abort cleanup"
            );
        }
        for code in [1603, 5, 1] {
            assert!(!uninstaller_exit_ok(Some(code)));
        }
        assert!(!uninstaller_exit_ok(None));
    }

    #[test]
    fn relative_uninstall_images_are_refused() {
        assert!(uninstall_exe_is_absolute(
            r"C:\Program Files\App\unins000.exe"
        ));
        assert!(uninstall_exe_is_absolute(r"\\server\share\unins000.exe"));
        assert!(!uninstall_exe_is_absolute("unins000.exe"));
        assert!(!uninstall_exe_is_absolute(r"bin\unins000.exe"));
    }

    #[test]
    fn only_reboot_initiated_blocks_residual_cleanup() {
        assert!(uninstaller_reboot_initiated(Some(1641)));
        for code in [0, 3010, 1603] {
            assert!(!uninstaller_reboot_initiated(Some(code)));
        }
        assert!(!uninstaller_reboot_initiated(None));
    }

    #[test]
    fn exit_1641_marks_the_global_reboot_gate() {
        let app = crate::apps::InstalledApp {
            name: "RebootProducerFixture".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: r"HKLM64\SOFTWARE\RebootProducerFixture".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        crate::executor::reboot::reset_for_test();
        assert!(record_reboot_exit(Some(1641), &app).0);
        assert!(crate::executor::reboot::is_pending(&crate::apps::app_key(
            &app
        )));
        assert!(crate::executor::reboot::any_pending());

        let other = crate::apps::InstalledApp {
            name: "OtherFixture".into(),
            ..app.clone()
        };
        assert!(!record_reboot_exit(Some(3010), &other).0);
        assert!(!crate::executor::reboot::is_pending(&crate::apps::app_key(
            &other
        )));
        crate::executor::reboot::reset_for_test();
        assert!(!crate::executor::reboot::any_pending());
    }

    #[test]
    fn reboot_pending_prevents_official_uninstall_work_before_spawn() {
        let app = crate::apps::InstalledApp {
            name: "PendingGateFixture".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: r"HKLM64\SOFTWARE\PendingGateFixture".into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        crate::executor::reboot::reset_for_test();
        assert!(record_reboot_exit(Some(1641), &app).0);
        let mut spawn_calls = 0;
        let blocked = with_no_reboot_pending(
            || {
                spawn_calls += 1;
                "spawned"
            },
            "blocked",
        );
        assert_eq!(blocked, "blocked");
        assert_eq!(spawn_calls, 0);

        crate::executor::reboot::reset_for_test();
        assert!(!record_reboot_exit(Some(3010), &app).0);
        let allowed = with_no_reboot_pending(
            || {
                spawn_calls += 1;
                "spawned"
            },
            "blocked",
        );
        assert_eq!(allowed, "spawned");
        assert_eq!(spawn_calls, 1);
        crate::executor::reboot::reset_for_test();
    }

    #[test]
    fn a_rewritten_source_cannot_reuse_another_identitys_trust() {
        let _g = crate::apps::trust_test_support::guard();
        let scanned = crate::apps::InstalledApp {
            name: "HkcuUserApp".into(),
            publisher: "Vendor".into(),
            version: "1.0".into(),
            install_location: String::new(),
            uninstall_string: r"C:\Users\me\AppData\Local\UserApp\uninstall.exe".into(),
            quiet_uninstall_string: String::new(),
            source: "HKCU".into(),
            registry_key: r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\HkcuUserApp"
                .into(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        crate::apps::remember_uninstall_commands(std::slice::from_ref(&scanned));
        assert!(crate::apps::is_trusted_uninstall_app(&scanned));

        // Same command bytes, forged identity: the trust record is keyed by the
        // scanned identity, so this must no longer resolve.
        let forged = crate::apps::InstalledApp {
            source: "HKLM64".into(),
            registry_key: r"HKLM64\SOFTWARE\Vendor\HkcuUserApp".into(),
            ..scanned.clone()
        };
        assert!(
            !crate::apps::is_trusted_uninstall_app(&forged),
            "a forged source/registry key must not inherit trust"
        );
        assert!(crate::apps::trusted_scan_source(&forged).is_none());

        // The recorded identity still resolves, and reports its scanned source
        // (which is what the elevated-HKCU decision now consumes).
        assert_eq!(
            crate::apps::trusted_scan_source(&scanned).as_deref(),
            Some("HKCU")
        );
    }

    #[test]
    fn msi_substrings_do_not_replace_vendor_commands() {
        let guid = "{9A1B2C3D-1111-2222-3333-444455556666}";
        for exe in [
            "msiexec-helper.exe",
            r"C:\msiexec\uninstall.exe",
            "vendor.exe",
        ] {
            let raw = format!("\"{exe}\" --msiexec {guid}");
            let cmd = build_uninstall_command(&raw, "", false).unwrap();
            assert_eq!(cmd, vec![exe.to_string(), "--msiexec".into(), guid.into()]);
        }
        for exe in ["msiexec", "MSIEXEC.EXE", r"C:\System Tools\msiexec.exe"] {
            let cmd = build_uninstall_command(&format!("\"{exe}\" /x {guid}"), "", false).unwrap();
            assert_eq!(cmd[0], crate::regops::sys_tool("msiexec.exe"));
            assert!(cmd[2].eq_ignore_ascii_case(guid));
        }
    }

    #[test]
    fn msi_only_for_msiexec_or_bare_guid() {
        // MSI product code string
        let bare = build_uninstall_command(r"{9A1B2C3D-1111-2222-3333-444455556666}", "", false);
        assert!(
            matches!(bare.as_deref(), Some([msi, ..]) if msi == &crate::regops::sys_tool("msiexec.exe"))
        );
        assert!(std::path::Path::new(&bare.unwrap()[0]).is_absolute());
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
        assert_eq!(
            cmd[0],
            crate::regops::sys_tool(r"WindowsPowerShell\v1.0\powershell.exe")
        );
        assert!(std::path::Path::new(&cmd[0]).is_absolute());
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
        assert_eq!(cmd[0], crate::regops::sys_tool("msiexec.exe"));
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
        let _g = crate::apps::trust_test_support::guard();
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
        crate::apps::remember_uninstall_commands(std::slice::from_ref(&app));
        let r = run_official_uninstall(&app);
        assert!(!r.had_command);
        assert!(!r.ok);
        assert_eq!(r.message, "no uninstall string");
    }

    /// A command that exists but is refused must not be reported as "there was
    /// no command". The pipeline aborts on `had_command && !ok`; reporting
    /// `false` here made it proceed and delete the leftovers of an app that is
    /// still installed, leaving it un-uninstallable from Programs & Features.
    #[test]
    fn refused_uninstaller_still_counts_as_having_a_command() {
        // An uninstall string the trust set has never seen, so the command is
        // present but refused.
        let app = crate::apps::InstalledApp {
            name: "Stale".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: r"C:\Vendor\unins000.exe".into(),
            quiet_uninstall_string: String::new(),
            source: "HKLM64".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        let r = run_official_uninstall(&app);
        assert!(!r.ok);
        assert_eq!(r.message, "uninstall command not from latest app scan");
        assert!(
            r.had_command,
            "a refused command must still be reported so cleanup aborts"
        );
    }

    /// An uninstall string that is present but cannot be parsed into argv must
    /// also count as "there was a command". Both shapes below are reachable
    /// from real vendor data (dirty `UninstallString` values), and reporting
    /// `had_command: false` for them meant the pipeline's abort gate never
    /// fired, so cleanup deleted the leftovers of an app that is still
    /// registered in Programs & Features.
    #[test]
    fn unparseable_uninstall_string_still_counts_as_having_a_command() {
        let _trust = crate::apps::trust_test_support::guard();
        for bad in [
            // Opens a quoted exe path but never closes the quote.
            r#""C:\Vendor\My App\unins000.exe /SILENT"#,
            // Appx PackageFullName with a character outside [A-Za-z0-9._-].
            "remova-store:Vendor App_1.2.3.0_x64__8wekyb3d8bbwe!",
            // Over the 200-char PackageFullName ceiling.
            format!("remova-store:{}", "a".repeat(201)).as_str(),
        ] {
            let app = crate::apps::InstalledApp {
                name: "Dirty".into(),
                version: String::new(),
                publisher: String::new(),
                install_location: String::new(),
                uninstall_string: bad.to_string(),
                quiet_uninstall_string: String::new(),
                source: "HKLM64".into(),
                registry_key: String::new(),
                estimated_size_kb: 0,
                install_date: String::new(),
                display_icon: String::new(),
            };
            // The command string must be non-empty, or the early return is the
            // legitimate "there really is no command" case.
            assert!(!app.uninstall_string.trim().is_empty());
            assert!(
                build_uninstall_command(&app.uninstall_string, &app.quiet_uninstall_string, true)
                    .is_none(),
                "fixture must actually be unparseable"
            );
            // Arm the trust set for exactly this command so the run reaches the
            // parse step instead of the earlier "not from latest scan" refusal,
            // then put the process-wide set back the way it was found.
            crate::apps::remember_uninstall_commands(std::slice::from_ref(&app));
            let r = run_official_uninstall(&app);
            assert!(!r.ok);
            assert_eq!(
                r.message, "unusable uninstall command",
                "the message must not claim there was no command for {bad:?}"
            );
            assert!(
                r.had_command,
                "an unparseable command must still be reported so cleanup aborts"
            );
        }
    }
}
