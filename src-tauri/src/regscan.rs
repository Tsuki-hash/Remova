//! Minimal registry helpers for scanner (read-only).

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ,
};

fn parse_alias(key: &str) -> Option<(HKEY, String, REG_SAM_FLAGS)> {
    let (alias, rest) = key.split_once('\\')?;
    let rest = rest.to_string();
    match alias.to_uppercase().as_str() {
        "HKLM64" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKLM32" | "HKLMWOW" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_32KEY)),
        "HKLM" => Some((HKEY_LOCAL_MACHINE, rest, KEY_WOW64_64KEY)),
        "HKCU" => Some((HKEY_CURRENT_USER, rest, KEY_WOW64_64KEY)),
        _ => None,
    }
}

// Shared Windows string helpers live in `fsutil`.
#[cfg(windows)]
use crate::fsutil::{to_wide, wstring_from_reg_data};

/// App Paths subkeys named like `exe_name` under HKLM64 / HKLM32 / HKCU.
pub fn find_app_paths(exe_name: &str) -> Vec<String> {
    #[cfg(not(windows))]
    {
        let _ = exe_name;
        return vec![];
    }
    #[cfg(windows)]
    {
        let targets = [
            (
                "HKLM64",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
            (
                "HKLM32",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
            (
                "HKCU",
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
            ),
        ];
        let needle = exe_name.to_lowercase();
        let mut hits = vec![];
        for (alias, sub) in targets {
            let full = format!("{alias}\\{sub}");
            for sk in list_subkeys(&full) {
                let l = sk.to_lowercase();
                if l == needle || l == format!("{needle}.exe") {
                    hits.push(format!("{full}\\{sk}"));
                }
            }
        }
        hits
    }
}

fn collect_complete_names(
    mut read: impl FnMut(u32) -> Result<Option<String>, String>,
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for index in 0..10_000 {
        if crate::scan_task::cancelled() {
            return Err("scan:cancelled".into());
        }
        match read(index)? {
            Some(name) => names.push(name),
            None => return Ok(names),
        }
    }
    Err("registry snapshot entry budget exceeded".into())
}

/// Query the exact key or value, preserving registry view and read errors.
pub fn target_exists(target: &str) -> Result<bool, String> {
    let (key, value) = match target.split_once('|') {
        Some((key, value)) if !value.contains('|') => (key, Some(value)),
        Some(_) => return Err("invalid registry target".into()),
        None => (target, None),
    };
    let (hive, sub, access) = parse_alias(key).ok_or("invalid registry alias")?;
    let sub = crate::fsutil::to_wide_name(&sub)?;
    let value = value.map(crate::fsutil::to_wide_name).transpose()?;
    unsafe {
        let mut root = HKEY::default();
        let opened = RegOpenKeyExW(hive, PCWSTR(sub.as_ptr()), 0, KEY_READ | access, &mut root);
        if opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND {
            return Ok(false);
        }
        if opened != ERROR_SUCCESS {
            return Err(format!("registry verify open: {}", opened.0));
        }
        let queried = match value {
            None => ERROR_SUCCESS,
            Some(name) => windows::Win32::System::Registry::RegQueryValueExW(
                root,
                PCWSTR(name.as_ptr()),
                None,
                None,
                None,
                None,
            ),
        };
        let _ = RegCloseKey(root);
        match queried {
            ERROR_SUCCESS | ERROR_MORE_DATA => Ok(true),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
            status => Err(format!("registry verify value: {}", status.0)),
        }
    }
}

/// Monitor snapshots need explicit completeness and names of every value type.
pub fn snapshot_names(key: &str) -> Result<(Vec<String>, Vec<String>), String> {
    if crate::scan_task::cancelled() {
        return Err("scan:cancelled".into());
    }
    #[cfg(not(windows))]
    {
        let _ = key;
        Ok((vec![], vec![]))
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse_alias(key).ok_or("invalid registry alias")?;
        let sub_w = crate::fsutil::to_wide_name(&sub)?;
        unsafe {
            let mut root = HKEY::default();
            let opened = RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            );
            if opened == ERROR_FILE_NOT_FOUND || opened == ERROR_PATH_NOT_FOUND {
                return Ok((vec![], vec![]));
            }
            if opened != ERROR_SUCCESS {
                return Err(format!("registry snapshot open: {}", opened.0));
            }
            let result = (|| {
                let keys = collect_complete_names(|index| {
                    let mut buffer = vec![0u16; 256];
                    let mut len = buffer.len() as u32;
                    let status = RegEnumKeyExW(
                        root,
                        index,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &mut len,
                        None,
                        windows::core::PWSTR::null(),
                        None,
                        None,
                    );
                    match status {
                        ERROR_SUCCESS => {
                            Ok(Some(String::from_utf16_lossy(&buffer[..len as usize])))
                        }
                        ERROR_NO_MORE_ITEMS => Ok(None),
                        _ => Err(format!("registry snapshot subkey: {}", status.0)),
                    }
                })?;
                let values = collect_complete_names(|index| {
                    let mut buffer = vec![0u16; 16_384];
                    let mut len = buffer.len() as u32;
                    let status = RegEnumValueW(
                        root,
                        index,
                        windows::core::PWSTR(buffer.as_mut_ptr()),
                        &mut len,
                        None,
                        None,
                        None,
                        None,
                    );
                    match status {
                        ERROR_SUCCESS => {
                            Ok(Some(String::from_utf16_lossy(&buffer[..len as usize])))
                        }
                        ERROR_NO_MORE_ITEMS => Ok(None),
                        _ => Err(format!("registry snapshot value: {}", status.0)),
                    }
                })?;
                Ok((keys, values))
            })();
            let _ = RegCloseKey(root);
            result
        }
    }
}

pub fn list_subkeys(key: &str) -> Vec<String> {
    if crate::scan_task::cancelled() {
        return vec![];
    }
    if let Some(cached) = subkey_cache_get(key) {
        return cached;
    }
    match list_subkeys_uncached(key) {
        Some(fresh) => {
            subkey_cache_put(key, &fresh);
            fresh
        }
        // An access failure is not an empty key. Caching it would answer every
        // later caller with a false empty list for the whole TTL.
        None => vec![],
    }
}

/// One batch scan analyzes app after app, and each analyze re-enumerated the
/// same Classes/CLSID/Services trees. Those trees do not change while a batch
/// runs. The cache is process-wide (batch apps run on different blocking
/// threads, so a thread-local copy was never reused).
///
/// Two properties keep the reuse honest:
/// - it is only consulted inside a scan task, so the deletion gates (which
///   re-verify a live target) always read the registry;
/// - a destructive operation calls [`invalidate_subkey_cache`], and the entry
///   expires on its own after [`subkey_cache::TTL`].
mod subkey_cache {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// Reuse window. A batch analyzes its items back to back; a new inventory
    /// scan and every destructive operation clear the cache explicitly, so a
    /// longer window only covers the run instead of pinning stale lists forever.
    pub(super) const TTL: Duration = Duration::from_secs(5 * 60);
    /// Enumerations larger than this are not cached: copying the pathological
    /// keys would cost more memory than re-reading them.
    pub(super) const MAX_CACHED_ENTRIES: usize = 20_000;
    /// Bound on how many distinct keys may be cached at once.
    pub(super) const MAX_CACHED_KEYS: usize = 64;

    struct Entry {
        names: Vec<String>,
        at: Instant,
    }

    #[cfg(test)]
    #[derive(Default)]
    pub(super) struct Counters {
        pub hits: u64,
        pub misses: u64,
        pub stored: u64,
        pub expired: u64,
        pub skipped_too_large: u64,
        pub evicted: u64,
    }

    #[derive(Default)]
    struct State {
        entries: HashMap<String, Entry>,
    }

    // Diagnostics are test-only: keeping the thread-local bump on the hot path
    // charged every real scan for counters only the test suite reads.
    #[cfg(test)]
    thread_local! {
        static COUNTERS: std::cell::RefCell<Counters> =
            std::cell::RefCell::new(Counters::default());
    }

    fn state() -> &'static Mutex<State> {
        static STATE: OnceLock<Mutex<State>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(State::default()))
    }

    pub(super) fn get(key: &str) -> Option<Vec<String>> {
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        let expired = state
            .entries
            .get(key)
            .is_some_and(|entry| entry.at.elapsed() >= TTL);
        if expired {
            state.entries.remove(key);
            #[cfg(test)]
            COUNTERS.with(|n| n.borrow_mut().expired += 1);
            return None;
        }
        let names = state.entries.get(key).map(|entry| entry.names.clone());
        #[cfg(test)]
        if names.is_some() {
            COUNTERS.with(|n| n.borrow_mut().hits += 1);
        }
        names
    }

    pub(super) fn put(key: &str, value: &[String]) {
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        state.entries.retain(|_, e| e.at.elapsed() < TTL);
        if !state.entries.contains_key(key) && state.entries.len() >= MAX_CACHED_KEYS {
            #[cfg(test)]
            COUNTERS.with(|n| n.borrow_mut().evicted += 1);
            return;
        }
        state.entries.insert(
            key.to_string(),
            Entry {
                names: value.to_vec(),
                at: Instant::now(),
            },
        );
        #[cfg(test)]
        COUNTERS.with(|n| n.borrow_mut().stored += 1);
    }

    pub(super) fn note_uncacheable() {
        #[cfg(test)]
        COUNTERS.with(|n| n.borrow_mut().skipped_too_large += 1);
    }

    pub(super) fn note_miss() {
        #[cfg(test)]
        COUNTERS.with(|n| n.borrow_mut().misses += 1);
    }

    pub(super) fn clear() {
        state()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .clear();
    }

    #[cfg(test)]
    pub(super) fn reset_counters() {
        COUNTERS.with(|n| *n.borrow_mut() = Counters::default());
    }

    #[cfg(test)]
    pub(super) fn put_expired(key: &str) {
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        state.entries.insert(
            key.to_string(),
            Entry {
                names: vec!["expired".into()],
                at: Instant::now() - TTL - Duration::from_secs(1),
            },
        );
    }

    #[cfg(test)]
    pub(super) fn snapshot() -> (u64, u64, u64, u64, u64, u64) {
        COUNTERS.with(|n| {
            let n = n.borrow();
            (
                n.hits,
                n.misses,
                n.stored,
                n.expired,
                n.skipped_too_large,
                n.evicted,
            )
        })
    }
}

/// Cache the default (description) value of CLSID keys. A batch checks every
/// CLSID leaf for every app, so without reuse the same ~20k values are opened
/// once per app. Values are read-only; a new inventory or destructive operation
/// clears the cache.
mod default_cache {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const TTL: Duration = Duration::from_secs(5 * 60);
    /// CLSID-rich machines can exceed 20k leaves; keep enough headroom that a
    /// full table is rare, but never grow without bound.
    const MAX_ENTRIES: usize = 40_000;
    /// Do not sweep a full table more than once a minute: a rejected insert
    /// must stay O(1) even on a machine with more leaves than the cap.
    const SWEEP_INTERVAL_SECS: u64 = 60;
    static LAST_SWEEP: AtomicU64 = AtomicU64::new(0);

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    struct Entry {
        /// `None` caches "this key has no usable default value" so a batch does
        /// not reopen every CLSID leaf for every app.
        value: Option<String>,
        at: Instant,
    }

    fn state() -> &'static Mutex<HashMap<String, Entry>> {
        static STATE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Outer `None` = cache miss/expired; `Some(None)` = a cached absence.
    pub(super) fn get(key: &str) -> Option<Option<String>> {
        let mut entries = state().lock().unwrap_or_else(|e| e.into_inner());
        if entries
            .get(key)
            .is_some_and(|entry| entry.at.elapsed() >= TTL)
        {
            entries.remove(key);
            return None;
        }
        entries.get(key).map(|entry| entry.value.clone())
    }

    pub(super) fn put(key: &str, value: Option<&str>) {
        let mut entries = state().lock().unwrap_or_else(|e| e.into_inner());
        // Never sweep per insert: the CLSID loop inserts ~20k entries and an
        // O(n) retain on each one made the "optimization" quadratic. Sweep only
        // at the cap, and reject new keys if the table is still full.
        if !entries.contains_key(key) && entries.len() >= MAX_ENTRIES {
            let now = now_secs();
            let last = LAST_SWEEP.load(Ordering::Relaxed);
            if now.saturating_sub(last) >= SWEEP_INTERVAL_SECS {
                entries.retain(|_, entry| entry.at.elapsed() < TTL);
                LAST_SWEEP.store(now, Ordering::Relaxed);
            }
            if entries.len() >= MAX_ENTRIES {
                return;
            }
        }
        entries.insert(
            key.to_string(),
            Entry {
                value: value.map(str::to_string),
                at: Instant::now(),
            },
        );
    }

    pub(super) fn clear() {
        state().lock().unwrap_or_else(|e| e.into_inner()).clear();
        LAST_SWEEP.store(0, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(super) fn put_expired(key: &str, value: Option<&str>) {
        let mut entries = state().lock().unwrap_or_else(|e| e.into_inner());
        entries.insert(
            key.to_string(),
            Entry {
                value: value.map(str::to_string),
                at: Instant::now() - TTL - Duration::from_secs(1),
            },
        );
    }
}

/// Drop cached subkey enumerations. Called after a destructive operation so the
/// next scan reads the registry rather than a list captured before the delete.
/// The default-value cache is intentionally *not* cleared here: a batch runs
/// analyze → cleanup → next analyze, and refilling ~20k descriptions per app
/// would erase the reuse this cache exists for. A new inventory clears both.
pub fn invalidate_subkey_cache() {
    subkey_cache::clear();
}

/// Drop the CLSID default-value cache. Called when a new app inventory makes
/// the previous generation's descriptions obsolete.
pub fn invalidate_default_value_cache() {
    default_cache::clear();
}

/// (hits, misses, stored, expired, skipped_too_large, evicted). Test and
/// diagnostic hook: it proves the reuse actually happened.
#[cfg(test)]
pub fn registry_cache_counters() -> (u64, u64, u64, u64, u64, u64) {
    subkey_cache::snapshot()
}

#[cfg(test)]
pub(crate) fn reset_registry_cache_for_test() {
    subkey_cache::clear();
    default_cache::clear();
    subkey_cache::reset_counters();
}

fn subkey_cache_get(key: &str) -> Option<Vec<String>> {
    // Outside a scan task the registry is read live: only a scan may serve a
    // list from memory.
    if !crate::scan_task::has_task() || crate::scan_task::cancelled() {
        return None;
    }
    subkey_cache::get(key)
}

fn subkey_cache_put(key: &str, value: &[String]) {
    if !crate::scan_task::has_task() || crate::scan_task::cancelled() {
        return;
    }
    if value.len() > subkey_cache::MAX_CACHED_ENTRIES {
        subkey_cache::note_uncacheable();
        return;
    }
    subkey_cache::put(key, value);
}

fn list_subkeys_uncached(key: &str) -> Option<Vec<String>> {
    subkey_cache::note_miss();
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut count = 0u32;
            let mut max_sub = 0u32;
            let _ = RegQueryInfoKeyW(
                root,
                windows::core::PWSTR::null(),
                None,
                None,
                Some(&mut count),
                Some(&mut max_sub),
                None,
                None,
                None,
                None,
                None,
                None,
            );
            let mut out = vec![];
            for i in 0..count {
                if crate::scan_task::cancelled() {
                    let _ = RegCloseKey(root);
                    // A partial enumeration must never become a cache entry.
                    return None;
                }
                let mut buf = vec![0u16; max_sub as usize + 2];
                let mut len = buf.len() as u32;
                if RegEnumKeyExW(
                    root,
                    i,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                    None,
                    windows::core::PWSTR::null(),
                    None,
                    None,
                ) == ERROR_SUCCESS
                {
                    buf.truncate(len as usize);
                    out.push(String::from_utf16_lossy(&buf));
                }
            }
            let _ = RegCloseKey(root);
            Some(out)
        }
    }
}

pub fn list_values(key: &str) -> Vec<(String, String)> {
    list_values_capped(key, usize::MAX)
}

/// Enumerate at most `max` registry values. Used by bounded scans so a huge
/// key is not fully materialized before the cap is applied.
pub fn list_values_capped(key: &str, max: usize) -> Vec<(String, String)> {
    if crate::scan_task::cancelled() || max == 0 {
        return vec![];
    }
    #[cfg(not(windows))]
    {
        let _ = (key, max);
        vec![]
    }
    #[cfg(windows)]
    {
        let Some((hive, sub, access)) = parse_alias(key) else {
            return vec![];
        };
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return vec![];
            }
            let mut out = vec![];
            let mut n = 0u32;
            loop {
                if crate::scan_task::cancelled() || n as usize >= max {
                    break;
                }
                let mut vname = vec![0u16; 256];
                let mut vname_len = vname.len() as u32;
                let mut vtype = 0u32;
                let mut data = vec![0u8; 4096];
                let mut data_len = data.len() as u32;
                let mut st = RegEnumValueW(
                    root,
                    n,
                    windows::core::PWSTR(vname.as_mut_ptr()),
                    &mut vname_len,
                    None,
                    Some(&mut vtype),
                    Some(data.as_mut_ptr()),
                    Some(&mut data_len),
                );
                if st == ERROR_MORE_DATA {
                    // one retry with the buffer sizes the API reported
                    // instead of silently dropping oversized names/values.
                    let need_name = vname_len as usize;
                    let need_data = data_len as usize;
                    if need_name > (1 << 14) || need_data > (1 << 20) {
                        n += 1; // pathological — skip this value, keep enumerating
                        continue;
                    }
                    if need_name > vname.len() {
                        vname = vec![0u16; need_name + 1];
                    }
                    if need_data > data.len() {
                        data = vec![0u8; need_data];
                    }
                    vname_len = vname.len() as u32;
                    data_len = data.len() as u32;
                    st = RegEnumValueW(
                        root,
                        n,
                        windows::core::PWSTR(vname.as_mut_ptr()),
                        &mut vname_len,
                        None,
                        Some(&mut vtype),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                }
                if st == ERROR_MORE_DATA {
                    // Second failure (value grew again / buffer race) — skip
                    // this value and keep enumerating instead of dropping the rest.
                    n += 1;
                    continue;
                }
                if st != ERROR_SUCCESS {
                    break;
                }
                n += 1;
                if vtype == REG_SZ.0 || vtype == REG_EXPAND_SZ.0 {
                    let name = String::from_utf16_lossy(&vname[..vname_len as usize]);
                    let val = wstring_from_reg_data(&data[..data_len as usize]);
                    out.push((name, val));
                }
            }
            let _ = RegCloseKey(root);
            out
        }
    }
}

/// Read REG_SZ/EXPAND_SZ value by name via RegQueryValueExW.
pub fn read_string(key: &str, value_name: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{
            RegQueryValueExW, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
        };
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 4096];
            let mut data_len = data.len() as u32;
            let mut st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if st == ERROR_MORE_DATA {
                // re-query with the reported size — long UninstallString
                // / display values must not be silently dropped.
                let need = data_len as usize;
                if need > data.len() && need <= (1 << 20) {
                    data = vec![0u8; need];
                    data_len = data.len() as u32;
                    st = RegQueryValueExW(
                        root,
                        PCWSTR(name_w.as_ptr()),
                        None,
                        Some(&mut vtype),
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    );
                }
            }
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || (vtype != REG_SZ && vtype != REG_EXPAND_SZ) {
                return None;
            }
            Some(wstring_from_reg_data(&data[..data_len as usize]))
        }
    }
}

/// Read REG_DWORD value by name via RegQueryValueExW (, O(1) — same path as `read_string`).
pub fn read_dword(key: &str, value_name: &str) -> Option<u32> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, REG_DWORD, REG_VALUE_TYPE};
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = [0u8; 4];
            let mut data_len = data.len() as u32;
            let st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || vtype != REG_DWORD || data_len < 4 {
                return None;
            }
            Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        }
    }
}

/// Read REG_BINARY value by name (StartupApproved etc.).
pub fn read_binary(key: &str, value_name: &str) -> Option<Vec<u8>> {
    #[cfg(not(windows))]
    {
        let _ = (key, value_name);
        None
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{RegQueryValueExW, REG_BINARY, REG_VALUE_TYPE};
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut root = HKEY::default();
            if RegOpenKeyExW(
                hive,
                PCWSTR(sub_w.as_ptr()),
                0,
                KEY_READ | access,
                &mut root,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let name_w = to_wide(value_name);
            let mut vtype = REG_VALUE_TYPE(0);
            let mut data = vec![0u8; 64];
            let mut data_len = data.len() as u32;
            let st = RegQueryValueExW(
                root,
                PCWSTR(name_w.as_ptr()),
                None,
                Some(&mut vtype),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            let _ = RegCloseKey(root);
            if st != ERROR_SUCCESS || vtype != REG_BINARY {
                return None;
            }
            data.truncate(data_len as usize);
            Some(data)
        }
    }
}

/// Cached variant of [`read_string_default`] for scan tasks. Outside a scan, or
/// after cancellation, the registry is read live (deletion gates rely on it).
pub fn read_string_default_cached(key: &str) -> Option<String> {
    if !crate::scan_task::has_task() || crate::scan_task::cancelled() {
        return read_string_default(key);
    }
    if let Some(value) = default_cache::get(key) {
        return value;
    }
    let value = read_string_default(key);
    // Cache the absence too: most CLSID leaves have no usable default value,
    // and without this every app re-opened thousands of keys.
    default_cache::put(key, value.as_deref());
    value
}

pub fn read_string_default(key: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let _ = key;
        None
    }
    #[cfg(windows)]
    {
        let (hive, sub, access) = parse_alias(key)?;
        unsafe {
            let sub_w = to_wide(&sub);
            let mut hk = HKEY::default();
            if RegOpenKeyExW(hive, PCWSTR(sub_w.as_ptr()), 0, KEY_READ | access, &mut hk)
                != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u8; 4096];
            let mut len = buf.len() as u32;
            let mut vtype = windows::Win32::System::Registry::REG_VALUE_TYPE(0);
            let empty = to_wide("");
            let mut st = windows::Win32::System::Registry::RegQueryValueExW(
                hk,
                PCWSTR(empty.as_ptr()),
                None,
                Some(&mut vtype),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            );
            if st == ERROR_MORE_DATA {
                // re-query with the reported size instead of dropping.
                let need = len as usize;
                if need > buf.len() && need <= (1 << 20) {
                    buf = vec![0u8; need];
                    len = buf.len() as u32;
                    st = windows::Win32::System::Registry::RegQueryValueExW(
                        hk,
                        PCWSTR(empty.as_ptr()),
                        None,
                        Some(&mut vtype),
                        Some(buf.as_mut_ptr()),
                        Some(&mut len),
                    );
                }
            }
            let _ = RegCloseKey(hk);
            if st != ERROR_SUCCESS {
                return None;
            }
            // A DWORD/QWORD default decodes to garbage UTF-16 and would poison
            // the matching chain — only string-ish types are text.
            if vtype != windows::Win32::System::Registry::REG_SZ
                && vtype != windows::Win32::System::Registry::REG_EXPAND_SZ
            {
                return None;
            }
            Some(wstring_from_reg_data(&buf[..len as usize]))
        }
    }
}

#[cfg(test)]
mod completeness_tests {
    #[test]
    fn cancellation_stops_before_the_next_registry_snapshot_read() {
        let id = crate::scan_task::begin().unwrap();
        let mut reads = 0;
        let result = crate::scan_task::run(id, || {
            super::collect_complete_names(|_| {
                reads += 1;
                crate::scan_task::cancel(id).unwrap();
                Ok(Some("first".into()))
            })
        });
        assert_eq!(reads, 1);
        assert_eq!(result, Err("scan:cancelled".into()));
    }
    #[test]
    fn late_enumeration_errors_and_caps_never_publish_partial_names() {
        assert!(super::collect_complete_names(|index| {
            if index == 0 {
                Ok(Some("existing".into()))
            } else {
                Err("access denied".into())
            }
        })
        .is_err());
        assert!(super::collect_complete_names(|_| Ok(Some("entry".into()))).is_err());
        assert_eq!(
            super::collect_complete_names(|_| Ok(None)).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            super::collect_complete_names(|index| Ok((index == 0).then(|| "entry".into())))
                .unwrap(),
            vec!["entry"]
        );
    }

    #[test]
    fn snapshot_rejects_invalid_names_before_native_access() {
        assert!(super::snapshot_names("INVALID").is_err());
        assert!(super::snapshot_names("HKCU\\Software\\Vendor\0Tail").is_err());
    }

    #[test]
    fn a_batch_scan_reuses_one_enumeration_instead_of_re_reading_per_app() {
        let _guard = crate::test_support::EnvLock::new();
        super::reset_registry_cache_for_test();
        let id = crate::scan_task::begin().unwrap();
        let first = crate::scan_task::run(id, || {
            let a = super::list_subkeys(r"HKCU\SOFTWARE");
            let (_, misses_after_first, ..) = super::registry_cache_counters();
            let b = super::list_subkeys(r"HKCU\SOFTWARE");
            let (hits, misses, stored, ..) = super::registry_cache_counters();
            (a, b, hits, misses, stored, misses_after_first)
        })
        .unwrap();
        let (a, b, hits, misses, stored, misses_after_first) = first;
        // The second read inside the same session is served from memory: the
        // enumeration count does not grow and a hit is recorded.
        assert_eq!(a, b, "the cached list must equal the live one");
        assert_eq!(misses_after_first, 1);
        assert_eq!(misses, 1, "the second read must not re-enumerate");
        assert_eq!(hits, 1);
        assert_eq!(stored, 1);
        super::reset_registry_cache_for_test();
    }

    #[test]
    fn enumeration_is_never_reused_outside_a_scan_task() {
        super::reset_registry_cache_for_test();
        // No scan task: the deletion gates read live, so nothing may be served
        // from memory even though a value is already cached.
        let id = crate::scan_task::begin().unwrap();
        crate::scan_task::run(id, || {
            let _ = super::list_subkeys(r"HKCU\SOFTWARE");
            // Second in-session read: this is the one that may hit the cache.
            let _ = super::list_subkeys(r"HKCU\SOFTWARE");
        })
        .unwrap();
        let (hits_inside, misses_inside, ..) = super::registry_cache_counters();
        assert_eq!(hits_inside, 1, "the in-session repeat read hits the cache");
        assert_eq!(misses_inside, 1);

        let _ = super::list_subkeys(r"HKCU\SOFTWARE");
        let (hits, misses, ..) = super::registry_cache_counters();
        assert_eq!(hits, 1, "the out-of-session read must not hit the cache");
        assert_eq!(misses, 2, "the out-of-session read must re-enumerate");
        super::reset_registry_cache_for_test();
    }

    #[test]
    fn cache_expiry_and_key_cap_are_enforced() {
        let _guard = crate::test_support::EnvLock::new();
        super::reset_registry_cache_for_test();
        super::subkey_cache::put_expired("expired-key");
        assert!(super::subkey_cache::get("expired-key").is_none());
        let (_, _, _, expired, _, _) = super::registry_cache_counters();
        assert_eq!(expired, 1);

        for i in 0..=super::subkey_cache::MAX_CACHED_KEYS {
            super::subkey_cache::put(&format!("key-{i}"), &[format!("value-{i}")]);
        }
        let (_, _, _, _, _, evicted) = super::registry_cache_counters();
        assert!(evicted > 0, "a full cache must reject a new key");
        super::reset_registry_cache_for_test();
    }

    #[test]
    fn default_value_cache_reuses_expires_and_invalidates() {
        super::default_cache::clear();
        super::default_cache::put("cls-key", Some("description"));
        assert_eq!(
            super::default_cache::get("cls-key").flatten().as_deref(),
            Some("description")
        );
        // Absence is a cacheable result too: most CLSID leaves have no default.
        super::default_cache::put("cls-missing", None);
        assert_eq!(super::default_cache::get("cls-missing"), Some(None));
        super::default_cache::put_expired("cls-old", Some("old"));
        assert!(super::default_cache::get("cls-old").is_none());
        // A cleanup clears subkey enumerations but must keep the CLSID values
        // reusable across the batch.
        super::invalidate_subkey_cache();
        assert_eq!(
            super::default_cache::get("cls-key").flatten().as_deref(),
            Some("description")
        );
        super::invalidate_default_value_cache();
        assert!(super::default_cache::get("cls-key").is_none());
    }

    #[test]
    fn cached_default_value_reads_live_outside_and_after_cancellation() {
        super::default_cache::clear();
        let key = r"HKCU\Software\RemovaP309Missing\NoSuchValue";
        super::default_cache::put(key, Some("stale-cached"));
        // Outside any scan task the deletion gate reads the registry; a seeded
        // cache entry must not be replayed.
        assert_eq!(super::read_string_default_cached(key), None);

        let id = crate::scan_task::begin().unwrap();
        let seen = std::cell::Cell::new(Some("sentinel".to_string()));
        let result = crate::scan_task::run(id, || {
            let _ = crate::scan_task::cancel(id);
            // A cancelled task must also fall back to the live read.
            seen.set(super::read_string_default_cached(key));
        });
        assert!(
            result.is_err(),
            "the cancelled task must report cancellation"
        );
        assert_eq!(seen.take(), None);
        super::default_cache::clear();
    }

    #[test]
    fn a_destructive_operation_drops_the_cached_enumerations() {
        super::reset_registry_cache_for_test();
        let id = crate::scan_task::begin().unwrap();
        crate::scan_task::run(id, || {
            let _ = super::list_subkeys(r"HKCU\SOFTWARE");
            let _ = super::list_subkeys(r"HKCU\SOFTWARE");
            let (hits, ..) = super::registry_cache_counters();
            assert_eq!(hits, 1, "fixture must have cached an enumeration");
            // A delete just changed the registry: the next scan must re-read.
            super::invalidate_subkey_cache();
            let _ = super::list_subkeys(r"HKCU\SOFTWARE");
            let (hits_after, misses_after, ..) = super::registry_cache_counters();
            assert_eq!(hits_after, 1, "the invalidated entry must not be reused");
            assert_eq!(misses_after, 2, "the read after invalidation re-enumerates");
        })
        .unwrap();
        super::reset_registry_cache_for_test();
    }
}
