use crate::duke_ai_rt::{acquire_worker_lease, query_foundry_local_chat, unload_idle_workers_internal};
use crate::DUKE_BUS;
use duke_core::pipe_ipc::DukeEvent;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

static LATEST_VISION: OnceLock<Mutex<Option<VisionAnalysisResult>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionAnalysisResult {
    pub id: String,
    pub game_title: String,
    pub source_lang: String,
    pub target_lang: String,
    pub mode: String,
    pub extracted_text: String,
    pub translated_text: String,
    pub quest_hint: String,
    pub ocr_engine: String,
    pub ai_backend: String,
    pub execution_provider: String,
    pub vram_allocated_mb: f32,
    pub capture_ms: f32,
    pub ocr_ms: f32,
    pub total_ms: f32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub created_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn try_read_duke_capture_fresh_frame() -> Option<(u32, u32, Vec<u8>)> {
    let local_app = std::env::var("LOCALAPPDATA").ok()?;
    let snap_path = PathBuf::from(local_app)
        .join("com.dukecapture.desktop")
        .join("ai_latest_frame.dkf");
    let meta = fs::metadata(&snap_path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age.as_millis() > 1200 || meta.len() < 16 || meta.len() > 40 * 1024 * 1024 {
        return None;
    }
    let bytes = fs::read(&snap_path).ok()?;
    if bytes.len() < 16 || &bytes[0..8] != b"DUKEFRM1" {
        return None;
    }
    let width = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let height = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let expected = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    if expected == 0 || bytes.len() < 16 + expected {
        return None;
    }
    Some((width, height, bytes[16..16 + expected].to_vec()))
}

#[cfg(target_os = "windows")]
fn capture_foreground_window_bgra() -> Result<(u32, u32, Vec<u8>, &'static str), String> {
    if let Some((w, h, bgra)) = try_read_duke_capture_fresh_frame() {
        return Ok((w, h, bgra, "duke-capture DXGI Bridge"));
    }

    use std::ffi::c_void;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDesktopWindow, GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN,
        SM_CYSCREEN,
    };

    unsafe {
        let fg = GetForegroundWindow();
        let target_hwnd = if !fg.0.is_null() { fg } else { GetDesktopWindow() };
        let mut rect = RECT::default();
        let screen_w = GetSystemMetrics(SM_CXSCREEN).max(640);
        let screen_h = GetSystemMetrics(SM_CYSCREEN).max(480);

        let (x, y, w, h) = if GetWindowRect(target_hwnd, &mut rect).is_ok()
            && rect.right > rect.left + 64
            && rect.bottom > rect.top + 64
        {
            let cx = rect.left.clamp(0, screen_w - 64);
            let cy = rect.top.clamp(0, screen_h - 64);
            let cw = (rect.right - cx).clamp(64, screen_w - cx);
            let ch = (rect.bottom - cy).clamp(64, screen_h - cy);
            (cx, cy, cw, ch)
        } else {
            (0, 0, screen_w, screen_h)
        };

        let hdc_screen = GetDC(None);
        if hdc_screen.0.is_null() {
            return Err("Failed acquiring screen DC for Duke Vision.".to_string());
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
        let old = SelectObject(hdc_mem, hbm.into());
        let _ = BitBlt(hdc_mem, 0, 0, w, h, Some(hdc_screen), x, y, SRCCOPY);

        let mut bi = BITMAPINFO::default();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h; // top-down BGRA
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB.0;

        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let _ = GetDIBits(
            hdc_mem,
            hbm,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut bi,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbm.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);

        Ok((w as u32, h as u32, buf, "Win32 Foreground Capture"))
    }
}

#[cfg(not(target_os = "windows"))]
fn capture_foreground_window_bgra() -> Result<(u32, u32, Vec<u8>, &'static str), String> {
    Ok((640, 360, vec![0u8; 640 * 360 * 4], "Headless Mock Frame"))
}

#[cfg(target_os = "windows")]
pub fn run_win11_ocr_bgra(
    width: u32,
    height: u32,
    bgra: &[u8],
    source_lang: &str,
) -> Result<(String, &'static str), String> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    if width == 0 || height == 0 || bgra.len() < (width as usize * height as usize * 4) {
        return Err("Invalid BGRA frame buffer passed to OCR.".to_string());
    }

    let com_res = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    let res = (|| -> Result<(String, &'static str), String> {
        let writer = DataWriter::new().map_err(|e| format!("DataWriter init error: {}", e))?;
        writer
            .WriteBytes(bgra)
            .map_err(|e| format!("DataWriter write error: {}", e))?;
        let ibuf = writer
            .DetachBuffer()
            .map_err(|e| format!("DataWriter detach error: {}", e))?;

        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &ibuf,
            BitmapPixelFormat::Bgra8,
            width as i32,
            height as i32,
        )
        .map_err(|e| format!("SoftwareBitmap creation error: {}", e))?;

        let lang_trim = source_lang.trim();
        let engine = if !lang_trim.is_empty() && !lang_trim.eq_ignore_ascii_case("auto") {
            let tag = HSTRING::from(lang_trim);
            if let Ok(lang) = Language::CreateLanguage(&tag) {
                OcrEngine::TryCreateFromLanguage(&lang)
                    .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())
            } else {
                OcrEngine::TryCreateFromUserProfileLanguages()
            }
        } else {
            OcrEngine::TryCreateFromUserProfileLanguages()
        }
        .map_err(|e| format!("Windows.Media.Ocr engine unavailable: {}", e))?;

        let ocr_op = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| format!("OCR RecognizeAsync failed: {}", e))?;
        let ocr_res = ocr_op
            .join()
            .map_err(|e| format!("OCR RecognizeAsync completion failed: {}", e))?;

        let mut lines_out = Vec::new();
        if let Ok(lines) = ocr_res.Lines() {
            let count = lines.Size().unwrap_or(0);
            for i in 0..count {
                if let Ok(line) = lines.GetAt(i) {
                    if let Ok(txt) = line.Text() {
                        let s = txt.to_string_lossy().trim().to_string();
                        if !s.is_empty() {
                            lines_out.push(s);
                        }
                    }
                }
            }
        } else if let Ok(all_txt) = ocr_res.Text() {
            let s = all_txt.to_string_lossy().trim().to_string();
            if !s.is_empty() {
                lines_out.push(s);
            }
        }

        Ok((lines_out.join("\n"), "Windows 11 OcrEngine (WinRT)"))
    })();

    if com_res.is_ok() {
        unsafe {
            CoUninitialize();
        }
    }

    res
}

#[cfg(not(target_os = "windows"))]
pub fn run_win11_ocr_bgra(
    _width: u32,
    _height: u32,
    _bgra: &[u8],
    _source_lang: &str,
) -> Result<(String, &'static str), String> {
    Ok((String::new(), "Non-Windows Stub OCR"))
}

pub fn detect_text_language_hint(text: &str) -> &'static str {
    for ch in text.chars() {
        if ('\u{3040}'..='\u{30ff}').contains(&ch) {
            return "ja";
        }
        if ('\u{ac00}'..='\u{d7af}').contains(&ch) {
            return "ko";
        }
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            return "zh";
        }
        if ('\u{0400}'..='\u{04ff}').contains(&ch) {
            return "ru";
        }
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("misión") || lower.contains("objetivo") || lower.contains("jefe") {
        return "es";
    }
    if lower.contains("quête") || lower.contains("sauvegarder") {
        return "fr";
    }
    if lower.contains("aufgabe") || lower.contains("speichern") {
        return "de";
    }
    "en"
}

fn translate_game_glossary_local(text: &str) -> Option<String> {
    let glossary: &[(&str, &str)] = &[
        ("セーブデータ", "Save Data"),
        ("セーブ", "Save"),
        ("クエスト", "Quest"),
        ("目標", "Objective"),
        ("弱点", "Weakness"),
        ("ボス", "Boss"),
        ("体力", "HP / Health"),
        ("回復", "Restore / Heal"),
        ("開始", "Start"),
        ("続ける", "Continue"),
        ("はい", "Yes"),
        ("いいえ", "No"),
        ("任務", "Mission"),
        ("任务", "Mission / Quest"),
        ("保存", "Save"),
        ("继续", "Continue"),
        ("목표", "Objective"),
        ("저장", "Save"),
        ("Misión", "Mission"),
        ("Quête", "Quest"),
        ("Aufgabe", "Quest"),
        ("Задание", "Mission / Quest"),
    ];

    let mut out = text.to_string();
    let mut hits = 0usize;
    for &(src, dst) in glossary {
        if out.contains(src) {
            out = out.replace(src, &format!("{} [{}]", dst, src));
            hits += 1;
        }
    }
    if hits > 0 {
        Some(out)
    } else {
        None
    }
}

pub fn synthesize_quest_hint_local(text: &str, game_title: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut clues = Vec::new();

    if lower.contains("save") || text.contains("セーブ") || text.contains("保存") || text.contains("저장") {
        clues.push("Checkpoint / save prompt detected — trigger Quick Save or keep progress before advancing.");
    }
    if lower.contains("weak") || lower.contains("fire") || lower.contains("lightning") || text.contains("弱点") {
        clues.push("Combat vulnerability indicator detected — swap elemental loadout to match highlighted weakness.");
    }
    if lower.contains("key") || lower.contains("door") || lower.contains("gate") || lower.contains("lock") {
        clues.push("Progression lock cue — check nearby side room, NPC dialogue, or inventory key tab.");
    }
    if lower.contains("quest") || lower.contains("objective") || text.contains("クエスト") || text.contains("目標") {
        clues.push("Active mission marker captured — pinned in overlay until next screen scan.");
    }

    if clues.is_empty() {
        let preview: String = text.split_whitespace().take(18).collect::<Vec<_>>().join(" ");
        if preview.is_empty() {
            format!("No legible UI/dialogue text found in {} frame — aim camera at text box and trigger Ctrl+Shift+T or Guide+Y.", game_title)
        } else {
            format!(
                "[{}] Captured screen context: \"{}\". Tip: Start Foundry Local on NPU for full conversational lore & boss guides.",
                game_title, preview
            )
        }
    } else {
        clues.join(" ")
    }
}

pub struct VisionAnalysisInput<'a> {
    pub extracted_text: &'a str,
    pub game_title: &'a str,
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub mode: &'a str,
    pub ocr_engine: &'a str,
    pub capture_ms: f32,
    pub ocr_ms: f32,
    pub frame_width: u32,
    pub frame_height: u32,
}

pub fn analyze_text_pipeline(input: VisionAnalysisInput<'_>) -> VisionAnalysisResult {
    let VisionAnalysisInput {
        extracted_text,
        game_title,
        source_lang,
        target_lang,
        mode,
        ocr_engine,
        capture_ms,
        ocr_ms,
        frame_width,
        frame_height,
    } = input;
    let total_t0 = Instant::now();
    let lease = acquire_worker_lease("vision", "translation-nllb-npu.onnx");
    let detected_src = if source_lang.trim().is_empty() || source_lang.eq_ignore_ascii_case("auto") {
        detect_text_language_hint(extracted_text).to_string()
    } else {
        source_lang.trim().to_string()
    };
    let clean_target = if target_lang.trim().is_empty() {
        "en".to_string()
    } else {
        target_lang.trim().to_string()
    };

    let clean_text = extracted_text.trim();
    let (translated_text, quest_hint, ai_backend) = if clean_text.is_empty() {
        (
            String::new(),
            synthesize_quest_hint_local("", game_title),
            lease.model_name.clone(),
        )
    } else {
        let sys_prompt = format!(
            "You are Duke Vision, a zero-VRAM on-device game copilot for '{}'. Reply strictly in JSON with keys `translation` (into '{}') and `questHint` (1 concise tactical sentence).",
            game_title, clean_target
        );
        let user_prompt = format!("Mode: {}\nSource ({}):\n{}", mode, detected_src, clean_text);
        if let Some((raw_reply, backend)) = query_foundry_local_chat(&sys_prompt, &user_prompt) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw_reply) {
                let tr = parsed
                    .get("translation")
                    .and_then(|v| v.as_str())
                    .unwrap_or(clean_text)
                    .to_string();
                let hint = parsed
                    .get("questHint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (tr, hint, backend)
            } else {
                (
                    raw_reply.clone(),
                    synthesize_quest_hint_local(clean_text, game_title),
                    backend,
                )
            }
        } else {
            let local_tr = translate_game_glossary_local(clean_text).unwrap_or_else(|| {
                if detected_src == clean_target {
                    clean_text.to_string()
                } else {
                    format!("[{} -> {}] {}", detected_src, clean_target, clean_text)
                }
            });
            let hint = synthesize_quest_hint_local(clean_text, game_title);
            (local_tr, hint, format!("On-Device Vision ({})", lease.ep_label()))
        }
    };

    let total_ms = capture_ms + ocr_ms + total_t0.elapsed().as_secs_f32() * 1000.0;
    VisionAnalysisResult {
        id: format!("vis_{}", now_ms()),
        game_title: game_title.to_string(),
        source_lang: detected_src,
        target_lang: clean_target,
        mode: mode.to_string(),
        extracted_text: clean_text.to_string(),
        translated_text,
        quest_hint,
        ocr_engine: ocr_engine.to_string(),
        ai_backend,
        execution_provider: lease.ep_label().to_string(),
        vram_allocated_mb: lease.ep.vram_allocated_mb(),
        capture_ms,
        ocr_ms,
        total_ms,
        frame_width,
        frame_height,
        created_at_ms: now_ms(),
    }
}

fn store_and_broadcast_vision(app: &AppHandle, res: VisionAnalysisResult) -> VisionAnalysisResult {
    if let Ok(mut guard) = LATEST_VISION.get_or_init(|| Mutex::new(None)).lock() {
        *guard = Some(res.clone());
    }
    if let Some(bus) = DUKE_BUS.get() {
        bus.publish(DukeEvent::SubtitleBroadcast {
            channel: "vision".to_string(),
            source_lang: res.source_lang.clone(),
            target_lang: res.target_lang.clone(),
            original_text: res.extracted_text.clone(),
            translated_text: res.translated_text.clone(),
            quest_hint: (!res.quest_hint.is_empty()).then(|| res.quest_hint.clone()),
            confidence: 0.94,
            backend: res.execution_provider.clone(),
        });
    }
    let _ = app.emit("duke-vision-result", &res);
    let _ = unload_idle_workers_internal(false);
    res
}

#[tauri::command]
pub async fn trigger_duke_vision_capture(
    app: AppHandle,
    game_title: Option<String>,
    source_lang: Option<String>,
    target_lang: Option<String>,
    mode: Option<String>,
) -> Result<VisionAnalysisResult, String> {
    let title = game_title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Active Game".to_string());
    let src_l = source_lang.unwrap_or_else(|| "auto".to_string());
    let dst_l = target_lang.unwrap_or_else(|| "en".to_string());
    let mode_s = mode.unwrap_or_else(|| "both".to_string());

    let res = tokio::task::spawn_blocking(move || -> Result<VisionAnalysisResult, String> {
        let cap_t0 = Instant::now();
        let (w, h, bgra, cap_src) = capture_foreground_window_bgra()?;
        let capture_ms = cap_t0.elapsed().as_secs_f32() * 1000.0;

        let ocr_t0 = Instant::now();
        let (extracted, ocr_engine) = match run_win11_ocr_bgra(w, h, &bgra, &src_l) {
            Ok((text, engine)) => (text, format!("{} via {}", engine, cap_src)),
            Err(err) => (String::new(), format!("OCR unavailable ({}) [{}]", err, cap_src)),
        };
        let ocr_ms = ocr_t0.elapsed().as_secs_f32() * 1000.0;

        Ok(analyze_text_pipeline(VisionAnalysisInput {
            extracted_text: &extracted,
            game_title: &title,
            source_lang: &src_l,
            target_lang: &dst_l,
            mode: &mode_s,
            ocr_engine: &ocr_engine,
            capture_ms,
            ocr_ms,
            frame_width: w,
            frame_height: h,
        }))
    })
    .await
    .map_err(|e| format!("Duke Vision worker panicked: {}", e))??;

    Ok(store_and_broadcast_vision(&app, res))
}

#[tauri::command]
pub async fn analyze_vision_text_query(
    app: AppHandle,
    text: String,
    game_title: Option<String>,
    source_lang: Option<String>,
    target_lang: Option<String>,
    mode: Option<String>,
) -> Result<VisionAnalysisResult, String> {
    let title = game_title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Active Game".to_string());
    let src_l = source_lang.unwrap_or_else(|| "auto".to_string());
    let dst_l = target_lang.unwrap_or_else(|| "en".to_string());
    let mode_s = mode.unwrap_or_else(|| "both".to_string());

    let res = tokio::task::spawn_blocking(move || {
        analyze_text_pipeline(VisionAnalysisInput {
            extracted_text: &text,
            game_title: &title,
            source_lang: &src_l,
            target_lang: &dst_l,
            mode: &mode_s,
            ocr_engine: "Direct Overlay Query",
            capture_ms: 0.0,
            ocr_ms: 0.2,
            frame_width: 0,
            frame_height: 0,
        })
    })
    .await
    .map_err(|e| format!("Vision query task panicked: {}", e))?;

    Ok(store_and_broadcast_vision(&app, res))
}

#[tauri::command]
pub fn get_latest_vision_state() -> Option<VisionAnalysisResult> {
    LATEST_VISION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_pipeline_detects_japanese_glossary_and_quest_hint() {
        let out = analyze_text_pipeline(VisionAnalysisInput {
            extracted_text: "セーブデータが見つかりました。クエスト目標: 火の弱点を突け",
            game_title: "Elden Ring",
            source_lang: "auto",
            target_lang: "en",
            mode: "both",
            ocr_engine: "UnitTest OCR",
            capture_ms: 2.5,
            ocr_ms: 8.0,
            frame_width: 1920,
            frame_height: 1080,
        });
        assert_eq!(out.source_lang, "ja");
        assert!(out.translated_text.contains("Save Data"));
        assert!(out.translated_text.contains("Weakness"));
        assert!(out.quest_hint.contains("Checkpoint") || out.quest_hint.contains("vulnerability"));
        assert_eq!(out.ocr_ms, 8.0);
    }
}
