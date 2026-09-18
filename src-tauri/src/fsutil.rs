//! Shared filesystem helpers (dedupe backup/restore copy).

use std::path::Path;

/// Recursively copy a directory tree (files + dirs). Symlinks are skipped.
pub fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&path, &target)?;
        } else if ty.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// CSV field escape: wrap in quotes when needed; double internal quotes.
pub fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn csv_escape_quotes_and_commas() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn copy_dir_roundtrip() {
        let root = std::env::temp_dir().join(format!("remova_fsutil_{}", std::process::id()));
        let src = root.join("src");
        let dest = root.join("dest");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("a.txt"), b"hello").unwrap();
        fs::write(src.join("nested/b.txt"), b"world").unwrap();
        copy_dir(&src, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(dest.join("nested/b.txt")).unwrap(),
            "world"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
