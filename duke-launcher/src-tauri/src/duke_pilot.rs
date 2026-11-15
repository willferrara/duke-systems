//! Duke Pilot: Deterministic Natural-Language Autonomous Launcher Shell (`Ctrl+K`)
//! + optional local Windows ML / Foundry Local intent refinement.
//!
//! Converts plain-English multi-step commands into typed execution plans with
//! live UI previews (e.g. "Shrink my biggest games over 50 GB", "Set up Cemu and
//! import my Wii U folder", "Freeze ARC Raiders and launch Balatro", "Back up
//! Elden Ring saves and apply my modpack", "Export portable capsule for Project Zomboid").

use crate::catalog_mmap::{parse_nl_semantic_query, query_catalog_mmap, CatalogQueryParams};
use crate::CatalogGameEntry;
use crate::dkpack;
use crate::duke_ai_rt;
use crate::duke_capsule;
use crate::emulators;
use crate::ludusavi;
use crate::mod_graph;
use crate::GameEntry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PilotActionKind {
    ShrinkGames,
    SetupEmulator,
    ImportFolder,
    SuspendGame,
    ResumeGame,
    LaunchGame,
    BackupSaves,
    ApplyModpack,
    ExportCapsule,
    ExportDkpack,
    ToggleCouchMode,
    PlayTonight,
    ScanMalware,
    BlockInternet,
    FindFixes,
    VerifyBios,
    DiskPressureAutopilot,
    SemanticCatalogSearch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotStep {
    pub id: String,
    pub kind: PilotActionKind,
    pub label: String,
    pub detail: String,
    pub action_icon: String,
    pub target_game_ids: Vec<String>,
    pub target_titles: Vec<String>,
    pub console_id: Option<String>,
    pub path_hint: Option<String>,
    pub min_size_gb: Option<f64>,
    pub session_minutes: Option<u32>,
    pub capsule_format: Option<String>,
    pub query_text: Option<String>,
    pub requires_folder_prompt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotPlan {
    pub raw_query: String,
    pub headline: String,
    pub mode: String,
    pub confidence: f32,
    pub steps: Vec<PilotStep>,
    pub requires_folder_prompt: bool,
    pub preview_summary: String,
    pub semantic_matches: Vec<CatalogGameEntry>,
    pub ai_refined: bool,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotStepExecutionResult {
    pub step_id: String,
    pub kind: PilotActionKind,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PilotExecutionReport {
    pub ok: bool,
    pub steps_succeeded: usize,
    pub steps_failed: usize,
    pub results: Vec<PilotStepExecutionResult>,
    pub summary: String,
    pub needs_folder_prompt: bool,
    pub follow_up_action: Option<String>,
    pub follow_up_query: Option<String>,
}

const ACTION_VERB_PREFIXES: &[&str] = &[
    "shrink ",
    "compress ",
    "compact ",
    "slim ",
    "set up ",
    "setup ",
    "install ",
    "configure ",
    "import ",
    "scan ",
    "add ",
    "freeze ",
    "suspend ",
    "pause ",
    "resume ",
    "unfreeze ",
    "launch ",
    "play ",
    "start ",
    "open ",
    "back up ",
    "backup ",
    "save ",
    "apply ",
    "load ",
    "enable ",
    "activate ",
    "export ",
    "package ",
    "pack ",
    "create ",
    "build ",
    "compile ",
    "enter ",
    "toggle ",
    "switch ",
    "find ",
    "search ",
];

fn starts_with_action_verb(fragment: &str) -> bool {
    let lower = fragment.trim_start().to_ascii_lowercase();
    ACTION_VERB_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Splits multi-step NL queries into independent clauses without breaking game titles
/// like "Mount and Blade" when the right-hand side is not an action clause.
pub fn split_pilot_clauses(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut clauses = Vec::new();
    let mut start = 0usize;
    let bytes = trimmed.as_bytes();
    let mut in_quotes = false;
    let lower = trimmed.to_ascii_lowercase();
    let delimiters = [" and then ", " then ", " and ", " & ", "; "];

    let mut i = 0usize;
    while i < trimmed.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if !in_quotes && lower.is_char_boundary(i) {
            let mut matched_delim: Option<usize> = None;
            for delim in delimiters {
                if lower[i..].starts_with(delim) {
                    let rhs = &trimmed[i + delim.len()..];
                    if delim == "; " || delim == " then " || delim == " and then " || starts_with_action_verb(rhs) {
                        matched_delim = Some(delim.len());
                        break;
                    }
                }
            }
            if let Some(dlen) = matched_delim {
                let piece = trimmed[start..i].trim();
                if !piece.is_empty() {
                    clauses.push(piece.to_string());
                }
                i += dlen;
                start = i;
                continue;
            }
        }
        if let Some(ch) = trimmed[i..].chars().next() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    let tail = trimmed[start..].trim();
    if !tail.is_empty() {
        clauses.push(tail.to_string());
    }
    clauses
}

fn normalize_text_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn find_best_library_game<'a>(clause: &str, library: &'a [GameEntry]) -> Option<&'a GameEntry> {
    if library.is_empty() {
        return None;
    }
    let clause_lower = clause.to_ascii_lowercase();
    let clause_tokens = normalize_text_tokens(clause);

    let mut best: Option<(&GameEntry, i32)> = None;
    for entry in library {
        let title_lower = entry.title.trim().to_ascii_lowercase();
        if title_lower.is_empty() {
            continue;
        }
        let mut score = 0i32;
        if clause_lower.contains(&title_lower) {
            score += 500 + (title_lower.len() as i32 * 4);
        } else {
            let title_tokens = normalize_text_tokens(&title_lower);
            if !title_tokens.is_empty() {
                let mut matched = 0i32;
                for tt in &title_tokens {
                    if tt.len() >= 2 && clause_tokens.iter().any(|ct| ct == tt) {
                        matched += 1;
                    }
                }
                if matched > 0 {
                    let ratio = (matched * 100) / (title_tokens.len() as i32);
                    if matched == title_tokens.len() as i32 {
                        score += 360 + matched * 30;
                    } else if ratio >= 60 {
                        score += 180 + ratio + matched * 20;
                    } else if matched >= 1 && title_tokens.len() == 1 && title_tokens[0].len() >= 4 {
                        score += 220;
                    }
                }
            }
        }
        if score > 0 {
            if entry.is_installed {
                score += 15;
            }
            if match best {
                None => true,
                Some((_, bscore)) => score > bscore,
            } {
                best = Some((entry, score));
            }
        }
    }
    best.map(|(e, _)| e)
}

fn detect_console_id(clause_lower: &str) -> Option<(&'static str, &'static str)> {
    let map = [
        ("cemu", "cemu", "Cemu (Wii U)"),
        ("wii u", "cemu", "Cemu (Wii U)"),
        ("wiiu", "cemu", "Cemu (Wii U)"),
        ("ryujinx", "ryujinx", "Ryujinx (Switch)"),
        ("switch", "ryujinx", "Ryujinx (Switch)"),
        ("dolphin", "dolphin", "Dolphin (GameCube / Wii)"),
        ("gamecube", "dolphin", "Dolphin (GameCube / Wii)"),
        ("pcsx2", "pcsx2", "PCSX2 (PlayStation 2)"),
        ("ps2", "pcsx2", "PCSX2 (PlayStation 2)"),
        ("rpcs3", "rpcs3", "RPCS3 (PlayStation 3)"),
        ("ps3", "rpcs3", "RPCS3 (PlayStation 3)"),
        ("duckstation", "duckstation", "DuckStation (PS1)"),
        ("azahar", "azahar", "Azahar (3DS)"),
        ("3ds", "azahar", "Azahar (3DS)"),
        ("melonds", "melonds", "melonDS (DS)"),
        ("mgba", "mgba", "mGBA (GBA)"),
        ("ppsspp", "ppsspp", "PPSSPP (PSP)"),
        ("xenia", "xenia", "Xenia (Xbox 360)"),
        ("retroarch", "retroarch", "RetroArch"),
    ];
    for (needle, id, label) in map {
        if clause_lower.contains(needle) {
            return Some((id, label));
        }
    }
    None
}

fn extract_windows_path_from_clause(clause: &str) -> Option<String> {
    if let Some(q1) = clause.find('"') {
        if let Some(q2_rel) = clause[q1 + 1..].find('"') {
            let inside = clause[q1 + 1..q1 + 1 + q2_rel].trim();
            if inside.contains(":\\") || inside.starts_with("\\\\") || inside.contains('/') {
                return Some(inside.to_string());
            }
        }
    }
    for token in clause.split_whitespace() {
        let clean = token.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';'));
        let bytes = clean.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            return Some(clean.to_string());
        }
    }
    None
}

fn extract_threshold_gb(clause_lower: &str) -> Option<f64> {
    let tokens: Vec<&str> = clause_lower.split_whitespace().collect();
    for (idx, tok) in tokens.iter().enumerate() {
        let cleaned = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.');
        if cleaned.ends_with("gb") && cleaned.len() > 2 {
            if let Ok(v) = cleaned.trim_end_matches("gb").parse::<f64>() {
                return Some(v);
            }
        }
        if let Ok(v) = cleaned.parse::<f64>() {
            if tokens.get(idx + 1).is_some_and(|n| n.starts_with("gb")) {
                return Some(v);
            }
        }
    }
    None
}

fn extract_minutes(clause_lower: &str) -> Option<u32> {
    let tokens: Vec<&str> = clause_lower.split_whitespace().collect();
    for (idx, tok) in tokens.iter().enumerate() {
        let cleaned = tok.trim_matches(|c: char| !c.is_ascii_digit());
        if let Ok(v) = cleaned.parse::<u32>() {
            if tokens
                .get(idx + 1)
                .is_some_and(|n| n.starts_with("min") || n.starts_with("hr") || n.starts_with("hour"))
            {
                let next = tokens[idx + 1];
                return Some(if next.starts_with("hr") || next.starts_with("hour") {
                    v.saturating_mul(60)
                } else {
                    v
                });
            }
        }
    }
    None
}

/// Deterministic fast parser turning plain-English commands into actionable `PilotPlan` steps.
pub fn parse_pilot_command(raw_query: &str, library: &[GameEntry]) -> PilotPlan {
    let started = Instant::now();
    let trimmed = raw_query.trim();
    if trimmed.is_empty() {
        return PilotPlan {
            raw_query: String::new(),
            headline: "Type a command or search query".to_string(),
            mode: "search".to_string(),
            confidence: 0.0,
            steps: Vec::new(),
            requires_folder_prompt: false,
            preview_summary: "Examples: \"Shrink my biggest games over 50 GB\", \"Freeze ARC Raiders and launch Balatro\"".to_string(),
            semantic_matches: Vec::new(),
            ai_refined: false,
            elapsed_us: started.elapsed().as_micros() as u64,
        };
    }

    let clauses = split_pilot_clauses(trimmed);
    let mut steps: Vec<PilotStep> = Vec::new();
    let mut active_game_ctx: Option<GameEntry> = None;
    let mut requires_folder_prompt = false;

    for (idx, clause) in clauses.iter().enumerate() {
        let step_id = format!("step-{}", idx + 1);
        let lower = clause.to_ascii_lowercase();
        let matched_game = find_best_library_game(clause, library).cloned();
        if let Some(ref g) = matched_game {
            active_game_ctx = Some(g.clone());
        }
        let target_game = matched_game.or_else(|| {
            if lower.contains("my modpack")
                || lower.contains("its saves")
                || lower.contains("this game")
                || lower.contains("the game")
                || lower.contains("for it")
            {
                active_game_ctx.clone()
            } else {
                None
            }
        });

        // 1. Shrink / Compress games
        if lower.starts_with("shrink ")
            || lower.starts_with("compress ")
            || lower.starts_with("compact ")
            || lower.contains("shrink my ")
            || lower.contains("compress games")
        {
            let threshold_gb = extract_threshold_gb(&lower);
            let mut candidates: Vec<&GameEntry> = if let Some(ref g) = target_game {
                vec![g]
            } else {
                library
                    .iter()
                    .filter(|g| {
                        g.is_installed
                            && match threshold_gb {
                                None => true,
                                Some(gb) => {
                                    (g.size_bytes as f64) >= gb * 1024.0 * 1024.0 * 1024.0
                                }
                            }
                    })
                    .collect()
            };
            candidates.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
            if threshold_gb.is_none() && target_game.is_none() {
                candidates.truncate(5);
            }
            let ids: Vec<String> = candidates.iter().map(|g| g.id.clone()).collect();
            let titles: Vec<String> = candidates.iter().map(|g| g.title.clone()).collect();
            let total_gb: f64 = candidates
                .iter()
                .map(|g| (g.size_bytes as f64) / (1024.0 * 1024.0 * 1024.0))
                .sum();
            let detail = if titles.is_empty() {
                match threshold_gb {
                    Some(gb) => format!("No installed library titles currently exceed {:.0} GB.", gb),
                    None => "No installed library titles matched.".to_string(),
                }
            } else {
                format!(
                    "{} title(s) totaling {:.1} GB: {}",
                    titles.len(),
                    total_gb,
                    titles.join(", ")
                )
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ShrinkGames,
                label: match threshold_gb {
                    Some(gb) => format!("CompactOS Shrink (> {:.0} GB)", gb),
                    None => "CompactOS Shrink Largest Games".to_string(),
                },
                detail,
                action_icon: "icon-shrink".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: threshold_gb,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 2. Setup / Install Emulator
        if (lower.starts_with("set up ")
            || lower.starts_with("setup ")
            || lower.starts_with("install ")
            || lower.starts_with("configure "))
            && detect_console_id(&lower).is_some()
        {
            if let Some((cid, clabel)) = detect_console_id(&lower) {
                steps.push(PilotStep {
                    id: step_id,
                    kind: PilotActionKind::SetupEmulator,
                    label: format!("Set Up {}", clabel),
                    detail: format!("Download and verify portable {} binary into Duke Emulators.", clabel),
                    action_icon: "icon-gamepad".to_string(),
                    target_game_ids: Vec::new(),
                    target_titles: vec![clabel.to_string()],
                    console_id: Some(cid.to_string()),
                    path_hint: None,
                    min_size_gb: None,
                    session_minutes: None,
                    capsule_format: None,
                    query_text: Some(clause.clone()),
                    requires_folder_prompt: false,
                });
                continue;
            }
        }

        // 3. Import folder / ROMs / `.dkpack` (exclude malware/security scans handled below)
        if (lower.starts_with("import ")
            || (lower.starts_with("scan ")
                && !lower.contains("malware")
                && !lower.contains("security")
                && !lower.starts_with("scan for ")))
            || lower.contains(" import ")
        {
            let explicit_path = extract_windows_path_from_clause(clause);
            let prompt_needed = explicit_path.is_none();
            if prompt_needed {
                requires_folder_prompt = true;
            }
            let detail = match &explicit_path {
                Some(p) => format!("Ingest executables, ROMs, or .dkpack recipes from {}", p),
                None => {
                    if let Some((_, clabel)) = detect_console_id(&lower) {
                        format!("Prompt for your {} content folder and auto-import detected titles.", clabel)
                    } else {
                        "Prompt for folder selection and auto-ingest discovered titles.".to_string()
                    }
                }
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ImportFolder,
                label: "Import Game / ROM Folder".to_string(),
                detail,
                action_icon: "icon-folder".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: detect_console_id(&lower).map(|(c, _)| c.to_string()),
                path_hint: explicit_path,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: prompt_needed,
            });
            continue;
        }

        // 4. Freeze / Suspend game
        if lower.starts_with("freeze ") || lower.starts_with("suspend ") || lower.starts_with("pause ") {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Call NtSuspendProcess & trim working-set RAM for {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    format!("Target game from \"{}\" not found in local library.", clause),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::SuspendGame,
                label: match target_game {
                    Some(ref g) => format!("Freeze {}", g.title),
                    None => "Freeze Running Game".to_string(),
                },
                detail,
                action_icon: "icon-pause".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 5. Resume / Unfreeze game
        if lower.starts_with("resume ") || lower.starts_with("unfreeze ") {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Resume suspended threads for {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    format!("Target game from \"{}\" not found in local library.", clause),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ResumeGame,
                label: match target_game {
                    Some(ref g) => format!("Resume {}", g.title),
                    None => "Resume Game".to_string(),
                },
                detail,
                action_icon: "icon-play".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 6. Export Portable Capsule
        if (lower.contains("capsule") || lower.contains("usb") || lower.contains("dkcap"))
            && (lower.contains("export") || lower.contains("create") || lower.contains("build") || lower.contains("package") || lower.contains("make"))
        {
            let fmt = if lower.contains("dkcap") || lower.contains("archive") || lower.contains("compressed") {
                "dkcap"
            } else {
                "folder"
            };
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!(
                        "Bundle {} + PE runtime DLLs + proxy chains + sandbox saves into a portable {} capsule.",
                        g.title, fmt
                    ),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "Select an installed title to bundle into a Duke Capsule.".to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ExportCapsule,
                label: match target_game {
                    Some(ref g) => format!("Export Portable Capsule ({})", g.title),
                    None => "Export Portable Capsule".to_string(),
                },
                detail,
                action_icon: "icon-archive".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: Some(fmt.to_string()),
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 7. Export .dkpack recipe
        if lower.contains("dkpack") && (lower.contains("export") || lower.contains("compile") || lower.contains("create")) {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Compile <50 KB deterministic BLAKE3/proxy-chain .dkpack recipe for {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "Select a library game to compile a deterministic .dkpack recipe.".to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ExportDkpack,
                label: match target_game {
                    Some(ref g) => format!("Export .dkpack Recipe ({})", g.title),
                    None => "Export .dkpack Recipe".to_string(),
                },
                detail,
                action_icon: "icon-archive".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 8. Backup saves
        if lower.starts_with("back up ") || lower.starts_with("backup ") || lower.contains(" saves") && lower.contains("back") {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Snapshot Ludusavi & sandbox saves for {} into rolling ZIP archive.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "No matching game found in library for save backup.".to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::BackupSaves,
                label: match target_game {
                    Some(ref g) => format!("Back Up Saves ({})", g.title),
                    None => "Back Up Game Saves".to_string(),
                },
                detail,
                action_icon: "icon-download".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 9. Apply modpack / loadout / dkpack
        if lower.starts_with("apply ") || lower.contains("modpack") || lower.contains("loadout") {
            let dkpack_path = extract_windows_path_from_clause(clause)
                .filter(|p| p.to_ascii_lowercase().ends_with(".dkpack"));
            let (ids, titles, detail) = if let Some(ref g) = target_game {
                (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    match dkpack_path {
                        Some(ref p) => format!("Verify hashes & apply {} to {}.", p, g.title),
                        None => format!("Deploy active conflict-free mod loadout & proxy DLL chains for {}.", g.title),
                    },
                )
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    "Identify which modded game loadout or .dkpack recipe to apply.".to_string(),
                )
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ApplyModpack,
                label: match target_game {
                    Some(ref g) => format!("Apply Modpack ({})", g.title),
                    None => "Apply Modpack Loadout".to_string(),
                },
                detail,
                action_icon: "icon-settings".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: dkpack_path,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 9b. 1-Click Security, Fix Finder, BIOS Verification, Disk Pressure Autopilot
        if lower.contains("malware")
            || lower.contains("security scan")
            || lower.starts_with("scan for ")
        {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Run Security Shield v2 deep PE static inspection on {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "Run Security Shield v2 deep PE malware & dropper inspection across library."
                        .to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ScanMalware,
                label: match target_game {
                    Some(ref g) => format!("Scan {} for Malware", g.title),
                    None => "Security Shield v2 Malware Scan".to_string(),
                },
                detail,
                action_icon: "icon-shield-check".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        if lower.contains("block internet")
            || lower.contains("offline mode")
            || lower.contains("network lock")
        {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Enable Offline Mode WFP/AppContainer network lock for {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "Identify a library game to lock into Offline Mode.".to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::BlockInternet,
                label: match target_game {
                    Some(ref g) => format!("Block Internet for {}", g.title),
                    None => "Block Internet (Offline Mode)".to_string(),
                },
                detail,
                action_icon: "icon-shield-check".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        if lower.contains("find fix") || lower.starts_with("fix ") || lower.contains("crash fix") {
            let (ids, titles, detail) = match target_game {
                Some(ref g) => (
                    vec![g.id.clone()],
                    vec![g.title.clone()],
                    format!("Diagnose missing DLLs/crashes & fetch PCGamingWiki fixes for {}.", g.title),
                ),
                None => (
                    Vec::new(),
                    Vec::new(),
                    "Specify a game title to run Crash-to-Fix diagnosis + PCGamingWiki lookup."
                        .to_string(),
                ),
            };
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::FindFixes,
                label: match target_game {
                    Some(ref g) => format!("Find Fixes for {}", g.title),
                    None => "Crash-to-Fix & PCGamingWiki Diagnostics".to_string(),
                },
                detail,
                action_icon: "icon-wrench".to_string(),
                target_game_ids: ids,
                target_titles: titles,
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        if lower.contains("verify bios") || lower.contains("check bios") || lower.contains("bios vault") {
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::VerifyBios,
                label: "Verify Console BIOS & Firmware Vault".to_string(),
                detail: "Check Libretro MD5/SHA-1 hashes across DuckStation, PCSX2, melonDS, Azahar & RetroArch."
                    .to_string(),
                action_icon: "icon-shield-check".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        if lower.contains("disk pressure")
            || lower.contains("free space autopilot")
            || lower.contains("space autopilot")
        {
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::DiskPressureAutopilot,
                label: "Run Smart Disk-Space Pressure Autopilot".to_string(),
                detail: "Rank untouched 60+ day games by highest WOF/LZX + language trim space reclaimed."
                    .to_string(),
                action_icon: "icon-hard-drive".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 10. Play Tonight smart picker
        if lower.contains("play tonight") || lower.contains("what should i play") || lower.contains("pick a game") {
            let mins = extract_minutes(&lower).unwrap_or(60);
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::PlayTonight,
                label: format!("Play Tonight ({} min session)", mins),
                detail: "Rank installed titles by HLTB duration fit, active gamepad & live friends.".to_string(),
                action_icon: "icon-gamepad".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: Some(mins),
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 11. Couch Mode toggle
        if lower.contains("couch mode") || lower.contains("duke deck") || lower.contains("10-foot") || lower.contains("big picture") {
            steps.push(PilotStep {
                id: step_id,
                kind: PilotActionKind::ToggleCouchMode,
                label: "Toggle Duke Deck Couch Mode".to_string(),
                detail: "Switch between Desktop and 10-foot controller-first Duke Deck interface.".to_string(),
                action_icon: "icon-gamepad".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(clause.clone()),
                requires_folder_prompt: false,
            });
            continue;
        }

        // 12. Launch / Play specific game
        if lower.starts_with("launch ") || lower.starts_with("play ") || lower.starts_with("start ") || lower.starts_with("open ") {
            if let Some(ref g) = target_game {
                steps.push(PilotStep {
                    id: step_id,
                    kind: PilotActionKind::LaunchGame,
                    label: format!("Launch {}", g.title),
                    detail: format!("Start {} directly through Duke Launcher.", g.title),
                    action_icon: "icon-play".to_string(),
                    target_game_ids: vec![g.id.clone()],
                    target_titles: vec![g.title.clone()],
                    console_id: None,
                    path_hint: None,
                    min_size_gb: None,
                    session_minutes: None,
                    capsule_format: None,
                    query_text: Some(clause.clone()),
                    requires_folder_prompt: false,
                });
                continue;
            }
        }
    }

    // Fallback: DKCAT003 natural-language semantic catalog search OR direct game match.
    if steps.is_empty() {
        let parsed_nl = parse_nl_semantic_query(trimmed);
        let nl_has_constraints = parsed_nl.is_nl_semantic
            || parsed_nl.max_size_mb.is_some()
            || parsed_nl.max_hours_tenths.is_some()
            || parsed_nl.require_controller
            || parsed_nl.require_coop;

        if !nl_has_constraints {
            if let Some(g) = find_best_library_game(trimmed, library) {
                steps.push(PilotStep {
                    id: "step-1".to_string(),
                    kind: PilotActionKind::LaunchGame,
                    label: format!("Launch {}", g.title),
                    detail: format!("Quick-matched installed title {}", g.title),
                    action_icon: "icon-play".to_string(),
                    target_game_ids: vec![g.id.clone()],
                    target_titles: vec![g.title.clone()],
                    console_id: None,
                    path_hint: None,
                    min_size_gb: None,
                    session_minutes: None,
                    capsule_format: None,
                    query_text: Some(trimmed.to_string()),
                    requires_folder_prompt: false,
                });
            }
        }

        if steps.is_empty() {
            steps.push(PilotStep {
                id: "step-1".to_string(),
                kind: PilotActionKind::SemanticCatalogSearch,
                label: "DKCAT003 Quantized Semantic Search".to_string(),
                detail: format!("Query offline mmap catalog for \"{}\"", trimmed),
                action_icon: "icon-search".to_string(),
                target_game_ids: Vec::new(),
                target_titles: Vec::new(),
                console_id: None,
                path_hint: None,
                min_size_gb: None,
                session_minutes: None,
                capsule_format: None,
                query_text: Some(trimmed.to_string()),
                requires_folder_prompt: false,
            });
        }
    }

    let is_semantic_only = steps.len() == 1 && steps[0].kind == PilotActionKind::SemanticCatalogSearch;
    let headline = if is_semantic_only {
        format!("Semantic Search: \"{}\"", trimmed)
    } else if steps.len() == 1 {
        steps[0].label.clone()
    } else {
        format!("{}-Step Autonomous Plan", steps.len())
    };

    let preview_summary = steps
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}: {}", i + 1, s.label, s.detail))
        .collect::<Vec<_>>()
        .join(" | ");

    PilotPlan {
        raw_query: trimmed.to_string(),
        headline,
        mode: if is_semantic_only {
            "semantic_search".to_string()
        } else {
            "action_plan".to_string()
        },
        confidence: if is_semantic_only { 0.72 } else { 0.96 },
        steps,
        requires_folder_prompt,
        preview_summary,
        semantic_matches: Vec::new(),
        ai_refined: false,
        elapsed_us: started.elapsed().as_micros() as u64,
    }
}

fn try_refine_with_foundry_ai(query: &str, library: &[GameEntry], plan: &mut PilotPlan) {
    let sample_titles: Vec<&str> = library.iter().take(25).map(|g| g.title.as_str()).collect();
    let system_prompt = "You are Duke Pilot, an offline Windows game launcher planner. Output ONE concise semicolon-separated command string using verbs like: shrink over N GB; setup cemu; import <path>; freeze <game>; launch <game>; backup <game>; apply modpack <game>; export capsule <game>. No markdown.";
    let user_prompt = format!(
        "Installed games sample: [{}]. User command: \"{}\". Normalize into canonical step commands:",
        sample_titles.join(", "),
        query
    );
    if let Some((reply, _model)) = duke_ai_rt::query_foundry_local_chat(system_prompt, &user_prompt) {
        let candidate = reply.trim();
        if !candidate.is_empty() && candidate.len() < 260 {
            let refined = parse_pilot_command(candidate, library);
            if !refined.steps.is_empty()
                && refined.steps[0].kind != PilotActionKind::SemanticCatalogSearch
            {
                plan.steps = refined.steps;
                plan.headline = refined.headline;
                plan.mode = refined.mode;
                plan.confidence = 0.88;
                plan.requires_folder_prompt = refined.requires_folder_prompt;
                plan.preview_summary = refined.preview_summary;
                plan.ai_refined = true;
            }
        }
    }
}

#[tauri::command]
pub async fn plan_duke_pilot_command(
    app: AppHandle,
    query: String,
    use_local_ai: Option<bool>,
) -> Result<PilotPlan, String> {
    let started = Instant::now();
    let library = crate::load_library_from_disk(&app).unwrap_or_default();
    let mut plan = parse_pilot_command(&query, &library);

    if use_local_ai.unwrap_or(false)
        && plan.steps.len() == 1
        && plan.steps[0].kind == PilotActionKind::SemanticCatalogSearch
    {
        let q_clone = query.clone();
        let lib_clone = library.clone();
        if let Ok(refined) = tokio::task::spawn_blocking(move || {
            let mut tmp = parse_pilot_command(&q_clone, &lib_clone);
            try_refine_with_foundry_ai(&q_clone, &lib_clone, &mut tmp);
            tmp
        })
        .await
        {
            if refined.ai_refined {
                plan = refined;
            }
        }
    }

    // Attach quick <3 ms DKCAT003 semantic matches when query ends in search mode.
    if plan.mode == "semantic_search" && !plan.raw_query.is_empty() {
        let qparams = CatalogQueryParams {
            query: plan.raw_query.clone(),
            limit: 8,
            ..Default::default()
        };
        if let Ok(page) = query_catalog_mmap(qparams).await {
            plan.semantic_matches = page.items;
        }
    }

    plan.elapsed_us = started.elapsed().as_micros() as u64;
    Ok(plan)
}

#[tauri::command]
pub async fn execute_duke_pilot_plan(
    app: AppHandle,
    plan: PilotPlan,
    explicit_import_path: Option<String>,
) -> Result<PilotExecutionReport, String> {
    let mut results = Vec::new();
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut needs_folder_prompt = false;
    let mut follow_up_action: Option<String> = None;
    let mut follow_up_query: Option<String> = None;

    for step in &plan.steps {
        let res: Result<String, String> = match step.kind {
            PilotActionKind::ShrinkGames => {
                if step.target_game_ids.is_empty() {
                    Err("No installed games matched the size threshold.".to_string())
                } else {
                    let mut shrunk = Vec::new();
                    for gid in &step.target_game_ids {
                        crate::compress_game_folder_core(app.clone(), gid.clone()).await?;
                        shrunk.push(gid.clone());
                    }
                    Ok(format!(
                        "Compressed {} game(s): {}",
                        shrunk.len(),
                        step.target_titles.join(", ")
                    ))
                }
            }
            PilotActionKind::SetupEmulator => {
                let cid = step
                    .console_id
                    .clone()
                    .ok_or_else(|| "Missing emulator identifier.".to_string())?;
                let _statuses = emulators::install_emulator_for_console(app.clone(), cid.clone()).await?;
                Ok(format!("Configured portable emulator '{}'.", cid))
            }
            PilotActionKind::ImportFolder => {
                let chosen_path = explicit_import_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .or_else(|| step.path_hint.clone());
                if let Some(path_str) = chosen_path {
                    let p = PathBuf::from(&path_str);
                    if p.exists() {
                        let rep = crate::ingest_dropped_paths_core(app.clone(), vec![path_str.clone()]).await?;
                        Ok(format!(
                            "Imported {} item(s) from {}",
                            rep.added_titles.len(),
                            path_str
                        ))
                    } else {
                        Err(format!("Folder path not found: {}", path_str))
                    }
                } else {
                    needs_folder_prompt = true;
                    follow_up_action = Some("prompt_import_folder".to_string());
                    Ok("Waiting for folder selection to finish import.".to_string())
                }
            }
            PilotActionKind::SuspendGame => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game to freeze.".to_string())?;
                let _ = crate::suspend_or_resume_game_core(
                    app.clone(),
                    Some(gid),
                    Some("suspend".to_string()),
                )
                .await?;
                Ok(format!(
                    "Suspended {}",
                    step.target_titles.first().map(String::as_str).unwrap_or("game")
                ))
            }
            PilotActionKind::ResumeGame => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game to resume.".to_string())?;
                let _ = crate::suspend_or_resume_game_core(
                    app.clone(),
                    Some(gid),
                    Some("resume".to_string()),
                )
                .await?;
                Ok(format!(
                    "Resumed {}",
                    step.target_titles.first().map(String::as_str).unwrap_or("game")
                ))
            }
            PilotActionKind::LaunchGame => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game to launch.".to_string())?;
                let library = crate::load_library_from_disk(&app)?;
                let game = library
                    .iter()
                    .find(|g| g.id == gid)
                    .ok_or_else(|| "Game missing from library.".to_string())?
                    .clone();
                crate::launch_game_headless(
                    app.clone(),
                    game.exe_path.clone(),
                    Some(game.id.clone()),
                    game.launch_args.clone(),
                )
                .await?;
                Ok(format!("Launched {}", game.title))
            }
            PilotActionKind::BackupSaves => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game for save backup.".to_string())?;
                let msg = ludusavi::backup_game_saves_core(Some(gid), None, None, None).await?;
                Ok(msg)
            }
            PilotActionKind::ApplyModpack => {
                if let Some(ref dkpack_file) = step.path_hint {
                    let target_gid = step.target_game_ids.first().map(String::as_str);
                    let rep = dkpack::ingest_dropped_dkpack_file(
                        &app,
                        std::path::Path::new(dkpack_file),
                        target_gid,
                    )
                    .await?;
                    Ok(rep.summary)
                } else {
                    let gid = step
                        .target_game_ids
                        .first()
                        .cloned()
                        .ok_or_else(|| "Could not identify target game mod loadout.".to_string())?;
                    let res = mod_graph::activate_game_mod_loadout(app.clone(), gid, None).await?;
                    Ok(res.report.summary)
                }
            }
            PilotActionKind::ExportCapsule => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game for capsule export.".to_string())?;
                let rep = duke_capsule::export_game_capsule_core(
                    &app,
                    &gid,
                    None,
                    step.capsule_format.clone(),
                    true,
                )
                .await?;
                Ok(rep.summary)
            }
            PilotActionKind::ExportDkpack => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Could not identify target game for .dkpack export.".to_string())?;
                let rep = dkpack::export_game_dkpack(app.clone(), gid, None).await?;
                Ok(format!(
                    "Exported .dkpack recipe '{}' ({} bytes) to {}",
                    rep.pack_id, rep.size_bytes, rep.output_path
                ))
            }
            PilotActionKind::ScanMalware => {
                let target_gid = step.target_game_ids.first().cloned();
                let library = crate::load_library_from_disk(&app)?;
                if let Some(gid) = target_gid {
                    let entry = library
                        .into_iter()
                        .find(|g| g.id == gid)
                        .ok_or_else(|| "Target game not found.".to_string())?;
                    let shield = crate::pe_inspect::inspect_pe_security_shield_file(
                        std::path::Path::new(&entry.exe_path),
                        false,
                        None,
                    )?;
                    Ok(format!(
                        "Security Shield v2 scanned {}: badge={} ({} behavioral finding(s)).",
                        entry.title,
                        shield.shield_badge,
                        shield.suspicious_behaviors.len()
                    ))
                } else {
                    let mut scanned = 0usize;
                    let mut flagged = 0usize;
                    for entry in library.iter().take(25) {
                        let p = std::path::Path::new(&entry.exe_path);
                        if p.is_file() {
                            if let Ok(sh) = crate::pe_inspect::inspect_pe_security_shield_file(p, false, None) {
                                scanned += 1;
                                if sh.shield_badge != "Safe" {
                                    flagged += 1;
                                }
                            }
                        }
                    }
                    Ok(format!(
                        "Security Shield v2 scanned {scanned} binaries ({flagged} warning/blocked)."
                    ))
                }
            }
            PilotActionKind::BlockInternet => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Identify which game to lock offline.".to_string())?;
                let st = crate::duke_jail::set_game_jail_policy(app.clone(), gid, false, true).await?;
                Ok(st.summary)
            }
            PilotActionKind::FindFixes => {
                let gid = step
                    .target_game_ids
                    .first()
                    .cloned()
                    .ok_or_else(|| "Identify which game to diagnose fixes for.".to_string())?;
                let rep = crate::fix_finder::analyze_game_crash_or_preflight(gid, None, None).await?;
                Ok(rep.plain_english_cause)
            }
            PilotActionKind::VerifyBios => {
                let rep = crate::bios_vault::get_bios_vault_status().await?;
                Ok(rep.summary)
            }
            PilotActionKind::DiskPressureAutopilot => {
                let rep = crate::deep_shrink::evaluate_disk_pressure_autopilot(
                    None,
                    None,
                    Some(true),
                )
                .await?;
                Ok(rep.summary)
            }
            PilotActionKind::ToggleCouchMode => {
                follow_up_action = Some("toggle_couch_mode".to_string());
                Ok("Toggling Duke Deck couch interface.".to_string())
            }
            PilotActionKind::PlayTonight => {
                follow_up_action = Some("open_play_tonight".to_string());
                follow_up_query = step.session_minutes.map(|m| m.to_string());
                let _ = app.emit("duke-pilot-open-play-tonight", step.session_minutes.unwrap_or(60));
                Ok("Activated Play Tonight smart session picker.".to_string())
            }
            PilotActionKind::SemanticCatalogSearch => {
                follow_up_action = Some("open_semantic_catalog".to_string());
                follow_up_query = step.query_text.clone();
                if let Some(ref q) = step.query_text {
                    let _ = app.emit("duke-pilot-semantic-search", q);
                }
                Ok("Opened Search Games with DKCAT003 semantic filter.".to_string())
            }
        };

        match res {
            Ok(msg) => {
                succeeded += 1;
                results.push(PilotStepExecutionResult {
                    step_id: step.id.clone(),
                    kind: step.kind.clone(),
                    ok: true,
                    message: msg,
                });
            }
            Err(err) => {
                failed += 1;
                results.push(PilotStepExecutionResult {
                    step_id: step.id.clone(),
                    kind: step.kind.clone(),
                    ok: false,
                    message: err,
                });
            }
        }
    }

    let summary = results
        .iter()
        .map(|r| r.message.as_str())
        .collect::<Vec<_>>()
        .join(" • ");

    Ok(PilotExecutionReport {
        ok: failed == 0,
        steps_succeeded: succeeded,
        steps_failed: failed,
        results,
        summary,
        needs_folder_prompt,
        follow_up_action,
        follow_up_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_game(id: &str, title: &str, size_gb: u64) -> GameEntry {
        GameEntry {
            id: id.to_string(),
            title: title.to_string(),
            exe_path: format!("D:\\Games\\{}\\{}.exe", id, id),
            install_dir: format!("D:\\Games\\{}", id),
            size_bytes: size_gb * 1024 * 1024 * 1024,
            size_formatted: format!("{} GB", size_gb),
            cover_url: String::new(),
            hero_url: None,
            icon_path: None,
            genre: "Action".to_string(),
            platform: "PC".to_string(),
            source: "manual".to_string(),
            acquisition_source: "manual".to_string(),
            is_favorite: false,
            is_installed: true,
            added_at: 0,
            last_played: None,
            steam_app_id: None,
            app_id: None,
            playtime_seconds: 0,
            imported_playtime_secs: 0,
            duke_playtime_secs: 0,
            last_played_ts: None,
            steam_price_cents: None,
            steam_price_formatted: None,
            launch_args: None,
            sandbox_enabled: false,
            nvme_prefetch_enabled: None,
            version_tag: String::new(),
            update_feed_url: String::new(),
            latest_version_tag: String::new(),
            ..Default::default()
        }
    }

    fn mock_library() -> Vec<GameEntry> {
        vec![
            mock_game("arc", "ARC Raiders", 32),
            mock_game("balatro", "Balatro", 1),
            mock_game("er", "Elden Ring", 68),
            mock_game("pz", "Project Zomboid", 8),
            mock_game("cp77", "Cyberpunk 2077", 85),
        ]
    }

    #[test]
    fn pilot_parses_all_multistep_natural_language_commands() {
        let lib = mock_library();

        // 1. Shrink biggest games over 50 GB
        let p1 = parse_pilot_command("Shrink my biggest games over 50 GB", &lib);
        assert_eq!(p1.steps.len(), 1);
        assert_eq!(p1.steps[0].kind, PilotActionKind::ShrinkGames);
        assert_eq!(p1.steps[0].min_size_gb, Some(50.0));
        assert_eq!(p1.steps[0].target_titles, vec!["Cyberpunk 2077", "Elden Ring"]);

        // 2. Set up Cemu and import my Wii U folder
        let p2 = parse_pilot_command("Set up Cemu and import my Wii U folder", &lib);
        assert_eq!(p2.steps.len(), 2);
        assert_eq!(p2.steps[0].kind, PilotActionKind::SetupEmulator);
        assert_eq!(p2.steps[0].console_id.as_deref(), Some("cemu"));
        assert_eq!(p2.steps[1].kind, PilotActionKind::ImportFolder);
        assert!(p2.requires_folder_prompt);

        // 3. Freeze ARC Raiders and launch Balatro
        let p3 = parse_pilot_command("Freeze ARC Raiders and launch Balatro", &lib);
        assert_eq!(p3.steps.len(), 2);
        assert_eq!(p3.steps[0].kind, PilotActionKind::SuspendGame);
        assert_eq!(p3.steps[0].target_titles, vec!["ARC Raiders"]);
        assert_eq!(p3.steps[1].kind, PilotActionKind::LaunchGame);
        assert_eq!(p3.steps[1].target_titles, vec!["Balatro"]);

        // 4. Back up Elden Ring saves and apply my modpack (carries game context forward!)
        let p4 = parse_pilot_command("Back up Elden Ring saves and apply my modpack", &lib);
        assert_eq!(p4.steps.len(), 2);
        assert_eq!(p4.steps[0].kind, PilotActionKind::BackupSaves);
        assert_eq!(p4.steps[0].target_titles, vec!["Elden Ring"]);
        assert_eq!(p4.steps[1].kind, PilotActionKind::ApplyModpack);
        assert_eq!(p4.steps[1].target_titles, vec!["Elden Ring"]);

        // 5. Export portable capsule for Project Zomboid
        let p5 = parse_pilot_command("Export portable capsule for Project Zomboid", &lib);
        assert_eq!(p5.steps.len(), 1);
        assert_eq!(p5.steps[0].kind, PilotActionKind::ExportCapsule);
        assert_eq!(p5.steps[0].target_titles, vec!["Project Zomboid"]);

        // 6. Semantic catalog query falls back cleanly without breaking "co-op physics"
        let p6 = parse_pilot_command("co-op physics chaos games under 5 GB", &lib);
        assert_eq!(p6.steps.len(), 1);
        assert_eq!(p6.steps[0].kind, PilotActionKind::SemanticCatalogSearch);

        // 7. Security, Fix, BIOS, Disk Pressure actions
        let p7 = parse_pilot_command("block internet for Balatro", &lib);
        assert_eq!(p7.steps[0].kind, PilotActionKind::BlockInternet);
        assert_eq!(p7.steps[0].target_titles, vec!["Balatro"]);

        let p8 = parse_pilot_command("find fixes for Cyberpunk 2077", &lib);
        assert_eq!(p8.steps[0].kind, PilotActionKind::FindFixes);
        assert_eq!(p8.steps[0].target_titles, vec!["Cyberpunk 2077"]);

        let p9 = parse_pilot_command("scan for malware", &lib);
        assert_eq!(p9.steps[0].kind, PilotActionKind::ScanMalware);

        let p10 = parse_pilot_command("verify bios", &lib);
        assert_eq!(p10.steps[0].kind, PilotActionKind::VerifyBios);

        let p11 = parse_pilot_command("disk pressure", &lib);
        assert_eq!(p11.steps[0].kind, PilotActionKind::DiskPressureAutopilot);
    }
}
