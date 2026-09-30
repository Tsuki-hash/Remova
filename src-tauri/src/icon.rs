//! Extract a file/app icon as PNG bytes (Windows). Optional disk cache.

use crate::apps::parse_display_icon;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

const ICON_SIZE: i32 = 32;

fn icon_cache_dir() -> PathBuf {
    // never fall back to C:\Users\Public (shared writable).
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let temp =
            std::env::var("TEMP").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into());
        format!("{temp}\\Remova-{}", std::process::id())
    });
    PathBuf::from(local).join("Remova").join("icons")
}

fn fnv1a64(s: &str) -> u64 {
    crate::fsutil::fnv1a64(s)
}

/// File fingerprint so icon updates (same DisplayIcon path) invalidate the cache.
fn source_fingerprint(path: &str) -> String {
    match std::fs::metadata(path) {
        Ok(m) => {
            let len = m.len();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{len}:{mtime}")
        }
        Err(_) => "missing".to_string(),
    }
}

/// PNG file signature (0x89 'PNG' CR LF 0x1A LF) — every cached
/// payload must start with it, so a torn or planted file is never served.
fn looks_like_png(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}

fn cache_key(raw: &str) -> String {
    let (path, index) = parse_display_icon(raw);
    let fp = source_fingerprint(&path);
    let payload = format!("{raw}\0{index}\0{fp}");
    format!("{:016x}.png", fnv1a64(&payload))
}

/// Extract icon from `DisplayIcon` raw value (`path` or `path,index`) as PNG bytes.
/// Uses `%LOCALAPPDATA%\Remova\icons\{hash}.png` as disk cache (key includes source fingerprint).
pub fn extract_icon_png(raw_display_icon: &str) -> Option<Vec<u8>> {
    let dir = icon_cache_dir();
    let cache_file = dir.join(cache_key(raw_display_icon));
    if cache_file.is_file() {
        if let Ok(bytes) = std::fs::read(&cache_file) {
            // A torn or planted non-PNG cache file must not be served as an icon.
            if looks_like_png(&bytes) {
                return Some(bytes);
            }
        }
    }
    let (path, index) = parse_display_icon(raw_display_icon);
    if path.is_empty() {
        return None;
    }
    let png = extract_from_path(&path, index)?;
    let _ = std::fs::create_dir_all(&dir);
    // tmp + rename: a crash mid-write must not leave a torn PNG that would
    // poison the cache key until the fingerprint changes.
    let _ = write_cache_png(&cache_file, &png);
    prune_stale_cache(&dir, &cache_file);
    Some(png)
}

fn write_cache_png(cache_file: &std::path::Path, png: &[u8]) -> std::io::Result<()> {
    // pid alone is not enough: two threads rendering the same app icon
    // concurrently would share one tmp file and tear the PNG — add a nonce.
    static TMP_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = TMP_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp_name = cache_file.as_os_str().to_owned();
    tmp_name.push(format!(".{}_{}.png.tmp", std::process::id(), nonce));
    let tmp = std::path::PathBuf::from(tmp_name);
    let result = std::fs::write(&tmp, png).and_then(|_| std::fs::rename(&tmp, cache_file));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Drop cache PNGs left behind by older source fingerprints (age > 7 days).
fn prune_stale_cache(dir: &std::path::Path, keep: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let keep_name = keep.file_name();
    for ent in rd.flatten() {
        let p = ent.path();
        let is_tmp = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".png.tmp"));
        if p.extension().and_then(|e| e.to_str()) != Some("png") && !is_tmp {
            continue;
        }
        if p.file_name() == keep_name {
            continue;
        }
        if let Ok(meta) = p.metadata() {
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = modified.elapsed() {
                    if age.as_secs() > 7 * 24 * 3600 {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn extract_from_path(_path: &str, _index: i32) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn extract_from_path(path: &str, index: i32) -> Option<Vec<u8>> {
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    if !Path::new(path).exists() {
        return None;
    }

    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut large = windows::Win32::UI::WindowsAndMessaging::HICON::default();
        let mut small = windows::Win32::UI::WindowsAndMessaging::HICON::default();
        let count = ExtractIconExW(
            PCWSTR(path_w.as_ptr()),
            index,
            Some(&mut large),
            Some(&mut small),
            1,
        );

        // The unused small handle belongs to us even when large is invalid.
        if !small.is_invalid() {
            let _ = DestroyIcon(small);
        }
        let hicon = if count > 0 && !large.is_invalid() {
            large
        } else {
            // Fallback: shell file icon (works for many non-PE paths)
            let mut shfi: SHFILEINFOW = std::mem::zeroed();
            let ok = SHGetFileInfoW(
                PCWSTR(path_w.as_ptr()),
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut shfi),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            );
            if ok == 0 || shfi.hIcon.is_invalid() {
                return None;
            }
            shfi.hIcon
        };

        let png = hicon_to_png(hicon, ICON_SIZE);
        let _ = DestroyIcon(hicon);
        png
    }
}

#[cfg(windows)]
fn hicon_to_png(
    hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
    size: i32,
) -> Option<Vec<u8>> {
    use std::mem::{size_of, zeroed};
    use std::ptr;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, RGBQUAD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};

    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memdc = CreateCompatibleDC(screen);
        let _ = ReleaseDC(None, screen);
        if memdc.is_invalid() {
            return None;
        }

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size;
        bmi.bmiHeader.biHeight = -size; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;
        bmi.bmiColors = [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }];

        let mut bits: *mut std::ffi::c_void = ptr::null_mut();
        let hbitmap = match CreateDIBSection(memdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(b) if !b.is_invalid() && !bits.is_null() => b,
            _ => {
                let _ = DeleteDC(memdc);
                return None;
            }
        };

        let old = SelectObject(memdc, HGDIOBJ(hbitmap.0));
        let _ = DrawIconEx(memdc, 0, 0, hicon, size, size, 0, None, DI_NORMAL);

        let stride = (size as usize) * 4;
        let mut bgra = vec![0u8; stride * size as usize];
        std::ptr::copy_nonoverlapping(bits as *const u8, bgra.as_mut_ptr(), bgra.len());

        let _ = SelectObject(memdc, old);
        let _ = DeleteObject(HGDIOBJ(hbitmap.0));
        let _ = DeleteDC(memdc);

        encode_png_bgra(&bgra, size as u32, size as u32)
    }
}

/// Minimal PNG encoder (RGBA8).
fn encode_png_bgra(bgra: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        // Windows DIB is BGRA
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_publish_removes_tmp_and_prune_includes_stale_tmp() {
        let dir = std::env::temp_dir().join(format!("remova-r23-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let blocked = dir.join("blocked.png");
        std::fs::create_dir_all(&blocked).unwrap();
        assert!(write_cache_png(&blocked, b"png").is_err());
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".png.tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no tmp leftovers: {leftovers:?}");
        let keep = dir.join("keep.png");
        let stale = dir.join("stale.png.tmp");
        let unrelated = dir.join("unrelated.tmp");
        let recent = dir.join("recent.png.tmp");
        for p in [&keep, &stale, &unrelated, &recent] {
            std::fs::write(p, b"x").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 86400);
        for p in [&keep, &stale, &unrelated] {
            std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(old))
                .unwrap();
        }
        prune_stale_cache(&dir, &keep);
        assert!(!stale.exists());
        assert!(keep.exists());
        assert!(unrelated.exists());
        assert!(recent.exists());
        std::fs::remove_file(keep).unwrap();
        std::fs::remove_file(unrelated).unwrap();
        std::fs::remove_file(recent).unwrap();
        std::fs::remove_dir(blocked).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn png_magic_rejects_torn_or_planted_files() {
        let mut ok = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        ok.extend_from_slice(b"rest-of-image");
        assert!(looks_like_png(&ok));
        assert!(!looks_like_png(&[]));
        assert!(!looks_like_png(&[0x89, 0x50]));
        assert!(!looks_like_png(b"<html>evil</html>"));
        let mut torn = vec![0x89, 0x50, 0x4E, 0x47];
        torn.extend_from_slice(b"half-written");
        assert!(!looks_like_png(&torn));
    }
    #[test]
    fn encode_png_signature() {
        let bgra = [
            0u8, 0, 255, 255, 0, 255, 0, 128, 255, 0, 0, 255, 255, 255, 255, 255,
        ];
        let png = super::encode_png_bgra(&bgra, 2, 2).expect("png encode");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[cfg(windows)]
    #[test]
    fn extract_shell32_icon() {
        let raw = r"C:\Windows\System32\shell32.dll,0";
        if let Some(bytes) = super::extract_icon_png(raw) {
            assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        }
    }

    #[test]
    fn cache_key_changes_with_source_fingerprint() {
        let dir = std::env::temp_dir().join("remova-icon-cache-key-test");
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("app.exe");
        std::fs::write(&exe, b"v1").unwrap();
        let raw = format!("{},0", exe.display());
        let k1 = super::cache_key(&raw);
        std::fs::write(&exe, b"v2-with-new-icon-bytes").unwrap();
        let k2 = super::cache_key(&raw);
        assert_ne!(
            k1, k2,
            "cache key must change when the icon source file changes"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_key_stable_for_missing_source() {
        let k = super::cache_key(r"C:\no\such\file.dll,0");
        assert!(k.ends_with(".png"));
    }

    /// Two threads writing the SAME cache entry must not share a tmp file:
    /// each write gets its own nonce so both renames succeed and the final
    /// PNG is always one whole write (never a tear).
    #[test]
    fn concurrent_writes_stay_whole_and_leave_no_tmp() {
        let dir = std::env::temp_dir().join(format!("remova-r25-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("app.png");
        let whole_a = vec![0xAu8; 8192];
        let whole_b = vec![0xBu8; 8192];
        std::thread::scope(|s| {
            let t1 = s.spawn(|| write_cache_png(&cache, &whole_a));
            let t2 = s.spawn(|| write_cache_png(&cache, &whole_b));
            t1.join().unwrap().expect("write A must succeed");
            t2.join().unwrap().expect("write B must succeed");
        });
        let written = std::fs::read(&cache).unwrap();
        assert!(
            written == whole_a || written == whole_b,
            "final PNG must be one whole write, not a tear"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".png.tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no tmp leftovers: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
