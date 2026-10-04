#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Elevated-relaunch handshake: the admin copy spawned by a non-admin run
    // waits for the old process to exit, so the single-instance lock is free
    // by the time Tauri initializes. Timeout → the old run stayed (the user
    // could not finish exiting) — leave quietly. Busy confirmation finishes
    // before spawning this copy.
    if let Some(pid) = parse_elevated_relaunch_pid(&args) {
        if !remova_lib::sysops::wait_for_process_exit(pid, 15_000) {
            return;
        }
    }
    // Context menu: `Remova.exe --analyze "C:\path\to\app.exe"`
    // Write the path for the webview to pick up after startup.
    if let Some(i) = args.iter().position(|a| a == "--analyze") {
        if let Some(target) = args.get(i + 1) {
            let t = target.trim().to_string();
            if !t.is_empty() {
                if let Some(dir) = dirs_data_local() {
                    let _ = publish_pending(&dir, &t);
                }
            }
        }
    }
    remova_lib::run()
}

fn parse_elevated_relaunch_pid(args: &[String]) -> Option<u32> {
    args.iter()
        .find_map(|a| a.strip_prefix("--elevated-relaunch="))
        .and_then(|v| v.parse::<u32>().ok())
}

fn publish_pending(dir: &std::path::Path, target: &str) -> std::io::Result<()> {
    let _ancestors = remova_lib::fsutil::pin_existing_parents(dir)?;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("pending_analyze.txt");
    let _parents = remova_lib::fsutil::pin_existing_parents(&path)?;
    remova_lib::fsutil::write_bytes_atomic(&path, target.as_bytes())
}

fn dirs_data_local() -> Option<std::path::PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(std::path::PathBuf::from(local).join("Remova"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn elevated_relaunch_pid_parses_marker_only() {
        assert_eq!(
            super::parse_elevated_relaunch_pid(&["--elevated-relaunch=4242".to_string()]),
            Some(4242)
        );
        assert_eq!(
            super::parse_elevated_relaunch_pid(&[
                "--analyze".to_string(),
                "--elevated-relaunch=0".to_string()
            ]),
            Some(0)
        );
        assert_eq!(
            super::parse_elevated_relaunch_pid(&["--elevated-relaunch=abc".to_string()]),
            None
        );
        assert_eq!(super::parse_elevated_relaunch_pid(&[]), None);
    }

    #[cfg(windows)]
    #[test]
    fn wait_for_process_exit_times_out_on_live_process() {
        // The test process itself is alive for the whole call — a short wait
        // must report "still running" (false), never a false handoff.
        assert!(!remova_lib::sysops::wait_for_process_exit(
            std::process::id(),
            50
        ));
    }

    #[cfg(windows)]
    #[test]
    fn wait_for_process_exit_proceeds_when_old_instance_is_gone() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn probe process");
        // Wait for the child to actually terminate before treating its PID
        // as gone; the handle keeps the PID from being recycled meanwhile.
        let status = child.wait().expect("wait probe");
        assert!(status.success());
        assert!(remova_lib::sysops::wait_for_process_exit(child.id(), 5_000));
    }

    #[test]
    fn pending_publish_preserves_a_linked_target() {
        let root = std::env::temp_dir().join(format!(
            "remova-pending-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let sentinel = root.join("sentinel");
        std::fs::write(&sentinel, b"keep").unwrap();
        std::fs::hard_link(&sentinel, root.join("pending_analyze.txt")).unwrap();
        super::publish_pending(&root, "selected app").unwrap();
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
        assert_eq!(
            std::fs::read_to_string(root.join("pending_analyze.txt")).unwrap(),
            "selected app"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
