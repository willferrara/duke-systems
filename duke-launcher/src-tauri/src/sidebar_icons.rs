use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager};

pub(crate) const SIDEBAR_ICON_SIZE: u32 = 32;
static ICON_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarIconSource {
    pub id: String,
    pub exe_path: String,
    pub cover_url: String,
    pub icon_path: Option<String>,
    pub steam_app_id: Option<u64>,
    pub app_id: Option<u32>,
}

impl From<&crate::GameEntry> for SidebarIconSource {
    fn from(entry: &crate::GameEntry) -> Self {
        Self {
            id: entry.id.clone(),
            exe_path: entry.exe_path.clone(),
            cover_url: entry.cover_url.clone(),
            icon_path: entry.icon_path.clone(),
            steam_app_id: entry.steam_app_id,
            app_id: entry.app_id,
        }
    }
}

/// Sanitizes a `game_id` into a safe file stem for `<app_data_dir>/sidebar_icons/<stem>.png`.
/// Preserves short alphanumeric/`_`/`-` IDs verbatim and hash-truncates long IDs to avoid `MAX_PATH`.
pub(crate) fn safe_sidebar_icon_stem(game_id: &str) -> String {
    let cleaned: String = game_id
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return "game_unknown".to_string();
    }
    if cleaned.len() <= 100 {
        return cleaned;
    }
    let mut hasher = Sha256::new();
    hasher.update(game_id.as_bytes());
    let digest = hasher.finalize();
    let suffix = hex::encode(&digest[..8]);
    let prefix: String = cleaned.chars().take(80).collect();
    format!("{}_{}", prefix, suffix)
}

pub(crate) fn get_sidebar_icons_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let root = crate::resolve_data_root_dir(app)?;
    let dir = root.join("sidebar_icons");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create sidebar_icons directory: {}", e))?;
    }
    Ok(dir)
}

pub(crate) fn get_sidebar_icons_dir_standalone() -> Result<PathBuf, String> {
    let root = crate::resolve_data_root_dir_standalone()?;
    let dir = root.join("sidebar_icons");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create sidebar_icons directory: {}", e))?;
    }
    Ok(dir)
}

pub(crate) fn cached_sidebar_icon_path(sidebar_icons_dir: &Path, game_id: &str) -> PathBuf {
    sidebar_icons_dir.join(format!("{}.png", safe_sidebar_icon_stem(game_id)))
}

pub(crate) fn invalidate_sidebar_icon(app: &AppHandle, game_id: &str) {
    if let Ok(dir) = get_sidebar_icons_dir(app) {
        let _ = fs::remove_file(cached_sidebar_icon_path(&dir, game_id));
    }
}

pub(crate) fn invalidate_sidebar_icon_standalone(game_id: &str) {
    if let Ok(dir) = get_sidebar_icons_dir_standalone() {
        let _ = fs::remove_file(cached_sidebar_icon_path(&dir, game_id));
    }
}

pub(crate) fn atomic_write_png_bytes(target: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed creating icon cache dir: {}", e))?;
        }
    }
    let seq = ICON_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(
        ".{}.tmp.{}.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("icon.png"),
        std::process::id(),
        seq
    );
    let tmp_path = target.with_file_name(tmp_name);
    {
        use std::io::Write;
        let mut f = File::create(&tmp_path)
            .map_err(|e| format!("Failed creating temp icon file: {}", e))?;
        f.write_all(data)
            .map_err(|e| format!("Failed writing temp icon file: {}", e))?;
        let _ = f.sync_all();
    }
    if target.exists() {
        let _ = fs::remove_file(target);
    }
    if let Err(e) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed finalizing cached icon PNG: {}", e));
    }
    Ok(())
}

pub(crate) fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "Image dimensions overflow".to_string())?;
    if rgba.len() != expected || width == 0 || height == 0 {
        return Err("Invalid RGBA buffer dimensions".to_string());
    }
    let mut out = Vec::with_capacity(2048);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("PNG header error: {}", e))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("PNG encode error: {}", e))?;
    }
    Ok(out)
}

fn decode_png_to_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let buf_size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).ok()?;
    let w = info.width;
    let h = info.height;
    let frame = &buf[..info.buffer_size()];
    if w == 0 || h == 0 {
        return None;
    }
    let pixel_count = (w as usize).checked_mul(h as usize)?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            if frame.len() < pixel_count * 4 {
                return None;
            }
            frame[..pixel_count * 4].to_vec()
        }
        png::ColorType::Rgb => {
            if frame.len() < pixel_count * 3 {
                return None;
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for rgb in frame[..pixel_count * 3].chunks_exact(3) {
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            if frame.len() < pixel_count * 2 {
                return None;
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for ga in frame[..pixel_count * 2].chunks_exact(2) {
                out.extend_from_slice(&[ga[0], ga[0], ga[0], ga[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            if frame.len() < pixel_count {
                return None;
            }
            let mut out = Vec::with_capacity(pixel_count * 4);
            for &g in &frame[..pixel_count] {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        png::ColorType::Indexed => return None,
    };
    Some((rgba, w, h))
}

#[cfg(target_os = "windows")]
fn decode_wic_to_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource,
        IWICImagingFactory, WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };

    if bytes.is_empty() {
        return None;
    }

    struct ComInitGuard(bool);
    impl Drop for ComInitGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _com_guard = ComInitGuard(hr.is_ok());

        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;

        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let src_bitmap: IWICBitmapSource = windows::core::Interface::cast(&frame).ok()?;
        let rgba_bitmap =
            WICConvertBitmapSource(&GUID_WICPixelFormat32bppRGBA, &src_bitmap).ok()?;

        let mut width = 0u32;
        let mut height = 0u32;
        rgba_bitmap.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return None;
        }
        let stride = width.checked_mul(4)?;
        let total = (stride as usize).checked_mul(height as usize)?;
        let mut rgba = vec![0u8; total];
        rgba_bitmap
            .CopyPixels(std::ptr::null(), stride, &mut rgba)
            .ok()?;
        Some((rgba, width, height))
    }
}

#[cfg(not(target_os = "windows"))]
fn decode_wic_to_rgba(_bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    None
}

pub(crate) fn decode_image_bytes_to_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if bytes.is_empty() {
        return None;
    }
    decode_png_to_rgba(bytes).or_else(|| decode_wic_to_rgba(bytes))
}

/// Center-crops an RGBA image into the largest centered square `side × side`.
pub(crate) fn center_crop_square_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Option<(Vec<u8>, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if rgba.len() < expected {
        return None;
    }
    if width == height {
        return Some((rgba[..expected].to_vec(), width));
    }
    let side = width.min(height);
    let x0 = (width - side) / 2;
    let y0 = (height - side) / 2;
    let side_usize = side as usize;
    let src_stride = (width as usize) * 4;
    let dst_stride = side_usize * 4;
    let mut out = vec![0u8; side_usize * dst_stride];
    for row in 0..side_usize {
        let src_start = (y0 as usize + row) * src_stride + (x0 as usize) * 4;
        let dst_start = row * dst_stride;
        out[dst_start..dst_start + dst_stride]
            .copy_from_slice(&rgba[src_start..src_start + dst_stride]);
    }
    Some((out, side))
}

#[inline]
fn lanczos3_kernel(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 1e-8 {
        1.0
    } else if ax >= 3.0 {
        0.0
    } else {
        let pi_x = std::f64::consts::PI * ax;
        let pi_x_3 = pi_x / 3.0;
        (pi_x.sin() / pi_x) * (pi_x_3.sin() / pi_x_3)
    }
}

/// Separable two-pass premultiplied-alpha Lanczos3 resampler.
pub(crate) fn lanczos3_resize_rgba(
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return None;
    }
    let expected = (src_w as usize).checked_mul(src_h as usize)?.checked_mul(4)?;
    if rgba.len() < expected {
        return None;
    }
    if src_w == dst_w && src_h == dst_h {
        return Some(rgba[..expected].to_vec());
    }

    let sw = src_w as usize;
    let sh = src_h as usize;
    let dw = dst_w as usize;
    let dh = dst_h as usize;

    // Convert to premultiplied linear float RGBA `[r*a, g*a, b*a, a]` in 0..1
    let mut premul = vec![0.0f64; sw * sh * 4];
    for (dst_px, src_px) in premul.chunks_exact_mut(4).zip(rgba[..expected].chunks_exact(4)) {
        let a = src_px[3] as f64 / 255.0;
        dst_px[0] = (src_px[0] as f64 / 255.0) * a;
        dst_px[1] = (src_px[1] as f64 / 255.0) * a;
        dst_px[2] = (src_px[2] as f64 / 255.0) * a;
        dst_px[3] = a;
    }

    // Horizontal pass: sw × sh -> dw × sh
    let x_scale = (sw as f64) / (dw as f64);
    let x_support = if x_scale > 1.0 { 3.0 * x_scale } else { 3.0 };
    let x_inv_scale = if x_scale > 1.0 { 1.0 / x_scale } else { 1.0 };

    let mut horiz = vec![0.0f64; dw * sh * 4];
    for dx in 0..dw {
        let center = (dx as f64 + 0.5) * x_scale - 0.5;
        let left = (center - x_support).floor().max(0.0) as isize;
        let right = (center + x_support).ceil().min((sw - 1) as f64) as isize;

        let mut weights = Vec::with_capacity((right - left + 1).max(1) as usize);
        let mut w_sum = 0.0f64;
        for sx in left..=right {
            let w = lanczos3_kernel((sx as f64 - center) * x_inv_scale);
            weights.push((sx as usize, w));
            w_sum += w;
        }
        if w_sum.abs() < 1e-12 {
            let clamped = center.round().clamp(0.0, (sw - 1) as f64) as usize;
            weights.clear();
            weights.push((clamped, 1.0));
            w_sum = 1.0;
        }

        for sy in 0..sh {
            let mut acc = [0.0f64; 4];
            let row_base = sy * sw * 4;
            for &(sx, w) in &weights {
                let idx = row_base + sx * 4;
                acc[0] += premul[idx] * w;
                acc[1] += premul[idx + 1] * w;
                acc[2] += premul[idx + 2] * w;
                acc[3] += premul[idx + 3] * w;
            }
            let out_idx = (sy * dw + dx) * 4;
            horiz[out_idx] = acc[0] / w_sum;
            horiz[out_idx + 1] = acc[1] / w_sum;
            horiz[out_idx + 2] = acc[2] / w_sum;
            horiz[out_idx + 3] = acc[3] / w_sum;
        }
    }

    // Vertical pass: dw × sh -> dw × dh
    let y_scale = (sh as f64) / (dh as f64);
    let y_support = if y_scale > 1.0 { 3.0 * y_scale } else { 3.0 };
    let y_inv_scale = if y_scale > 1.0 { 1.0 / y_scale } else { 1.0 };

    let mut out = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        let center = (dy as f64 + 0.5) * y_scale - 0.5;
        let top = (center - y_support).floor().max(0.0) as isize;
        let bottom = (center + y_support).ceil().min((sh - 1) as f64) as isize;

        let mut weights = Vec::with_capacity((bottom - top + 1).max(1) as usize);
        let mut w_sum = 0.0f64;
        for sy in top..=bottom {
            let w = lanczos3_kernel((sy as f64 - center) * y_inv_scale);
            weights.push((sy as usize, w));
            w_sum += w;
        }
        if w_sum.abs() < 1e-12 {
            let clamped = center.round().clamp(0.0, (sh - 1) as f64) as usize;
            weights.clear();
            weights.push((clamped, 1.0));
            w_sum = 1.0;
        }

        for dx in 0..dw {
            let mut acc = [0.0f64; 4];
            for &(sy, w) in &weights {
                let idx = (sy * dw + dx) * 4;
                acc[0] += horiz[idx] * w;
                acc[1] += horiz[idx + 1] * w;
                acc[2] += horiz[idx + 2] * w;
                acc[3] += horiz[idx + 3] * w;
            }
            let a = (acc[3] / w_sum).clamp(0.0, 1.0);
            let (r, g, b) = if a > 1e-6 {
                (
                    ((acc[0] / w_sum) / a).clamp(0.0, 1.0),
                    ((acc[1] / w_sum) / a).clamp(0.0, 1.0),
                    ((acc[2] / w_sum) / a).clamp(0.0, 1.0),
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            let out_idx = (dy * dw + dx) * 4;
            out[out_idx] = (r * 255.0).round() as u8;
            out[out_idx + 1] = (g * 255.0).round() as u8;
            out[out_idx + 2] = (b * 255.0).round() as u8;
            out[out_idx + 3] = (a * 255.0).round() as u8;
        }
    }

    Some(out)
}

#[cfg(target_os = "windows")]
fn hicon_to_rgba(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<(Vec<u8>, u32, u32)> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::Win32::Graphics::Gdi::{
        BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    struct BitmapGuard(HBITMAP);
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(self.0.0));
                }
            }
        }
    }

    struct DcGuard(HDC);
    impl Drop for DcGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = DeleteDC(self.0);
                }
            }
        }
    }

    fn top_down_bgra_bitmap_info(width: i32, height: i32) -> BITMAPINFO {
        BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative height requests a top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        }
    }

    if hicon.is_invalid() {
        return None;
    }

    unsafe {
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return None;
        }
        let color_guard = BitmapGuard(icon_info.hbmColor);
        let mask_guard = BitmapGuard(icon_info.hbmMask);

        let hdc = DcGuard(CreateCompatibleDC(None));
        if hdc.0.is_invalid() {
            return None;
        }

        if !color_guard.0.is_invalid() {
            let mut bmp = BITMAP::default();
            if GetObjectW(
                HGDIOBJ(color_guard.0.0),
                size_of::<BITMAP>() as i32,
                Some((&mut bmp as *mut BITMAP).cast::<c_void>()),
            ) == 0
            {
                return None;
            }
            let width = bmp.bmWidth;
            let height = bmp.bmHeight.abs();
            if width <= 0 || height <= 0 || width > 1024 || height > 1024 {
                return None;
            }

            let mut bmi = top_down_bgra_bitmap_info(width, height);

            let byte_len = (width as usize) * (height as usize) * 4;
            let mut pixels = vec![0u8; byte_len];
            let scan_lines = GetDIBits(
                hdc.0,
                color_guard.0,
                0,
                height as u32,
                Some(pixels.as_mut_ptr().cast::<c_void>()),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            if scan_lines == 0 {
                return None;
            }

            // Convert BGRA -> RGBA
            for px in pixels.chunks_exact_mut(4) {
                px.swap(0, 2);
            }

            let has_alpha = pixels.chunks_exact(4).any(|px| px[3] != 0);
            if !has_alpha {
                if !mask_guard.0.is_invalid() {
                    let mut mask_pixels = vec![0u8; byte_len];
                    let mask_lines = GetDIBits(
                        hdc.0,
                        mask_guard.0,
                        0,
                        height as u32,
                        Some(mask_pixels.as_mut_ptr().cast::<c_void>()),
                        &mut bmi,
                        DIB_RGB_COLORS,
                    );
                    if mask_lines != 0 {
                        for (px, mpx) in
                            pixels.chunks_exact_mut(4).zip(mask_pixels.chunks_exact(4))
                        {
                            // In the AND mask: 0 = opaque icon pixel, non-zero = transparent background
                            if mpx[0] == 0 {
                                px[3] = 255;
                            } else {
                                px[0] = 0;
                                px[1] = 0;
                                px[2] = 0;
                                px[3] = 0;
                            }
                        }
                    } else {
                        for px in pixels.chunks_exact_mut(4) {
                            px[3] = 255;
                        }
                    }
                } else {
                    for px in pixels.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                }
            }

            if !pixels.chunks_exact(4).any(|px| px[3] > 0) {
                return None;
            }
            return Some((pixels, width as u32, height as u32));
        }

        // Monochrome icon (hbmColor is NULL; hbmMask height = 2 * height: top AND mask, bottom XOR mask)
        if !mask_guard.0.is_invalid() {
            let mut bmp = BITMAP::default();
            if GetObjectW(
                HGDIOBJ(mask_guard.0.0),
                size_of::<BITMAP>() as i32,
                Some((&mut bmp as *mut BITMAP).cast::<c_void>()),
            ) == 0
            {
                return None;
            }
            let width = bmp.bmWidth;
            let full_height = bmp.bmHeight.abs();
            let height = full_height / 2;
            if width <= 0 || height <= 0 || width > 1024 || height > 1024 {
                return None;
            }

            let mut bmi = top_down_bgra_bitmap_info(width, full_height);
            let half_bytes = (width as usize) * (height as usize) * 4;
            let mut mask_pixels = vec![0u8; half_bytes * 2];
            if GetDIBits(
                hdc.0,
                mask_guard.0,
                0,
                full_height as u32,
                Some(mask_pixels.as_mut_ptr().cast::<c_void>()),
                &mut bmi,
                DIB_RGB_COLORS,
            ) == 0
            {
                return None;
            }
            let (and_slice, xor_slice) = mask_pixels.split_at(half_bytes);
            let mut rgba = vec![0u8; half_bytes];
            for ((out_px, and_px), xor_px) in rgba
                .chunks_exact_mut(4)
                .zip(and_slice.chunks_exact(4))
                .zip(xor_slice.chunks_exact(4))
            {
                if and_px[0] == 0 {
                    let v = if xor_px[0] != 0 { 255 } else { 0 };
                    out_px[0] = v;
                    out_px[1] = v;
                    out_px[2] = v;
                    out_px[3] = 255;
                }
            }
            if !rgba.chunks_exact(4).any(|px| px[3] > 0) {
                return None;
            }
            return Some((rgba, width as u32, height as u32));
        }

        None
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn extract_exe_icon_rgba(exe_path: &str) -> Option<(Vec<u8>, u32, u32)> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHFILEINFOW, SHGFI_ICON, SHGFI_ICONLOCATION, SHGFI_LARGEICON,
        SHGetFileInfoW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    struct IconGuard(HICON);
    impl Drop for IconGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = DestroyIcon(self.0);
                }
            }
        }
    }

    let trimmed = exe_path.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    if !path.is_file() {
        return None;
    }

    let wide: Vec<u16> = OsStr::new(trimmed)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // Primary path: direct PE/ICO embedded resource extraction via ExtractIconExW
        let mut hicon_large = HICON::default();
        let mut hicon_small = HICON::default();
        let count = ExtractIconExW(
            PCWSTR(wide.as_ptr()),
            0,
            Some(&mut hicon_large),
            Some(&mut hicon_small),
            1,
        );
        let large_guard = IconGuard(hicon_large);
        let small_guard = IconGuard(hicon_small);

        if count > 0 {
            if !large_guard.0.is_invalid() {
                if let Some(rgba) = hicon_to_rgba(large_guard.0) {
                    return Some(rgba);
                }
            }
            if !small_guard.0.is_invalid() {
                if let Some(rgba) = hicon_to_rgba(small_guard.0) {
                    return Some(rgba);
                }
            }
        }

        // Secondary path: SHGetFileInfoW (only if the shell resolves a real icon location,
        // NOT the generic shell32.dll / imageres.dll default executable icon)
        let mut loc_info = SHFILEINFOW::default();
        let loc_ok = SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut loc_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICONLOCATION,
        );
        if loc_ok != 0 {
            let end = loc_info
                .szDisplayName
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(loc_info.szDisplayName.len());
            let icon_loc = String::from_utf16_lossy(&loc_info.szDisplayName[..end])
                .trim()
                .to_lowercase();
            let is_generic_shell_icon = icon_loc.is_empty()
                || icon_loc.ends_with("shell32.dll")
                || icon_loc.ends_with("imageres.dll")
                || icon_loc.ends_with("shellstyle.dll");
            if !is_generic_shell_icon {
                let mut shfi = SHFILEINFOW::default();
                let res = SHGetFileInfoW(
                    PCWSTR(wide.as_ptr()),
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    Some(&mut shfi),
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                );
                let sh_guard = IconGuard(shfi.hIcon);
                if res != 0 && !sh_guard.0.is_invalid() {
                    if let Some(rgba) = hicon_to_rgba(sh_guard.0) {
                        return Some(rgba);
                    }
                }
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn extract_exe_icon_rgba(_exe_path: &str) -> Option<(Vec<u8>, u32, u32)> {
    None
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut lut = [-1i8; 256];
    for (idx, &b) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        lut[b as usize] = idx as i8;
    }

    let cleaned: Vec<u8> = input
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() || cleaned.len() % 4 != 0 {
        return None;
    }

    let mut out = Vec::with_capacity((cleaned.len() / 4) * 3);
    for chunk in cleaned.chunks_exact(4) {
        let a = lut[chunk[0] as usize];
        let b = lut[chunk[1] as usize];
        if a < 0 || b < 0 {
            return None;
        }
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        if c_pad && !d_pad {
            return None;
        }
        let c = if c_pad { 0 } else { lut[chunk[2] as usize] };
        let d = if d_pad { 0 } else { lut[chunk[3] as usize] };
        if c < 0 || d < 0 {
            return None;
        }
        let triple = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push(((triple >> 16) & 0xFF) as u8);
        if !c_pad {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if !d_pad {
            out.push((triple & 0xFF) as u8);
        }
    }
    Some(out)
}

fn decode_data_uri_to_rgba(data_uri: &str) -> Option<(Vec<u8>, u32, u32)> {
    let trimmed = data_uri.trim();
    if !trimmed.starts_with("data:image/") {
        return None;
    }
    let (header, payload) = trimmed.split_once(',')?;
    let lower_header = header.to_ascii_lowercase();
    if lower_header.contains("svg+xml") || !lower_header.contains(";base64") {
        return None;
    }
    let bytes = decode_base64(payload)?;
    decode_image_bytes_to_rgba(&bytes)
}

fn try_decode_cover_file(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    if !path.is_file() {
        return None;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "cover" {
        let text = fs::read_to_string(path).ok()?;
        return decode_data_uri_to_rgba(&text);
    }
    let bytes = fs::read(path).ok()?;
    decode_image_bytes_to_rgba(&bytes)
}

fn load_cover_fallback_rgba(
    entry: &SidebarIconSource,
    covers_dir: &Path,
) -> Option<(Vec<u8>, u32, u32)> {
    if let Some(ref custom_icon) = entry.icon_path {
        let p = Path::new(custom_icon.trim());
        if let Some(img) = try_decode_cover_file(p) {
            return Some(img);
        }
    }

    let cover_safe_id: String = entry
        .id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    let stem = safe_sidebar_icon_stem(&entry.id);

    let mut keys: Vec<String> = Vec::new();
    if let Some(cache_key) = entry.cover_url.trim().strip_prefix("cover-cache://") {
        let k = cache_key.trim();
        if !k.is_empty() {
            keys.push(k.to_string());
        }
    }
    if !cover_safe_id.is_empty() && !keys.contains(&cover_safe_id) {
        keys.push(cover_safe_id);
    }
    if !keys.contains(&stem) {
        keys.push(stem);
    }
    if let Some(aid) = entry
        .app_id
        .map(|v| v as u64)
        .or(entry.steam_app_id)
        .filter(|&v| v > 0)
    {
        keys.push(aid.to_string());
        keys.push(format!("steam_{}", aid));
    }

    const EXTS: [&str; 7] = ["cover", "png", "jpg", "jpeg", "webp", "bmp", "ico"];
    for key in &keys {
        for ext in EXTS {
            let candidate = covers_dir.join(format!("{}.{}", key, ext));
            if let Some(img) = try_decode_cover_file(&candidate) {
                return Some(img);
            }
        }
    }

    if entry.cover_url.trim().starts_with("data:image/") {
        if let Some(img) = decode_data_uri_to_rgba(&entry.cover_url) {
            return Some(img);
        }
    }

    None
}

/// Resolves a game's `32×32` PNG sidebar thumbnail path in strict priority order:
/// 1. Existing `<sidebar_icons_dir>/<game_id>.png` cache hit.
/// 2. Win32 primary `.exe` icon extraction -> `32×32` Lanczos3 PNG.
/// 3. Fallback center-crop of `<covers_dir>` cover/icon for `game_id`/`appid` -> `32×32` Lanczos3 PNG.
pub(crate) fn resolve_sidebar_icon_in_dirs(
    entry: &SidebarIconSource,
    sidebar_icons_dir: &Path,
    covers_dir: &Path,
) -> Option<PathBuf> {
    let dest_path = cached_sidebar_icon_path(sidebar_icons_dir, &entry.id);
    if let Ok(meta) = fs::metadata(&dest_path) {
        if meta.is_file() && meta.len() > 8 {
            return Some(dest_path);
        }
    }

    let resized_rgba = if let Some((rgba, w, h)) = extract_exe_icon_rgba(&entry.exe_path) {
        lanczos3_resize_rgba(&rgba, w, h, SIDEBAR_ICON_SIZE, SIDEBAR_ICON_SIZE)?
    } else if let Some((rgba, w, h)) = load_cover_fallback_rgba(entry, covers_dir) {
        let (sq, side) = center_crop_square_rgba(&rgba, w, h)?;
        lanczos3_resize_rgba(&sq, side, side, SIDEBAR_ICON_SIZE, SIDEBAR_ICON_SIZE)?
    } else {
        return None;
    };

    let png_bytes = encode_rgba_png(&resized_rgba, SIDEBAR_ICON_SIZE, SIDEBAR_ICON_SIZE).ok()?;
    atomic_write_png_bytes(&dest_path, &png_bytes).ok()?;
    Some(dest_path)
}

#[tauri::command]
pub(crate) async fn get_sidebar_icons(
    app: AppHandle,
    game_ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    if game_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || -> Result<HashMap<String, String>, String> {
        let sidebar_icons_dir = get_sidebar_icons_dir(&app_clone)?;
        let covers_dir = crate::get_covers_dir(&app_clone)?;

        let mut seen = HashSet::with_capacity(game_ids.len());
        let mut unique_ids = Vec::with_capacity(game_ids.len());
        for raw_id in game_ids {
            let trimmed = raw_id.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                unique_ids.push(trimmed.to_string());
            }
        }

        let mut result = HashMap::with_capacity(unique_ids.len());
        let mut missing_ids = Vec::new();

        // Priority 1: Fast path for already-cached 32×32 PNG thumbnails (zero library/cover parsing)
        for gid in unique_ids {
            let candidate = cached_sidebar_icon_path(&sidebar_icons_dir, &gid);
            if let Ok(meta) = fs::metadata(&candidate) {
                if meta.is_file() && meta.len() > 8 {
                    result.insert(gid, candidate.to_string_lossy().to_string());
                    continue;
                }
            }
            missing_ids.push(gid);
        }

        if missing_ids.is_empty() {
            return Ok(result);
        }

        let library = crate::load_library_lightweight(&app_clone)?;
        let by_id: HashMap<String, SidebarIconSource> = library
            .iter()
            .map(|entry| (entry.id.clone(), SidebarIconSource::from(entry)))
            .collect();

        let pending: Vec<SidebarIconSource> = missing_ids
            .into_iter()
            .filter_map(|gid| by_id.get(&gid).cloned())
            .collect();

        if pending.is_empty() {
            return Ok(result);
        }

        let workers = pending.len().min(4);
        if workers <= 1 {
            for src in &pending {
                if let Some(path) =
                    resolve_sidebar_icon_in_dirs(src, &sidebar_icons_dir, &covers_dir)
                {
                    result.insert(src.id.clone(), path.to_string_lossy().to_string());
                }
            }
        } else {
            let chunk_size = pending.len().div_ceil(workers);
            let mut chunk_results: Vec<Vec<(String, String)>> = vec![Vec::new(); workers];
            std::thread::scope(|scope| {
                for (chunk, out_slot) in pending.chunks(chunk_size).zip(chunk_results.iter_mut()) {
                    let icons_dir = &sidebar_icons_dir;
                    let cov_dir = &covers_dir;
                    scope.spawn(move || {
                        for src in chunk {
                            if let Some(path) = resolve_sidebar_icon_in_dirs(src, icons_dir, cov_dir)
                            {
                                out_slot.push((
                                    src.id.clone(),
                                    path.to_string_lossy().to_string(),
                                ));
                            }
                        }
                    });
                }
            });
            for pairs in chunk_results {
                for (gid, path_str) in pairs {
                    result.insert(gid, path_str);
                }
            }
        }

        Ok(result)
    })
    .await
    .map_err(|e| format!("Failed resolving sidebar icons: {}", e))?
}

/// Ensures the Portable Mode `sidebar_icons` folder is explicitly permitted on the asset protocol scope.
pub(crate) fn allow_portable_sidebar_icons_scope(app: &AppHandle) {
    if let Ok(dir) = get_sidebar_icons_dir(app) {
        let _ = app.asset_protocol_scope().allow_directory(&dir, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_solid_png(width: u32, height: u32, rgba_pixel: [u8; 4]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            rgba.extend_from_slice(&rgba_pixel);
        }
        encode_rgba_png(&rgba, width, height).unwrap()
    }

    #[test]
    fn test_sidebar_icon_cover_fallback_center_crop_and_cache_hit() {
        let root = std::env::temp_dir().join(format!(
            "duke_sidebar_icon_test_{}_{}",
            std::process::id(),
            ICON_TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let icons_dir = root.join("sidebar_icons");
        let covers_dir = root.join("covers");
        fs::create_dir_all(&icons_dir).unwrap();
        fs::create_dir_all(&covers_dir).unwrap();

        // Create a non-square 96×48 PNG with a 48×48 bright green center block and red wings,
        // verifying that center-cropping removes the outer red wings before 32×32 Lanczos3 resize.
        let (w, h) = (96u32, 48u32);
        let mut raw = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                if (24..72).contains(&x) {
                    raw[idx] = 40;
                    raw[idx + 1] = 200;
                    raw[idx + 2] = 60;
                    raw[idx + 3] = 255;
                } else {
                    raw[idx] = 220;
                    raw[idx + 1] = 20;
                    raw[idx + 2] = 20;
                    raw[idx + 3] = 255;
                }
            }
        }
        let png_bytes = encode_rgba_png(&raw, w, h).unwrap();
        let data_uri = format!("data:image/png;base64,{}", crate::encode_base64(&png_bytes));
        fs::write(covers_dir.join("game_test_cover.cover"), data_uri).unwrap();

        let source = SidebarIconSource {
            id: "game_test_cover".to_string(),
            exe_path: root.join("missing_game.exe").to_string_lossy().to_string(),
            cover_url: "cover-cache://game_test_cover".to_string(),
            icon_path: None,
            steam_app_id: Some(730),
            app_id: Some(730),
        };

        let cached_path = resolve_sidebar_icon_in_dirs(&source, &icons_dir, &covers_dir)
            .expect("Expected cover fallback to generate 32x32 PNG");
        assert!(cached_path.ends_with("game_test_cover.png"));

        let cached_bytes = fs::read(&cached_path).unwrap();
        let (decoded, dw, dh) = decode_png_to_rgba(&cached_bytes).unwrap();
        assert_eq!((dw, dh), (32, 32));
        // Center pixels must be pure green (red wings cropped away)
        let center_idx = ((16 * 32 + 16) * 4) as usize;
        assert!(decoded[center_idx + 1] > 180);
        assert!(decoded[center_idx] < 60);

        // Priority 1: verify cached file is returned verbatim without regeneration
        let sentinel_png = build_solid_png(32, 32, [12, 34, 56, 255]);
        fs::write(&cached_path, &sentinel_png).unwrap();
        let second = resolve_sidebar_icon_in_dirs(&source, &icons_dir, &covers_dir).unwrap();
        assert_eq!(second, cached_path);
        let after_bytes = fs::read(&second).unwrap();
        assert_eq!(after_bytes, sentinel_png);

        // AppID fallback inside covers/ when game_id has no direct .cover
        let appid_png = build_solid_png(64, 64, [20, 110, 230, 255]);
        fs::write(covers_dir.join("1245620.png"), &appid_png).unwrap();
        let appid_source = SidebarIconSource {
            id: "game_appid_only".to_string(),
            exe_path: root.join("also_missing.exe").to_string_lossy().to_string(),
            cover_url: String::new(),
            icon_path: None,
            steam_app_id: Some(1245620),
            app_id: Some(1245620),
        };
        let appid_cached =
            resolve_sidebar_icon_in_dirs(&appid_source, &icons_dir, &covers_dir).unwrap();
        let (_, aw, ah) = decode_png_to_rgba(&fs::read(&appid_cached).unwrap()).unwrap();
        assert_eq!((aw, ah), (32, 32));

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_win32_exe_icon_extraction_to_32x32_png() {
        let sys_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        let candidate_exe = PathBuf::from(&sys_root).join("notepad.exe");
        if !candidate_exe.is_file() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "duke_sidebar_exe_icon_test_{}_{}",
            std::process::id(),
            ICON_TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let icons_dir = root.join("sidebar_icons");
        let covers_dir = root.join("covers");
        fs::create_dir_all(&icons_dir).unwrap();
        fs::create_dir_all(&covers_dir).unwrap();

        let source = SidebarIconSource {
            id: "game_notepad_test".to_string(),
            exe_path: candidate_exe.to_string_lossy().to_string(),
            ..Default::default()
        };
        let out_path = resolve_sidebar_icon_in_dirs(&source, &icons_dir, &covers_dir)
            .expect("Expected Win32 icon extraction for notepad.exe");
        let (rgba, w, h) = decode_png_to_rgba(&fs::read(&out_path).unwrap()).unwrap();
        assert_eq!((w, h), (32, 32));
        assert!(rgba.chunks_exact(4).any(|px| px[3] > 0));

        let _ = fs::remove_dir_all(&root);
    }
}
