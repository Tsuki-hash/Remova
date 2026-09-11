//! Extract a file/app icon as PNG bytes (Windows).

use crate::apps::parse_display_icon;

const ICON_SIZE: i32 = 32;

/// Extract icon from `DisplayIcon` raw value (`path` or `path,index`) as PNG bytes.
pub fn extract_icon_png(raw_display_icon: &str) -> Option<Vec<u8>> {
    let (path, index) = parse_display_icon(raw_display_icon);
    if path.is_empty() {
        return None;
    }
    extract_from_path(&path, index)
}

#[cfg(not(windows))]
fn extract_from_path(_path: &str, _index: i32) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn extract_from_path(path: &str, index: i32) -> Option<Vec<u8>> {
    use std::path::Path;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ExtractIconExW, SHGetFileInfoW, SHGFI_ICON, SHGFI_LARGEICON, SHFILEINFOW};
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
}
