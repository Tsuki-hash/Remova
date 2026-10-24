use super::*;
use std::fs;

#[cfg(windows)]
fn fixture_junction(link: &Path, target: &Path) {
    let status = std::process::Command::new(crate::regops::sys_tool("cmd.exe"))
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(status.success(), "junction fixture creation failed");
    assert!(is_reparse_point(link));
}

#[test]
fn atomic_tmp_collisions_never_truncate_or_remove_foreign_files() {
    use std::io::Write;
    let root = unique_tmp("exclusive_tmp");
    let sentinel = root.join("sentinel");
    fs::write(&sentinel, b"original").unwrap();
    let foreign = root.join("foreign.tmp");
    fs::hard_link(&sentinel, &foreign).unwrap();
    let owned = root.join("owned.tmp");
    let mut first = true;
    let (path, mut file) = create_atomic_tmp_with(|| {
        if std::mem::take(&mut first) {
            foreign.clone()
        } else {
            owned.clone()
        }
    })
    .unwrap();
    file.write_all(b"new").unwrap();
    drop(file);
    assert_eq!(path, owned);
    assert_eq!(fs::read(&sentinel).unwrap(), b"original");
    assert_eq!(fs::read(&foreign).unwrap(), b"original");
    let error = create_atomic_tmp_with(|| foreign.clone()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(foreign.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"original");
    let destination = root.join("directory");
    fs::create_dir(&destination).unwrap();
    assert!(write_bytes_atomic(&destination, b"cannot publish").is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
    remove_tree_no_reparse(&root).unwrap();
}

#[test]
#[cfg(windows)]
fn reboot_target_pin_accepts_a_read_locked_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!(
        "remova-lock-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, b"locked").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(pin_dir_no_reparse(&path).is_err());
    let readonly = pin_target_readonly(&path);
    drop(lock);
    let pinned = readonly.unwrap();
    assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    drop(pinned);
    fs::remove_file(path).unwrap();
}

#[test]
fn local_drive_probe_rejects_network_and_device_paths() {
    assert!(super::is_local_drive_path(r"C:\Apps\Demo\demo.exe"));
    assert!(super::is_local_drive_path("d:/apps/demo"));
    assert!(!super::is_local_drive_path(r"\\server\share\demo.exe"));
    assert!(!super::is_local_drive_path(r"\\.\pipe\x"));
    assert!(!super::is_local_drive_path(r"\\?\C:\Apps\demo.exe"));
    assert!(!super::is_local_drive_path(r"C:relative\demo.exe"));
    assert!(!super::is_local_drive_path(r"C:\Apps\..\Windows\x.exe"));
}

#[test]
fn local_drive_probe_mock_rejects_mapped_network_drives_without_network_io() {
    let mut probed = Vec::new();
    let accepted = super::is_local_drive_path_with(
        r"Z:\mapped\app.exe",
        |root| root != r"Z:\",
        |path| {
            probed.push(path.to_string());
            super::PathProbe::Safe
        },
    );
    assert!(!accepted);
    assert!(
        probed.is_empty(),
        "remote drives must be rejected before probing paths"
    );
}

#[test]
fn local_drive_probe_mock_rejects_reparse_prefix_before_descending() {
    let mut probed = Vec::new();
    let accepted = super::is_local_drive_path_with(
        r"C:\Apps\redirect\remote.exe",
        |_| true,
        |path| {
            probed.push(path.to_string());
            if path.eq_ignore_ascii_case(r"C:\Apps\redirect") {
                super::PathProbe::Reparse
            } else {
                super::PathProbe::Safe
            }
        },
    );
    assert!(!accepted);
    assert_eq!(probed, [r"C:\Apps", r"C:\Apps\redirect"]);
}

#[test]
fn local_drive_probe_mock_accepts_local_volume_and_missing_tail() {
    let mut probed = Vec::new();
    let accepted = super::is_local_drive_path_with(
        r"C:\Apps\not-yet-created\app.exe",
        |root| root == r"C:\",
        |path| {
            probed.push(path.to_string());
            if path.ends_with("not-yet-created") {
                super::PathProbe::Missing
            } else {
                super::PathProbe::Safe
            }
        },
    );
    assert!(accepted);
    assert_eq!(probed, [r"C:\Apps", r"C:\Apps\not-yet-created"]);
}

#[test]
fn atomic_write_publishes_and_leaves_no_tmp() {
    let dir = std::env::temp_dir().join(format!("remova-r25-fsutil-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let target = dir.join("data.bin");
    write_bytes_atomic(&target, b"one").unwrap();
    write_bytes_atomic(&target, b"two").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"two");
    let leftovers = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0, "failed-or-done writes must not leave tmp");
    // The tmp helper is unique per call even for the same target.
    assert_ne!(atomic_tmp_path(&target), atomic_tmp_path(&target));
    let _ = fs::remove_dir_all(&dir);
}

/// A fixture directory that removes itself on drop, including on panic.
fn unique_tmp(tag: &str) -> crate::test_support::TempDir {
    crate::test_support::TempDir::new(&format!("fsutil_{tag}"))
}

/// `path_key` is the one definition of path identity shared by the allow-list,
/// the PATH-entry comparison and the red-line checks. Each case below is a pair
/// that Win32 resolves to the same object but that the four hand-rolled
/// normalizers used to treat as different keys.
#[test]
fn path_key_agrees_with_win32_on_the_same_object() {
    let key = |s: &str| path_key(std::path::Path::new(s));
    // Case and slash style.
    assert_eq!(key("C:/Foo/Bar").as_deref(), key("c:\\foo\\bar").as_deref());
    // A trailing separator names the same directory.
    assert_eq!(
        key("C:\\Foo\\Bar\\").as_deref(),
        key("C:\\Foo\\Bar").as_deref()
    );
    // Win32 strips trailing dots and spaces per segment, not just at the end.
    assert_eq!(
        key("C:\\Foo\\Bar.").as_deref(),
        key("C:\\Foo\\Bar").as_deref()
    );
    assert_eq!(
        key("C:\\Foo\\Bar ").as_deref(),
        key("C:\\Foo\\Bar").as_deref()
    );
    assert_eq!(
        key("C:\\Foo. \\Bar. ").as_deref(),
        key("C:\\Foo\\Bar").as_deref()
    );
    // Surrounding whitespace and the quotes a registry PATH value arrives with.
    assert_eq!(
        key("  \"C:\\Foo\\Bar\"  ").as_deref(),
        key("C:\\Foo\\Bar").as_deref()
    );
    // A drive root keeps its colon; it is not collapsed to the empty key.
    assert_eq!(key("C:\\").as_deref(), Some("c:"));
}

/// The dot-segment exemption exists so traversal rejection still works — a key
/// that quietly turned `..` into an empty segment would make `C:\A\..\..\B`
/// look like `C:\A\B`.
#[test]
fn path_key_keeps_dot_segments_for_traversal_checks() {
    let key = |s: &str| path_key(std::path::Path::new(s));
    assert_eq!(key("C:\\A\\..\\..\\B").as_deref(), Some("c:\\a\\..\\..\\b"));
    assert_ne!(
        key("C:\\A\\..\\..\\B").as_deref(),
        key("C:\\A\\B").as_deref()
    );
}

/// Distinct paths must stay distinct: the exemptions above must not over-merge.
#[test]
fn path_key_does_not_merge_distinct_paths() {
    let key = |s: &str| path_key(std::path::Path::new(s));
    assert_ne!(
        key("C:\\Foo\\Bar").as_deref(),
        key("C:\\Foo\\Bar2").as_deref()
    );
    assert_ne!(
        key("C:\\Foo\\Bar").as_deref(),
        key("C:\\Foo\\Baz").as_deref()
    );
    assert_ne!(key("C:\\Foo").as_deref(), key("C:\\Foo\\Bar").as_deref());
}

/// A path that cannot be named cannot be matched, so the caller fails closed
/// rather than comparing on a lossy replacement.
#[test]
fn path_key_is_none_for_unnameable_and_empty_input() {
    use std::os::windows::ffi::OsStringExt;
    let lone_surrogate: std::ffi::OsString = std::ffi::OsString::from_wide(&[0xD800, b'x' as u16]);
    assert!(path_key(std::path::Path::new(&lone_surrogate)).is_none());
    assert!(path_key(std::path::Path::new("")).is_none());
    assert!(path_key(std::path::Path::new("   ")).is_none());
    assert!(path_key(std::path::Path::new("\"\"")).is_none());
}

#[test]
fn copy_file_no_reparse_refuses_missing_is_ok_error() {
    let tmp = unique_tmp("nofile");
    let missing = tmp.join("nope.bin");
    assert!(copy_file_no_reparse(&missing, &tmp.join("out.bin")).is_err());
    let _ = fs::remove_dir_all(&tmp);
}

/// A failing mid-stream copy must never truncate or replace the destination.
#[test]
fn copy_stream_failure_preserves_dest_and_leaves_no_tmp() {
    struct FailAfter {
        left: usize,
    }
    impl std::io::Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Err(std::io::Error::other("injected mid-copy failure"));
            }
            let n = self.left.min(buf.len()).min(4);
            buf[..n].fill(b'N');
            self.left -= n;
            Ok(n)
        }
    }
    let tmp = unique_tmp("atomic_copy");
    let dest = tmp.join("out.bin");
    fs::write(&dest, b"ORIGINAL").unwrap();
    let err = copy_stream_atomic(FailAfter { left: 8 }, &dest).unwrap_err();
    assert!(err.to_string().contains("injected"), "{err}");
    assert_eq!(
        fs::read(&dest).unwrap(),
        b"ORIGINAL",
        "dest must stay intact"
    );
    let leftovers: Vec<_> = fs::read_dir(&tmp)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "no tmp leftovers: {leftovers:?}");
    let _ = fs::remove_dir_all(&tmp);
}

/// Successful replace is atomic: dest gets the new bytes, no tmp remains.
#[test]
fn copy_stream_success_replaces_dest_without_tmp() {
    let tmp = unique_tmp("atomic_copy_ok");
    let dest = tmp.join("out.bin");
    fs::write(&dest, b"OLD").unwrap();
    let n = copy_stream_atomic(&b"NEWER"[..], &dest).unwrap();
    assert_eq!(n, 5);
    assert_eq!(fs::read(&dest).unwrap(), b"NEWER");
    assert!(!tmp.join("out.bin.removal-tmp").exists());
    assert!(!tmp.join("out.bin.removal-tmp").exists());
    let leftovers: Vec<_> = fs::read_dir(&tmp)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "no tmp leftovers: {leftovers:?}");
    let _ = fs::remove_dir_all(&tmp);
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows symlink privilege or Developer Mode; run explicitly on the Windows CI runner"]
fn copy_dir_refuses_file_swapped_to_symlink_after_enumeration() {
    let tmp = unique_tmp("swap_file");
    let source = tmp.join("source");
    let dest = tmp.join("dest");
    fs::create_dir(&source).unwrap();
    let secret = tmp.join("secret.bin");
    fs::write(&secret, b"must never be copied").unwrap();
    let child = source.join("child.bin");
    fs::write(&child, b"safe").unwrap();
    let mut swapped = false;
    let result = copy_dir_before_file(&source, &dest, &mut |path| {
        fs::remove_file(path)?;
        std::os::windows::fs::symlink_file(&secret, path)?;
        swapped = true;
        Ok(())
    });
    // No skip/return: an unavailable fixture is a visible failed test.
    assert!(
        swapped,
        "symlink fixture requires developer mode or symlink privilege: {result:?}"
    );
    assert!(result.is_err());
    assert!(!dest.join("child.bin").exists());
    assert_eq!(fs::read(&secret).unwrap(), b"must never be copied");
    fs::remove_file(child).unwrap();
    fs::remove_file(secret).unwrap();
    fs::remove_dir(source).unwrap();
    fs::remove_dir(dest).unwrap();
    fs::remove_dir(tmp).unwrap();
}

#[test]
fn remove_tree_no_reparse_refuses_root_reparse() {
    let tmp = unique_tmp("rm_reparse");
    let link = tmp.join("link");
    let target = tmp.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("f.txt"), b"x").unwrap();
    fixture_junction(&link, &target);
    {
        assert!(remove_tree_no_reparse(&link).is_err());
        assert!(
            target.join("f.txt").exists(),
            "must not delete through link"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

/// the pin itself must refuse reparse points and actually pin
/// (rename of a pinned directory fails; after drop it succeeds again).
#[test]
fn dir_pin_refuses_reparse_and_blocks_rename() {
    let tmp = unique_tmp("pin");
    let dir = tmp.join("d");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.txt"), b"a").unwrap();

    let pin = pin_dir_no_reparse(&dir).unwrap();
    #[cfg(windows)]
    {
        assert!(
            fs::rename(&dir, tmp.join("d2")).is_err(),
            "pinned dir must not be renameable (no DELETE share)"
        );
    }
    drop(pin);
    #[cfg(windows)]
    fs::rename(&dir, tmp.join("d2")).unwrap();

    let target = tmp.join("t2");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    let junc = tmp.join("junc");
    fixture_junction(&junc, &target);
    {
        assert!(pin_dir_no_reparse(&junc).is_err());
        assert!(target.join("keep.txt").exists());
    }
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn remove_tree_no_reparse_unlinks_child_junction_without_following() {
    let tmp = unique_tmp("rm_child");
    let tree = tmp.join("tree");
    let outside = tmp.join("outside");
    fs::create_dir_all(tree.join("ok")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), b"keep").unwrap();
    fs::write(tree.join("ok").join("a.txt"), b"a").unwrap();
    let junc = tree.join("junc");
    fixture_junction(&junc, &outside);
    remove_tree_no_reparse(&tree).unwrap();
    assert!(!tree.exists());
    assert!(
        outside.join("keep.txt").exists(),
        "junction target must survive"
    );
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn copy_dir_skips_reparse_entries() {
    let tmp = unique_tmp("link");
    let src = tmp.join("src");
    let dest = tmp.join("dest");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("ok.txt"), b"x").unwrap();
    let outside = tmp.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"outside").unwrap();
    fixture_junction(&src.join("link.txt"), &outside);
    copy_dir(&src, &dest).unwrap();
    assert!(dest.join("ok.txt").exists());
    assert!(!dest.join("link.txt").exists());
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn fnv1a64_stable() {
    assert_eq!(fnv1a64("hello"), fnv1a64("hello"));
    assert_ne!(fnv1a64("a"), fnv1a64("b"));
}

#[test]
fn csv_escape_quotes_and_commas() {
    assert_eq!(csv_escape("plain"), "plain");
    assert_eq!(csv_escape("a,b"), "\"a,b\"");
    assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    // OWASP CSV injection: formula prefixes are neutralized.
    assert_eq!(csv_escape("=cmd|' /C calc'!A0"), "'=cmd|' /C calc'!A0");
    assert_eq!(csv_escape("+sum"), "'+sum");
    assert_eq!(csv_escape("-flag"), "'-flag");
    assert_eq!(csv_escape("@import"), "'@import");
    assert_eq!(csv_escape("\t=1"), "'\t=1");
    assert_eq!(csv_escape("\r=1+1"), "\"'\r=1+1\"");
    assert_eq!(csv_escape("\n=1+1"), "\"'\n=1+1\"");
    // Quoted fields keep the apostrophe inside the cell value.
    assert_eq!(csv_escape("=a,\"b"), "\"'=a,\"\"b\"");
    // Ordinary names (CJK, digits, drive paths) are untouched.
    assert_eq!(csv_escape("7-Zip 压缩"), "7-Zip 压缩");
    assert_eq!(csv_escape(r"C:\Users\a\b"), r"C:\Users\a\b");
}

#[test]
fn path_utf8_fails_closed_on_wide_junk() {
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let wide: Vec<u16> = vec!['C' as u16, ':' as u16, 0xD800, 'x' as u16];
        let os = OsString::from_wide(&wide);
        assert!(path_utf8(Path::new(&os)).is_none());
    }
    assert_eq!(
        path_utf8(Path::new(r"C:\Foo/Bar")).as_deref(),
        Some(r"C:\Foo\Bar")
    );
}

#[test]
fn copy_dir_roundtrip() {
    let root = unique_tmp("rt");
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
/// M2 (SOP §8-22): only empty, matching vendor/product shells below a known
/// root are removed; the root and sibling directories survive.
#[test]
fn prune_removes_only_empty_matching_shells() {
    let root = unique_tmp("prune");
    let root_key = PathBuf::from(path_key(root.path()).unwrap());
    let roots = [root_key.clone()];
    let vendor = root.join("Vendor");
    let app = vendor.join("App");
    std::fs::create_dir_all(app.join("bin")).unwrap();
    std::fs::write(app.join("bin").join("a.bin"), b"x").unwrap();
    std::fs::create_dir_all(root.join("Unrelated").join("App")).unwrap();

    std::fs::remove_dir_all(&app).unwrap();
    let removed = prune_empty_ancestors_under(&app, &["Vendor".into()], &roots);
    assert_eq!(removed, vec![vendor.clone()]);
    assert!(!vendor.exists(), "the empty vendor shell must be gone");
    assert!(root.exists(), "the known root must survive");

    let other = root.join("Unrelated").join("App");
    std::fs::remove_dir_all(&other).unwrap();
    let removed = prune_empty_ancestors_under(&other, &["Vendor".into()], &roots);
    assert!(removed.is_empty(), "a non-matching name must not be pruned");
    assert!(root.join("Unrelated").exists());
}

#[test]
fn attribute_retry_is_limited_to_access_denied_with_protected_attributes() {
    assert!(super::should_clear_attributes(
        &std::io::Error::from_raw_os_error(5),
        0x1
    ));
    assert!(super::should_clear_attributes(
        &std::io::Error::from_raw_os_error(5),
        0x2 | 0x4
    ));
    assert!(!super::should_clear_attributes(
        &std::io::Error::from_raw_os_error(32),
        0x1
    ));
    assert!(!super::should_clear_attributes(
        &std::io::Error::from_raw_os_error(5),
        0
    ));
    assert!(!super::should_clear_attributes(
        &std::io::Error::from_raw_os_error(5),
        0x400 | 0x1
    ));
}

/// M4 (SOP §9.3): READONLY/HIDDEN/SYSTEM is an attribute, not a lock —
/// the delete path clears it and retries once.
#[cfg(windows)]
#[test]
fn readonly_tree_is_removed_after_clearing_attributes() {
    let root = unique_tmp("readonly");
    let tree = root.join("tree");
    let inner = tree.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(tree.join("a.bin"), b"a").unwrap();
    std::fs::write(inner.join("b.bin"), b"b").unwrap();
    for p in [tree.join("a.bin"), inner.join("b.bin"), inner.clone()] {
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&p, perm).unwrap();
    }
    remove_tree_no_reparse(&tree).expect("readonly attributes must not block deletion");
    assert!(!tree.exists());
}

/// The root-skip rule is a pure decision, so it can be tested without
/// pinning a real volume root.
#[test]
fn target_can_be_marked_covers_roots_and_unc_shares() {
    assert!(!target_can_be_marked(std::path::Path::new(r"C:\")));
    assert!(!target_can_be_marked(std::path::Path::new("C:/")));
    assert!(!target_can_be_marked(std::path::Path::new(
        r"\\server\share"
    )));
    assert!(target_can_be_marked(std::path::Path::new(r"C:\Temp")));
    assert!(target_can_be_marked(std::path::Path::new(
        r"C:\Temp\nested"
    )));
    assert!(target_can_be_marked(std::path::Path::new("relative")));
}

/// A10 / A4: the marker name is bounded, unique per attempt, and still
/// parses as a hidden file name.
#[cfg(windows)]
#[test]
fn guard_marker_names_are_short_and_distinct() {
    let names: Vec<String> = (0..3).map(guard_marker_name).collect();
    assert_eq!(names.len(), 3);
    assert_ne!(names[0], names[1]);
    assert_ne!(names[1], names[2]);
    for name in &names {
        assert!(name.starts_with(".remova-guard-"));
        assert!(name.encode_utf16().count() < 255);
    }
}

/// A2: raw CreateFileW does not get std's verbatim conversion. Long
/// absolute paths must be promoted so a valid deep destination can still
/// be pinned; short and relative paths must keep their ordinary form.
#[cfg(windows)]
#[test]
fn wide_path_promotes_only_long_absolute_paths() {
    let short = wide_path_for_create_file(r"C:\Temp\x");
    assert_eq!(
        String::from_utf16(&short[..short.len() - 1]).unwrap(),
        r"C:\Temp\x"
    );

    let long_drive = format!(r"C:\{}", "a".repeat(260));
    let wide = wide_path_for_create_file(&long_drive);
    let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
    assert!(text.starts_with(r"\\?\C:\"), "{text}");

    let long_unc = format!(r"\\server\share\{}", "a".repeat(260));
    let wide = wide_path_for_create_file(&long_unc);
    let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
    assert!(text.starts_with(r"\\?\UNC\server\share\"), "{text}");

    let relative = format!(r"rel\{}", "a".repeat(260));
    let wide = wide_path_for_create_file(&relative);
    let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
    assert_eq!(text, relative.replace('/', "\\"));
}

/// A2 e2e: a destination deeper than MAX_PATH must pin successfully, and
/// its target must carry the marker.
#[cfg(windows)]
#[test]
fn create_dirs_pinned_supports_paths_beyond_max_path() {
    let root = unique_tmp("long_path");
    let deep = root
        .join("a".repeat(140))
        .join("b".repeat(140))
        .join("target");
    assert!(deep.to_string_lossy().len() > 260);
    let pins = create_dirs_pinned(&deep).unwrap();
    assert!(
        std::fs::read_dir(&deep)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(".remova-guard")),
        "the deep target must be reachable and marked"
    );
    drop(pins);
}

/// regression fix: only the operation's own target carries a guard marker.
/// A crash mid-operation must never leave `.remova-guard-*` in `C:\` or
/// `%ProgramData%`, and the ancestor chain must stay marker-free.
#[cfg(windows)]
#[test]
fn create_dirs_pinned_marks_only_the_target() {
    let root = unique_tmp("guard_ancestors");
    let deep = root.join("a").join("b").join("target");
    let pins = create_dirs_pinned(&deep).unwrap();
    let has_guard = |dir: &std::path::Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(".remova-guard"))
    };
    assert!(has_guard(&deep), "the target must carry the marker");
    for ancestor in [root.join("a"), root.join("a").join("b")] {
        assert!(
            !has_guard(&ancestor),
            "an ancestor must never be marked: {}",
            ancestor.display()
        );
    }
    drop(pins);
    assert!(!has_guard(&deep), "the marker must disappear on close");
}

#[cfg(windows)]
#[test]
fn directory_pins_block_reparse_mutation_but_allow_child_io() {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    fn make_junction(path: &Path, target: &Path) -> windows::core::Result<()> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x0200_0000)
            .open(path)
            .unwrap();
        let substitute: Vec<u16> = format!("\\??\\{}", target.display())
            .encode_utf16()
            .collect();
        let print: Vec<u16> = target.to_str().unwrap().encode_utf16().collect();
        let mut buffer = 0xa000_0003u32.to_le_bytes().to_vec();
        buffer.extend_from_slice(
            &((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes(),
        );
        buffer.extend_from_slice(&0u16.to_le_bytes());
        for value in [
            0,
            substitute.len() * 2,
            (substitute.len() + 1) * 2,
            print.len() * 2,
        ] {
            buffer.extend_from_slice(&(value as u16).to_le_bytes());
        }
        for unit in substitute
            .into_iter()
            .chain(Some(0))
            .chain(print)
            .chain(Some(0))
        {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
        let mut written = 0;
        unsafe {
            windows::Win32::System::IO::DeviceIoControl(
                windows::Win32::Foundation::HANDLE(file.as_raw_handle()),
                0x0009_00a4,
                Some(buffer.as_ptr().cast()),
                buffer.len() as u32,
                None,
                0,
                Some(&mut written),
                None,
            )
        }
    }
    let root = unique_tmp("directory_write_pin");
    let target = root.join("target");
    let control = root.join("control");
    let guarded = root.join("guarded");
    fs::create_dir(&target).unwrap();
    fs::create_dir(&control).unwrap();
    fs::create_dir(&guarded).unwrap();
    // First establish a real unprivileged in-place junction mutation fixture.
    make_junction(&control, &target).expect("control junction mutation");
    assert!(is_reparse_point(&control));
    let pins = create_dirs_pinned(&guarded).unwrap();
    let mutation = make_junction(&guarded, &target);
    assert!(
        mutation.is_err(),
        "a pinned directory must refuse reparse mutation: {mutation:?}"
    );
    assert!(!is_reparse_point(&guarded));
    fs::create_dir(guarded.join("child")).unwrap();
    fs::write(guarded.join("child/file"), b"child IO is allowed").unwrap();
    fs::rename(guarded.join("child/file"), guarded.join("child/moved")).unwrap();
    fs::remove_file(guarded.join("child/moved")).unwrap();
    fs::remove_dir(guarded.join("child")).unwrap();
    drop(pins);
    assert_eq!(
        fs::read_dir(&guarded).unwrap().count(),
        0,
        "guard must disappear on close"
    );
    fs::write(guarded.join("existing"), b"keep directory nonempty").unwrap();
    let pins = create_dirs_pinned(&guarded).unwrap();
    fs::remove_file(guarded.join("existing")).unwrap();
    assert!(make_junction(&guarded, &target).is_err());
    drop(pins);
    let copied = root.join("copied");
    copy_dir(&guarded, &copied).unwrap();
    assert_eq!(fs::read_dir(&guarded).unwrap().count(), 0);
    assert_eq!(
        fs::read_dir(&copied).unwrap().count(),
        0,
        "guard must never enter a backup"
    );
    fs::remove_dir(control).unwrap();
    fs::remove_dir_all(root).unwrap();
}
#[cfg(windows)]
#[test]
fn output_directory_pins_refuse_junction_ancestors_before_creating_children() {
    let root = unique_tmp("output_pins");
    let target = root.join("target");
    let link = root.join("link");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), b"unchanged").unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(create_dirs_pinned(&link.join("child")).is_err());
    assert!(remove_tree_no_reparse(&link.join("keep")).is_err());
    assert!(!crate::sysops::schedule_delete_on_reboot(
        link.join("keep").to_str().unwrap()
    ));
    assert!(!target.join("child").exists());
    assert_eq!(fs::read(target.join("keep")).unwrap(), b"unchanged");
    // Unlink only the fixture junction before recursively removing its sandbox.
    fs::remove_dir(link).unwrap();
    fs::remove_dir_all(root).unwrap();
}
/// the restore write-back relies on `pin_dir_resolved` to expose
/// junction redirects — regressions here would silently reintroduce the
/// planted-junction write path. Junctions need no admin rights.
#[cfg(windows)]
#[test]
fn pin_dir_resolved_resolves_junction_redirect() {
    let base = unique_tmp("pinresolved");
    let real = base.join("real");
    fs::create_dir_all(&real).unwrap();
    // CI's TEMP can contain RUNNER~1 even when our leaf is a long name.
    // Compare to the expanded request, not the environment's short spelling.
    let expected = long_path_form(&real).expect("expand real directory name");

    // A real directory resolves to itself.
    let pin = pin_dir_resolved(&real).expect("pin real dir");
    assert_eq!(
        pin.final_path().replace('/', "\\").to_lowercase(),
        expected.replace('/', "\\").to_lowercase()
    );
    drop(pin);

    let link = base.join("link");
    let st = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .status()
        .expect("mklink /J");
    assert!(st.success(), "mklink /J failed");

    // The junction path resolves to the TARGET, never to itself.
    let pin2 = pin_dir_resolved(&link).expect("pin junction");
    assert_eq!(
        pin2.final_path().to_lowercase(),
        expected.to_lowercase(),
        "final path must resolve through the junction"
    );
    assert_ne!(
        pin2.final_path().to_lowercase(),
        link.to_string_lossy().to_lowercase()
    );
    drop(pin2);
    fs::remove_dir_all(&base).unwrap();
}
