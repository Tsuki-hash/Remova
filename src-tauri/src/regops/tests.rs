#[test]
fn path_presence_requires_complete_reads_only_to_prove_absence() {
    assert!(super::path_entry_presence_with("fixture", |_| Err("denied".into())).is_err());
    assert!(super::path_entry_presence_with("fixture", |scope| {
        if scope == "User" {
            Err("denied".into())
        } else {
            Ok("fixture".into())
        }
    })
    .unwrap());
    assert!(!super::path_entry_presence_with("missing", |_| Ok("fixture".into())).unwrap());
}
#[test]
fn task_delete_rejects_wildcards_before_launching_native_tool() {
    for name in [
        "*",
        r"\*",
        r"\Vendor\*",
        "?",
        "",
        " ",
        "bad\0name",
        "bad\nname",
    ] {
        assert!(!super::schtasks_delete(name));
    }
}
#[test]
fn path_partial_failure_preserves_successful_user_write() {
    let _lock = super::path_mock::lock_mock();
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            super::path_mock::clear();
        }
    }
    let _reset = Reset;
    super::path_mock::install(r"C:\Vendor\bin", r"C:\Vendor\bin");
    super::path_mock::fail_machine_write();
    assert!(super::scrub_path_entry(r"C:\Vendor\bin")
        .unwrap_err()
        .contains("Machine"));
    assert_eq!(super::path_mock::get("User"), "");
    assert_eq!(super::path_mock::get("Machine"), r"C:\Vendor\bin");
    super::path_mock::install("", "");
    assert!(
        super::restore_path_entry(r"C:\Vendor\bin", &["User", "Machine"])
            .unwrap_err()
            .contains("Machine")
    );
    assert_eq!(super::path_mock::get("User"), r"C:\Vendor\bin");
    assert_eq!(super::path_mock::get("Machine"), "");
}

#[cfg(windows)]
#[test]
#[ignore = "writes real startup entries; requires REMOVA_TEST_ALLOW_SYS_MUTATION=1 and --ignored"]
fn rename_value_refuses_existing_destination_without_changing_either() {
    assert_eq!(
        std::env::var("REMOVA_TEST_ALLOW_SYS_MUTATION").as_deref(),
        Ok("1"),
        "REMOVA_TEST_ALLOW_SYS_MUTATION=1 is required before writing real startup entries"
    );
    const KEY: &str = r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    struct Cleanup(Vec<String>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for name in &self.0 {
                let _ = super::delete_value(KEY, name);
            }
        }
    }
    let prefix = format!(
        "RemovaRenameTest_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let from = format!("{prefix}.remova-disabled");
    let to = prefix;
    let _cleanup = Cleanup(vec![from.clone(), to.clone()]);
    super::create_reg_sz(KEY, &from, "old command").unwrap();
    super::create_reg_sz(KEY, &to, "new command").unwrap();
    let before_from = super::read_reg_value_bytes(KEY, &from).unwrap();
    let before_to = super::read_reg_value_bytes(KEY, &to).unwrap();
    assert!(super::rename_reg_value(KEY, &from, &to).is_err());
    assert_eq!(
        super::read_reg_value_bytes(KEY, &from).unwrap(),
        before_from
    );
    assert_eq!(super::read_reg_value_bytes(KEY, &to).unwrap(), before_to);
    super::delete_value(KEY, &to).unwrap();
    super::rename_reg_value(KEY, &from, &to).unwrap();
    assert!(super::read_reg_value_bytes(KEY, &from).unwrap().is_none());
    assert_eq!(super::read_reg_value_bytes(KEY, &to).unwrap(), before_from);
}
#[test]
fn parse_reg_query_multi_line_binary_and_name_with_reg_token() {
    // Long REG_BINARY data wraps onto continuation lines; a value NAME may
    // itself contain "REG_" — the known-type token wins, continuations join.
    let out = concat!(
        "HKEY_CURRENT_USER\\Software\\T\r\n",
        "\r\n",
        "    My REG_Thing    REG_BINARY    0A0B\r\n",
        "        0C0D0E0F\r\n",
        "\r\n",
        "HKEY_CURRENT_USER\\Software\\T2\r\n",
        "\r\n",
        "    Other    REG_SZ    hello\r\n",
    );
    let (ty, data) = super::parse_reg_query(out).unwrap();
    assert_eq!(ty, "REG_BINARY");
    assert_eq!(data, "0A0B 0C0D0E0F");
    // Single-line REG_SZ still parses; the value name is ignored.
    let (ty, data) = super::parse_reg_query("    Path    REG_SZ    C:\\x y\\z.exe\r\n").unwrap();
    assert_eq!(ty, "REG_SZ");
    assert_eq!(data, "C:\\x y\\z.exe");
}

#[test]
fn parse_reg_query_no_type_is_none() {
    assert!(
        super::parse_reg_query("HKEY_CURRENT_USER\\Software\\T\r\n\r\n    junk line\r\n").is_none()
    );
}

#[test]
fn fs_rejects_drive_root() {
    assert_eq!(
        super::normalize_reg_exe_hive(r"HKLM64\SOFTWARE\Foo"),
        r"HKLM\SOFTWARE\Foo"
    );
    assert_eq!(
        super::normalize_reg_exe_hive(r"HKLM32\SOFTWARE\Foo"),
        r"HKLM\SOFTWARE\WOW6432Node\Foo"
    );
    assert_eq!(
        super::normalize_reg_exe_hive(r"HKCU\Software\Bar"),
        r"HKCU\Software\Bar"
    );
    assert_eq!(
        super::normalize_reg_exe_hive(r"HKLM32\SOFTWARE\WOW6432Node\Foo"),
        r"HKLM\SOFTWARE\WOW6432Node\Foo"
    );
    assert_eq!(
        super::normalize_reg_exe_hive(r"HKLM32\SYSTEM\Vendor"),
        r"HKLM\SYSTEM\Vendor"
    );
}

#[test]
fn normalize_path_entry_strips_quotes_and_slash() {
    assert_eq!(
        super::normalize_path_entry(r#""C:\Python3\\"#).as_deref(),
        Some("c:\\python3")
    );
    assert_eq!(
        super::normalize_path_entry("C:/Python3/").as_deref(),
        Some("c:\\python3")
    );
}

/// The PATH comparison now shares `fsutil::path_key` with the allow-lists and
/// the red-line checks, so it must agree with Win32 on the extra shapes the
/// four hand-rolled normalizers used to miss.
#[test]
fn normalize_path_entry_matches_win32_segment_trimming() {
    // Trailing dots and spaces are stripped per segment, not just at the end.
    assert_eq!(
        super::normalize_path_entry(r"C:\Python3.").as_deref(),
        Some("c:\\python3")
    );
    assert_eq!(
        super::normalize_path_entry(r"C:\Python3\Scripts ").as_deref(),
        Some("c:\\python3\\scripts")
    );
    // An entry that cannot be named never matches.
    assert_eq!(super::normalize_path_entry("   "), None);
}

#[test]
fn scrub_uses_exact_segment_match() {
    // C:\Python3 must not equal C:\Python312
    let a = super::normalize_path_entry(r"C:\Python3");
    let b = super::normalize_path_entry(r"C:\Python312");
    let c = super::normalize_path_entry(r"C:\Python3\Scripts");
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(a, super::normalize_path_entry(r"C:\Python3\"));
}

/// `path_contains_entry` now shares `fsutil::path_key` with the
/// allow-lists and the red lines. A PATH value carrying Win32's trailing
/// dot/space decoration must still be recognized as the entry the scanner
/// reported, or the verify step reports a scrub that actually succeeded.
#[test]
fn path_contains_entry_matches_win32_decorated_segments() {
    assert!(super::path_contains_entry(
        r"C:\Vendor\Tool.;C:\Windows",
        r"C:\Vendor\Tool"
    ));
    assert!(super::path_contains_entry(
        r"C:\Vendor\Tool ;C:\Windows",
        r"c:/vendor/tool"
    ));
    assert!(super::path_contains_entry(
        r#"C:\Vendor\Tool;C:\Windows"#,
        r#"  "C:\Vendor\Tool"  "#
    ));
    assert!(!super::path_contains_entry(
        r"C:\Vendor\Tool2",
        r"C:\Vendor\Tool"
    ));
    assert!(!super::path_contains_entry(r"C:\Windows", "   "));
}

#[test]
fn delete_key_intrinsic_gate_blocks_protected() {
    // write primitive refuses protected trees even without caller checks.
    assert!(super::delete_key(r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend").is_err());
    assert!(super::delete_key(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run").is_err());
    assert!(
        super::delete_key(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall").is_err()
    );
}

/// A trailing separator makes the effective leaf empty — RegDeleteTreeW
/// with an empty subkey would wipe the PARENT's contents. The primitive
/// must refuse the shape before any registry access, and the key gate
/// must refuse root shapes with a trailing separator (an allow arm
/// matched `...\Uninstall\` as a prefix before the trim was added).
#[test]
fn delete_key_refuses_trailing_separator_shapes() {
    assert!(super::delete_key(r"HKCU\Software\RemovaShapeTest\").is_err());
    // Gate level: every root family must refuse the trailing-separator form.
    for shape in [
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\",
        r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\",
        r"HKLM\SYSTEM\CurrentControlSet\Services\",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\",
    ] {
        assert!(
            crate::safety::is_safe_to_delete_registry(shape).is_err(),
            "root with trailing separator must be refused: {shape}"
        );
    }
}

#[test]
fn scrub_path_entry_intrinsic_gate_blocks_system() {
    // PATH scrub refuses system segments even without caller checks.
    assert!(super::scrub_path_entry(r"C:\Windows\System32").is_err());
    assert!(super::scrub_path_entry(r"C:\Windows").is_err());
}

/// Non-ASCII REG_SZ data must survive export byte-exact: reg.exe text
/// output is OEM-codepage encoded and used to bake mojibake into the
/// seal-digested value.reg (the only authorized restore source).
#[cfg(windows)]
#[test]
fn native_path_roundtrip_handles_absence_empty_and_type() {
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new(super::sys_tool("reg.exe"))
                .args(["delete", &self.0, "/f"])
                .output();
        }
    }
    let key = format!(
        r"HKCU\Software\RemovaPathTest_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _cleanup = Cleanup(key.clone());
    assert_eq!(
        super::read_reg_value_raw_with_missing(&key, "Path", true).unwrap(),
        (String::new(), true)
    );
    for (value, expand) in [
        ("%LOCALAPPDATA%\\工具", true),
        ("", true),
        ("", false),
        ("C:\\Vendor", false),
    ] {
        super::write_path_value(&key, value, expand).unwrap();
        assert_eq!(
            super::read_reg_value_raw(&key, "Path").unwrap(),
            (value.to_string(), expand)
        );
    }
    assert_eq!(
        super::read_reg_value_raw_with_missing(&key, "Absent", true).unwrap(),
        (String::new(), true)
    );
    assert!(super::write_path_value("invalid", "data", false).is_err());
    assert!(super::write_path_value(&key, "a\0b", false).is_err());
    let status = std::process::Command::new(super::sys_tool("reg.exe"))
        .args([
            "add",
            &key,
            "/v",
            "Path",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(super::read_reg_value_raw(&key, "Path").is_err());
}

#[cfg(windows)]
#[test]
fn export_reg_value_preserves_non_ascii_bytes() {
    struct RegistryKeyCleanup(String);
    impl Drop for RegistryKeyCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new(super::sys_tool("reg.exe"))
                .args(["delete", &self.0, "/f"])
                .output();
        }
    }

    let key = format!(
        r"HKCU\Software\RemovaExportTest_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _cleanup = RegistryKeyCleanup(key.clone());
    let tmp = std::env::temp_dir().join(format!(
        "remova_value_reg_cjk_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let dest = tmp.join("value.reg");
    let add = std::process::Command::new(super::sys_tool("reg.exe"))
        .arg("add")
        .arg(&key)
        .args([
            "/v",
            "Path",
            "/t",
            "REG_SZ",
            "/d",
            r"C:\Users\张三\工具",
            "/f",
        ])
        .output()
        .unwrap();
    assert!(add.status.success(), "fixture key write failed");
    let exported = super::export_reg_value(&key, "Path", &dest).unwrap();
    assert!(exported, "test key must exist for the export");
    let text = crate::restore::decode_reg_text(std::fs::read(&dest).unwrap()).unwrap();
    let payload = text
        .split("=hex(1):")
        .nth(1)
        .expect("REG_SZ must export as hex(1)");
    let bytes: Vec<u8> = payload
        .split(',')
        .filter_map(|b| u8::from_str_radix(b.trim(), 16).ok())
        .collect();
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let decoded = String::from_utf16(&wide)
        .expect("payload must decode cleanly")
        .trim_end_matches('\0')
        .to_string();
    assert_eq!(decoded, r"C:\Users\张三\工具");
    assert!(
        !decoded.contains('\u{FFFD}'),
        "no replacement chars allowed"
    );
    let reg = super::sys_tool("reg.exe");
    let run = |args: &[&str]| {
        let output = std::process::Command::new(&reg)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "registry fixture operation failed: {output:?}"
        );
    };
    run(&[
        "add",
        &key,
        "/v",
        "Tool",
        "/t",
        "REG_SZ",
        "/d",
        "untouched",
        "/f",
    ]);
    for name in [" Tool ", r"Tool\Sub"] {
        use windows::Win32::System::Registry::{RegSetValueExW, REG_SZ};
        let (hive, sub, view) = super::parse(&key).unwrap();
        let sub = super::to_wide(&sub);
        let name_w = super::to_wide(name);
        let data: Vec<u8> = super::to_wide("original")
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let mut hk = super::HKEY::default();
        unsafe {
            super::RegOpenKeyExW(
                hive,
                super::PCWSTR(sub.as_ptr()),
                0,
                super::KEY_SET_VALUE | view,
                &mut hk,
            )
            .ok()
            .unwrap();
            RegSetValueExW(hk, super::PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(&data))
                .ok()
                .unwrap();
            let _ = super::RegCloseKey(hk);
        }
        let original = super::read_reg_value_bytes(&key, name).unwrap().unwrap();
        assert!(super::export_reg_value(&key, name, &dest).unwrap());
        unsafe {
            super::RegOpenKeyExW(
                hive,
                super::PCWSTR(sub.as_ptr()),
                0,
                super::KEY_SET_VALUE | view,
                &mut hk,
            )
            .ok()
            .unwrap();
            super::RegDeleteValueW(hk, super::PCWSTR(name_w.as_ptr()))
                .ok()
                .unwrap();
            let _ = super::RegCloseKey(hk);
        }
        run(&["import", dest.to_str().unwrap(), "/reg:64"]);
        assert_eq!(
            super::read_reg_value_bytes(&key, name).unwrap().unwrap(),
            original
        );
        assert_eq!(
            super::read_reg_value_raw(&key, "Tool").unwrap().0,
            "untouched"
        );
    }
    assert!(!super::export_reg_value(&key, " Tool missing ", &dest).unwrap());
    let add = std::process::Command::new(super::sys_tool("reg.exe"))
        .args([
            "add",
            &key,
            "/v",
            "REG_SZ",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ])
        .output()
        .unwrap();
    assert!(add.status.success());
    assert!(super::export_reg_value(&key, "REG_SZ", &dest).unwrap());
    assert!(
        crate::restore::decode_reg_text(std::fs::read(&dest).unwrap())
            .unwrap()
            .contains("\"REG_SZ\"=dword:00000001")
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn delete_value_intrinsic_gate_blocks_protected() {
    // value delete refuses protected trees even without caller checks.
    assert!(
        super::delete_value(r"HKLM\SYSTEM\CurrentControlSet\Services\WinDefend", "Start").is_err()
    );
    assert!(
        super::delete_value(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "").is_err()
    );
    assert!(super::delete_value(
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        "DisplayName"
    )
    .is_err());
    // Validate the allowed shape without deleting anything in the real Run key.
    assert!(crate::safety::is_safe_to_delete_registry(
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run|DemoVendorStartup"
    )
    .is_ok());
}

#[test]
fn native_primitives_reject_nul_names_before_system_access() {
    use super::*;
    let name = "Vendor\0Tail";
    assert!(parse(&format!(r"HKCU\Software\{name}")).is_none());
    // `delete_key` / `delete_value` report `Ok(false)` for "already gone", so
    // they still surface a refusal as an `Err`.
    for result in [
        delete_key(&format!(r"HKCU\Software\{name}")),
        delete_value("invalid", name),
    ] {
        assert!(result.unwrap_err().contains("native name contains NUL"));
    }
    for result in [
        rename_reg_value("invalid", name, name),
        create_reg_sz("invalid", name, "data"),
        write_service_start(name, 4),
    ] {
        assert!(result.unwrap_err().contains("native name contains NUL"));
    }
}

#[test]
fn merge_path_entry_dedupes_and_appends() {
    assert_eq!(super::merge_path_entry(r"C:\a;C:\b", r"C:\b"), None);
    assert_eq!(
        super::merge_path_entry(r"C:\a", r"C:\b"),
        Some(r"C:\a;C:\b".to_string())
    );
    assert!(super::path_contains_entry(r"C:\a;C:\b\", r"c:\b"));
    assert!(!super::path_contains_entry(r"C:\a", r"C:\b"));
}

#[cfg(windows)]
#[test]
fn export_reg_value_rejects_unsafe_names() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp =
        std::env::temp_dir().join(format!("remova_reg_inject_{}_{nonce}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let dest = tmp.join("value.reg");
    assert!(super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "bad\"name", &dest).is_err());
    assert!(super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "bad\r\nname", &dest).is_err());
    assert!(super::export_reg_value(r"HKCU\SOFTWARE\RemovaTest", "", &dest).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Only "not found" means the value is absent. Every other read failure must
/// surface as an error: reporting it as "the value does not exist" is what
/// let the Safety Vault's `.reg` export silently drop a value and leave
/// nothing to restore from, while the UI reported a successful backup.
#[cfg(windows)]
#[test]
fn only_not_found_counts_as_a_missing_registry_value() {
    struct KeyCleanup(String);
    impl Drop for KeyCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new(super::sys_tool("reg.exe"))
                .args(["delete", &self.0, "/f"])
                .output();
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let key = format!(
        r"HKCU\Software\RemovaReadFail_{}_{}",
        std::process::id(),
        nonce
    );
    let _cleanup = KeyCleanup(key.clone());
    let tmp = crate::test_support::TempDir::new("read_fail");
    let dest = tmp.join("value.reg");
    // Create the fixture key, then write straight through the Win32 API: the
    // product-level registry-write allowlist does not apply to a fixture key,
    // and `reg.exe` cannot carry a megabyte of hex on a command line.
    let created = std::process::Command::new(super::sys_tool("reg.exe"))
        .args([
            "add",
            &key,
            "/v",
            "Small",
            "/t",
            "REG_BINARY",
            "/d",
            "010203",
            "/f",
        ])
        .output()
        .unwrap();
    assert!(created.status.success(), "fixture key write failed");
    let write_binary = |name: &str, data: &[u8]| {
        use windows::Win32::System::Registry::{RegSetValueExW, REG_BINARY};
        let (hive, sub, view) = super::parse(&key).unwrap();
        let sub_w = super::to_wide(&sub);
        let name_w = super::to_wide(name);
        let mut hk = super::HKEY::default();
        unsafe {
            super::RegOpenKeyExW(
                hive,
                super::PCWSTR(sub_w.as_ptr()),
                0,
                super::KEY_SET_VALUE | view,
                &mut hk,
            )
            .ok()
            .unwrap();
            RegSetValueExW(
                hk,
                super::PCWSTR(name_w.as_ptr()),
                0,
                REG_BINARY,
                Some(data),
            )
            .ok()
            .unwrap();
            let _ = super::RegCloseKey(hk);
        }
    };

    // A value that was never written — the missing case.
    assert!(super::export_reg_value(&key, "Small", &dest).unwrap());
    assert!(!super::export_reg_value(&key, "NeverWritten", &dest).unwrap());
    // A key that does not exist is the same missing case, not a failure.
    assert!(!super::export_reg_value(&format!(r"{key}\NoSuchChild"), "Any", &dest).unwrap());

    // A value the reader cannot fetch in one buffer is a FAILURE, not an
    // absence: it used to come back as `Ok(false)`, indistinguishable from a
    // value that was never there.
    write_binary("Huge", &vec![7u8; (1 << 20) + 4096]);
    let huge_dest = tmp.join("huge.reg");
    let result = super::export_reg_value(&key, "Huge", &huge_dest);
    assert!(
        result.is_err(),
        "a value the reader cannot fetch must be an error, not a silently dropped one: {result:?}"
    );
    assert!(
        !huge_dest.exists(),
        "no partial .reg may be left behind for a value that failed to read"
    );
}

/// a value above the initial 16 KiB buffer must still be renameable —
/// the anti-rollback `.remova-disabled` rename used to fail on it forever.
#[cfg(windows)]
#[test]
fn rename_reg_value_reads_values_beyond_the_first_buffer() {
    // The raw Win32 setter: the crate's own RegSetValueExW wrapper is
    // allowlist-gated and would refuse this fixture key.
    use windows::Win32::System::Registry::{RegSetValueExW as RawSetValueExW, REG_SZ};

    struct KeyCleanup(String);
    impl Drop for KeyCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new(super::sys_tool("reg.exe"))
                .args(["delete", &self.0, "/f"])
                .output();
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // rename_reg_value writes through the allowlist-gated primitive, so the
    // fixture key must live inside the Uninstall subtree it is allowed to
    // manage; it is deleted again on drop.
    let key = format!(
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\RemovaRenameBig_{}_{}",
        std::process::id(),
        nonce
    );
    let _cleanup = KeyCleanup(key.clone());
    let created = std::process::Command::new(super::sys_tool("reg.exe"))
        .args(["add", &key, "/v", "Big", "/t", "REG_SZ", "/d", "seed", "/f"])
        .output()
        .unwrap();
    assert!(created.status.success(), "fixture key write failed");

    // Overwrite "Big" with a 20_000-char string straight through the Win32
    // API — reg.exe cannot carry 20 KB on a command line.
    let payload: Vec<u16> = std::iter::repeat('x' as u16)
        .take(20_000)
        .chain(std::iter::once(0))
        .collect();
    let (hive, sub, view) = super::parse(&key).unwrap();
    let sub_w = super::to_wide(&sub);
    let name_w = super::to_wide("Big");
    let mut hk = super::HKEY::default();
    unsafe {
        super::RegOpenKeyExW(
            hive,
            super::PCWSTR(sub_w.as_ptr()),
            0,
            super::KEY_SET_VALUE | view,
            &mut hk,
        )
        .ok()
        .unwrap();
        RawSetValueExW(
            hk,
            super::PCWSTR(name_w.as_ptr()),
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                payload.as_ptr().cast::<u8>(),
                payload.len() * 2,
            )),
        )
        .ok()
        .unwrap();
        let _ = super::RegCloseKey(hk);
    }

    super::rename_reg_value(&key, "Big", "Big2").unwrap();
    let (bytes, _typ) = super::read_reg_value_bytes(&key, "Big2").unwrap().unwrap();
    assert_eq!(bytes.len() / 2 - 1, 20_000, "the value must survive intact");
    assert!(
        super::read_reg_value_bytes(&key, "Big").unwrap().is_none(),
        "the source value must be gone after the rename"
    );
    // A6: delete explicitly and prove the real-registry fixture is gone
    // (the Drop fallback only runs when the test returns).
    let removed = std::process::Command::new(super::sys_tool("reg.exe"))
        .args(["delete", &key, "/f"])
        .output()
        .unwrap();
    assert!(removed.status.success(), "fixture key delete failed");
    let gone = std::process::Command::new(super::sys_tool("reg.exe"))
        .args(["query", &key])
        .output()
        .unwrap();
    assert!(!gone.status.success(), "fixture key must be gone");
}

#[cfg(windows)]
#[test]
fn export_reg_value_writes_existing_value() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp = std::env::temp_dir().join(format!("remova_value_reg_{}_{nonce}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let dest = tmp.join("value.reg");
    // Read-only OS fixture: do not mutate the user's PATH or registry.
    let r = super::export_reg_value(
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders",
        "Desktop",
        &dest,
    );
    match r {
        Ok(true) => {
            let s = crate::restore::decode_reg_text(std::fs::read(&dest).unwrap()).unwrap();
            assert!(s.contains("Windows Registry Editor"));
            assert!(s.contains("Shell Folders"));
            assert!(s.contains("\"Desktop\"="));
            assert!(s.contains("HKEY_CURRENT_USER"));
        }
        Ok(false) => panic!("Desktop registry fixture missing: export was not exercised"),
        Err(e) => panic!("export_reg_value: {e}"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

/// registry-native PATH read — pure pieces get direct coverage.
/// `wstring_from_reg_data` decodes UTF-16LE and trims the trailing NUL.
#[cfg(windows)]
#[test]
fn wstring_from_reg_data_decodes_utf16le() {
    let s = r"C:\Vendor 工具;C:\Other";
    let mut wide: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    wide.extend_from_slice(&[0, 0]); // NUL terminator as the registry stores it
    assert_eq!(crate::fsutil::wstring_from_reg_data(&wide), s);
    // Odd byte count: the trailing half unit is dropped, not mis-decoded.
    assert_eq!(
        crate::fsutil::wstring_from_reg_data(&wide[..wide.len() - 1]),
        s
    );
    // Empty / short blobs decode to "".
    assert_eq!(crate::fsutil::wstring_from_reg_data(&[]), "");
    assert_eq!(crate::fsutil::wstring_from_reg_data(&[0x41]), "");
    assert_eq!(crate::fsutil::wstring_from_reg_data(&[0x41, 0x00]), "A");
}

/// `%VAR%` expansion used for REG_EXPAND_SZ Path values.
#[cfg(windows)]
#[test]
fn expand_env_string_expands_and_passes_through() {
    let _lock = crate::test_support::EnvLock::new();
    let _var = crate::test_support::EnvVar::set("REMOVA_QA02_VAR", r"C:\Vendor Tool");
    assert_eq!(
        super::expand_env_string(r"%REMOVA_QA02_VAR%\bin"),
        r"C:\Vendor Tool\bin"
    );
    assert_eq!(
        super::expand_env_string("no markers here"),
        "no markers here"
    );
    // Unknown variable stays verbatim (Windows keeps the reference).
    assert_eq!(
        super::expand_env_string("%REMOVA_QA02_UNSET_VAR%/x"),
        "%REMOVA_QA02_UNSET_VAR%/x"
    );
}

/// real-registry integration check — `HKCU\Environment` Path
/// reads through the native code path (mock must be inactive). Opt-in via
/// `cargo test -- --ignored` since stock machines may have no user Path.
#[cfg(windows)]
#[test]
#[ignore = "environment-dependent: needs a real user PATH value"]
fn read_reg_path_value_reads_real_user_environment() {
    assert!(!super::path_mock::active());
    // Must not fall back to the process env or error out on a healthy box.
    let raw = super::read_path_scope("User").expect("native user PATH read");
    // A successful read returns a string (possibly empty on a stripped
    // account) — never process-env-shaped noise, never Err.
    assert!(
        raw.contains(';') || raw.contains('\\') || raw.contains('/') || raw.is_empty(),
        "unexpected user PATH shape: {raw:?}"
    );
}

/// REG_EXPAND_SZ PATH values must be rewritten with their raw `%VAR%`
/// bytes and type intact — scrub/restore must not bake in expansions.
#[cfg(windows)]
#[test]
fn scrub_and_restore_preserve_expand_sz_raw_bytes() {
    use super::path_mock;
    let _lock = path_mock::lock_mock();
    path_mock::clear();
    // Expansion sources are pinned for hermeticity: the machine's
    // ProgramFiles may be relocated or absent, so the test pins its own
    // variable that only the fallback source can resolve.
    super::expand_mock::install(&[("RemovaVendorDir", r"C:\Program Files")]);
    assert_eq!(
        super::resolve_unresolved_vars("PREFIX%RemovaUnknownFixture%TAIL", 0),
        "PREFIX%RemovaUnknownFixture%TAIL"
    );
    assert_eq!(
        super::resolve_unresolved_vars(r"PRE%RemovaVendorDir%\mid%RemovaUnknownFixture%TAIL", 0),
        r"PREC:\Program Files\mid%RemovaUnknownFixture%TAIL"
    );
    assert_eq!(
        super::resolve_unresolved_vars("literal%unfinished", 0),
        "literal%unfinished"
    );
    path_mock::install_expand(
        r"%SystemRoot%\system32;C:\Tools\App;%RemovaVendorDir%\Shared",
        r"%SystemRoot%",
    );
    // Scrub a literal entry: untouched %VAR% segments survive verbatim.
    assert!(super::scrub_path_entry(r"C:\Tools\App").unwrap());
    let (raw, expand) = path_mock::read_raw("User").unwrap();
    assert!(expand, "scrub must not flatten REG_EXPAND_SZ");
    assert_eq!(raw, r"%SystemRoot%\system32;%RemovaVendorDir%\Shared");
    // An entry already present in expanded form must not duplicate raw.
    assert!(!super::restore_path_entry(r"C:\Program Files\Shared", &["User"]).unwrap());
    // Restore appends raw bytes and keeps the type.
    assert!(super::restore_path_entry(r"C:\Tools\App", &["User"]).unwrap());
    let (raw, expand) = path_mock::read_raw("User").unwrap();
    assert!(expand);
    assert_eq!(
        raw,
        r"%SystemRoot%\system32;%RemovaVendorDir%\Shared;C:\Tools\App"
    );
    // The literal entry now present must not be appended twice.
    assert!(!super::restore_path_entry(r"C:\Tools\App", &["User"]).unwrap());
    // Scrub an entry that only matches after expansion.
    assert!(super::scrub_path_entry(r"C:\Program Files\Shared").unwrap());
    let (raw, expand) = path_mock::read_raw("User").unwrap();
    assert!(expand);
    assert_eq!(raw, r"%SystemRoot%\system32;C:\Tools\App");
    // REG_SZ scopes keep plain String semantics.
    super::expand_mock::clear();
    path_mock::clear();
    path_mock::install(r"C:\A;C:\B", r"C:\Windows");
    assert!(super::scrub_path_entry(r"C:\A").unwrap());
    let (raw, expand) = path_mock::read_raw("User").unwrap();
    assert!(!expand);
    assert_eq!(raw, r"C:\B");
    path_mock::clear();
}

/// A variable no source can resolve stays literal; restore appends
/// honestly (and keeps the raw form) instead of guessing a match.
#[test]
fn restore_appends_when_var_unresolvable() {
    use super::path_mock;
    let _lock = path_mock::lock_mock();
    path_mock::clear();
    path_mock::install_expand(r"%RemovaNowhereAtAll%\Shared", r"");
    assert!(super::restore_path_entry(r"C:\Program Files\Shared", &["User"]).unwrap());
    let (raw, expand) = path_mock::read_raw("User").unwrap();
    assert!(expand);
    assert_eq!(raw, r"%RemovaNowhereAtAll%\Shared;C:\Program Files\Shared");
    path_mock::clear();
}
