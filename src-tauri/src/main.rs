#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Context menu: `Remova.exe --analyze "C:\path\to\app.exe"`
    // Write the path for the webview to pick up after startup.
    let args: Vec<String> = std::env::args().skip(1).collect();
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
