//! Program icons as PNG bytes: extracted from an executable / DLL / `.ico` resource, or from the
//! file's Shell association when it has no embedded icon.

use std::path::Path;

/// Extract icon `index` (negative = resource id, as in `DisplayIcon` values) at `size` pixels and
/// encode it as PNG. Returns `None` when the file has no usable icon.
pub fn icon_png_from_file(path: &Path, index: i32, size: u32) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        imp::extract(path, index, size)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, index, size);
        None
    }
}

/// Minimal PNG encoder (RGBA8, stored/uncompressed deflate). Icons are tiny, so compression is
/// not worth a dependency.
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = Vec::with_capacity(4 + data.len());
        body.extend_from_slice(kind);
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }

    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks_exact(stride).take(height as usize) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }

    // zlib stream with stored blocks (max 65535 bytes each) + Adler-32.
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        zlib.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
    };

    pub fn extract(path: &Path, index: i32, size: u32) -> Option<Vec<u8>> {
        let icon = embedded_icon(path, index, size).or_else(|| shell_icon(path))?;
        let png = icon_to_png(icon);
        unsafe {
            let _ = DestroyIcon(icon);
        }
        png
    }

    fn embedded_icon(path: &Path, index: i32, size: u32) -> Option<HICON> {
        let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().collect();
        if wide.len() >= 260 {
            return None;
        }
        let mut name = [0u16; 260];
        name[..wide.len()].copy_from_slice(&wide);
        let mut icons = [HICON::default()];
        let count = unsafe {
            PrivateExtractIconsW(&name, index, size as i32, size as i32, Some(&mut icons), None, 0)
        };
        // 0xFFFFFFFF = file not found / unreadable.
        (count != 0 && count != u32::MAX && !icons[0].is_invalid()).then_some(icons[0])
    }

    fn shell_icon(path: &Path) -> Option<HICON> {
        let mut info = SHFILEINFOW::default();
        let ok = unsafe {
            SHGetFileInfoW(
                &HSTRING::from(path.as_os_str()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        (ok != 0 && !info.hIcon.is_invalid()).then_some(info.hIcon)
    }

    fn bitmap_pixels(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
        let mut bm = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                HGDIOBJ(bitmap.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut core::ffi::c_void),
            )
        };
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 || bm.bmWidth > 256 {
            return None;
        }
        let (width, height) = (bm.bmWidth as u32, bm.bmHeight as u32);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(None) };
        let lines = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                height,
                Some(pixels.as_mut_ptr() as *mut core::ffi::c_void),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        unsafe {
            let _ = DeleteDC(dc);
        }
        (lines > 0).then_some((width, height, pixels))
    }

    fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
        let mut info = ICONINFO::default();
        unsafe { GetIconInfo(icon, &mut info) }.ok()?;
        let color = bitmap_pixels(info.hbmColor);
        let mask = bitmap_pixels(info.hbmMask);
        unsafe {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        let (width, height, mut bgra) = color?;
        let has_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
        for (i, px) in bgra.chunks_exact_mut(4).enumerate() {
            px.swap(0, 2); // BGRA → RGBA
            if !has_alpha {
                // Legacy icons without an alpha channel: the AND mask marks transparent pixels.
                let transparent = mask
                    .as_ref()
                    .and_then(|(_, _, m)| m.get(i * 4))
                    .is_some_and(|v| *v != 0);
                px[3] = if transparent { 0 } else { 255 };
            }
        }
        Some(encode_png(width, height, &bgra))
    }

    fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        super::encode_png_rgba(width, height, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::encode_png_rgba;

    #[test]
    fn encodes_valid_png_signature_and_chunks() {
        let png = encode_png_rgba(2, 2, &[255u8; 16]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}
