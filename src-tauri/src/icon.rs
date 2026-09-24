//! Extract a file/app icon as PNG bytes (Windows). Optional disk cache (PERF-4).

use crate::apps::parse_display_icon;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

const ICON_SIZE: i32 = 32;

fn icon_cache_dir() -> PathBuf {
    // REV-SUP-04: never fall back to C:\Users\Public (shared writable).
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let temp = std::env::var("TEMP").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into());
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
            if !bytes.is_empty() {
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
    let _ = std::fs::write(&cache_file, &png);
    prune_stale_cache(&dir, &cache_file);
    Some(png)
}

/// Drop cache PNGs left behind by older source fingerprints (age > 7 days).
fn prune_stale_cache(dir: &std::path::Path, keep: &std::path::Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let keep_name = keep.file_name();
    for ent in rd.flatten() {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) != Some("png") {
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

        let hicon = if count > 0 && !large.is_invalid() {
            if !small.is_invalid() {
                let _ = DestroyIcon(small);
            }
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
}
