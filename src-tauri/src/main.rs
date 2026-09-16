#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Context menu: `Remova.exe --analyze "C:\path\to\app.exe"`
    // Write the path for the webview to pick up after startup (FUNC-2).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--analyze") {
        if let Some(target) = args.get(i + 1) {
            let t = target.trim().to_string();
            if !t.is_empty() {
                if let Some(dir) = dirs_data_local() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join("pending_analyze.txt"), &t);
                }
            }
        }
    }
    remova_lib::run()
}

fn dirs_data_local() -> Option<std::path::PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(std::path::PathBuf::from(local).join("Remova"))
}
