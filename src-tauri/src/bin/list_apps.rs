//! CLI: print installed apps as JSON for parity compare.

fn main() {
    let apps = remova_lib::apps::scan_installed_apps();
    println!("{}", serde_json::to_string_pretty(&apps).unwrap_or_default());
}
