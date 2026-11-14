use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static MCP_SUSPENDED_PIDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();

fn suspended_pid_set() -> &'static Mutex<HashSet<u32>> {
    MCP_SUSPENDED_PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Debug, Clone, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Clone, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub fn build_mcp_tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "list_installed_games",
                "description": "Lists games registered in the local Duke Launcher library (portable, Steam, Epic, GOG, Xbox).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Optional case-insensitive title filter" },
                        "source": { "type": "string", "description": "Optional store source filter (Steam, Epic, GOG, Xbox, Portable)" }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "search_catalog",
                "description": "Searches the cached Steam catalog, live Steam Store metadata, and enabled Hydra community JSON sources.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Game title or keyword query" },
                        "limit": { "type": "integer", "description": "Maximum number of results per source (default 20)" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            },
            {
                "name": "launch_game",
                "description": "Launches an installed game executable with NTFS Mark-of-the-Web unblocking and telemetry scrubbing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "game_id": { "type": "string", "description": "Library game ID" },
                        "title": { "type": "string", "description": "Game title fallback lookup" },
                        "exe_path": { "type": "string", "description": "Direct path to executable" },
                        "launch_args": { "type": "string", "description": "Optional CLI arguments" }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "scan_binary",
                "description": "Runs Duke Launcher's pure-Rust PE header, SHA-256, and crack-aware whitelist security scan on a local file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to .exe, .dll, or archive file" }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            },
            {
                "name": "backup_saves",
                "description": "Discovers real save paths via Ludusavi manifest + heuristics and writes a rolling-5 ZIP snapshot.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "game_id": { "type": "string", "description": "Library game ID" },
                        "title": { "type": "string", "description": "Game title" },
                        "install_dir": { "type": "string", "description": "Game install directory" },
                        "exe_path": { "type": "string", "description": "Game executable path" }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "import_libraries",
                "description": "Scans Steam (VDF/ACF), Epic Games (.item), GOG Galaxy (Registry), and Xbox Game Pass folders into library.json.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "diagnose_game_crash",
                "description": "Parses crash logs, Windows .dmp minidumps, mod-graph DLL collisions, and GPU driver state via Duke Doctor.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "game_id": { "type": "string", "description": "Library game ID to diagnose" },
                        "extra_log_path": { "type": "string", "description": "Optional explicit path to Crash.log, Player.log, or .dmp file" }
                    },
                    "required": ["game_id"],
                    "additionalProperties": false
                }
            },
            {
                "name": "suspend_or_resume_game",
                "description": "Freezes or thaws a running game process via NtSuspendProcess / NtResumeProcess and trims physical working-set RAM.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "game_id": { "type": "string", "description": "Library game ID to resolve running process" },
                        "pid": { "type": "integer", "description": "Optional explicit target PID" },
                        "action": {
                            "type": "string",
                            "enum": ["toggle", "suspend", "resume"],
                            "description": "Operation mode (default: toggle)"
                        }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "discover_saves",
                "description": "Discovers save-game directories for an installed game using the Ludusavi manifest and Windows path heuristics.",
                "inputSchema": {
                    "type": "object",
                    "required": ["game_id"],
                    "properties": {
                        "game_id": { "type": "string", "description": "Library game ID, executable path, or exact title" },
                        "sync_if_missing": {
                            "type": "boolean",
                            "description": "Fetch Ludusavi manifest over DoH if local cache is empty/stale (default: false)"
                        }
                    },
                    "additionalProperties": false
                }
            }
        ]
    })
}

fn mcp_text_result(payload: Value, is_error: bool) -> Value {
    let text = if let Some(s) = payload.as_str() {
        s.to_string()
    } else {
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
    };
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "isError": is_error
    })
}

async fn execute_tool_call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "list_installed_games" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let source_filter = args
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_lowercase();

            let library = tokio::task::spawn_blocking(crate::load_library_standalone)
                .await
                .map_err(|e| format!("Failed reading library: {}", e))??;

            let filtered: Vec<Value> = library
                .into_iter()
                .filter(|g| {
                    (query.is_empty() || g.title.to_lowercase().contains(&query))
                        && (source_filter.is_empty()
                            || g.source.to_lowercase() == source_filter
                            || g.platform.to_lowercase().contains(&source_filter))
                })
                .map(|g| {
                    json!({
                        "id": g.id,
                        "title": g.title,
                        "source": g.source,
                        "platform": g.platform,
                        "exePath": g.exe_path,
                        "installDir": g.install_dir,
                        "sizeFormatted": g.size_formatted,
                        "isInstalled": g.is_installed,
                        "steamAppId": g.app_id,
                        "playtimeSeconds": g.playtime_seconds,
                        "isFavorite": g.is_favorite,
                    })
                })
                .collect();

            Ok(json!({
                "count": filtered.len(),
                "games": filtered
            }))
        }
        "search_catalog" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if query.is_empty() {
                return Err("Missing required parameter: query".to_string());
            }
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;

            let query_owned = query.clone();
            let mmap_hits = tokio::task::spawn_blocking(move || {
                crate::catalog_mmap::search_catalog_via_mmap(&query_owned, limit)
            })
            .await
            .ok()
            .flatten();

            let matched_entries = match mmap_hits {
                Some(hits) => hits,
                None => {
                    let q_lower = query.to_lowercase();
                    let cache =
                        tokio::task::spawn_blocking(crate::load_catalog_cache_standalone)
                            .await
                            .unwrap_or_default();
                    cache
                        .entries
                        .into_iter()
                        .filter(|e| e.name.to_lowercase().contains(&q_lower))
                        .take(limit)
                        .collect()
                }
            };

            let mut catalog_hits: Vec<Value> = matched_entries
                .into_iter()
                .map(|e| {
                    json!({
                        "appId": e.app_id,
                        "name": e.name,
                        "price": e.price_formatted,
                        "reviewPct": e.review_pct,
                        "ccu": e.ccu,
                    })
                })
                .collect();

            if catalog_hits.is_empty() && query.chars().count() >= 2 {
                let search_url = format!(
                    "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
                    urlencoding::encode(&query)
                );
                if let Ok(val) = crate::fetch_doh_json(&search_url).await {
                    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
                        for item in items.iter().take(limit) {
                            catalog_hits.push(json!({
                                "appId": item.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                                "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                "price": item.get("price")
                                    .and_then(|p| p.get("final"))
                                    .and_then(|v| v.as_u64())
                                    .map(|c| format!("${:.2}", c as f64 / 100.0))
                                    .unwrap_or_else(|| "Free / Unknown".to_string()),
                            }));
                        }
                    }
                }
            }

            let sources_hits = tokio::task::spawn_blocking(move || {
                crate::sources::search_imported_sources_core(&query, None, 0, limit)
            })
            .await
            .map_err(|e| format!("Failed searching imported sources: {}", e))?;

            Ok(json!({
                "catalogMatches": catalog_hits,
                "importedSourceMatches": sources_hits.items,
                "totalSourceMatches": sources_hits.total_matches
            }))
        }
        "launch_game" => {
            let game_id = args.get("game_id").and_then(|v| v.as_str()).map(str::trim);
            let title = args.get("title").and_then(|v| v.as_str()).map(str::trim);
            let explicit_exe = args.get("exe_path").and_then(|v| v.as_str()).map(str::trim);
            let launch_args = args.get("launch_args").and_then(|v| v.as_str()).map(str::to_string);

            let library = tokio::task::spawn_blocking(crate::load_library_standalone)
                .await
                .map_err(|e| format!("Failed reading library: {}", e))??;

            let matched = library.iter().find(|g| {
                game_id.map(|id| g.id == id).unwrap_or(false)
                    || explicit_exe
                        .map(|p| g.exe_path.eq_ignore_ascii_case(p))
                        .unwrap_or(false)
                    || title
                        .map(|t| g.title.eq_ignore_ascii_case(t))
                        .unwrap_or(false)
            });

            let (exe_to_run, resolved_gid, resolved_title, saved_args) = if let Some(ent) = matched {
                (
                    ent.exe_path.clone(),
                    Some(ent.id.clone()),
                    ent.title.clone(),
                    launch_args.or_else(|| ent.launch_args.clone()),
                )
            } else if let Some(p) = explicit_exe.filter(|s| !s.is_empty()) {
                (p.to_string(), None, p.to_string(), launch_args)
            } else {
                return Err(
                    "Specify a valid game_id, title, or exe_path to launch.".to_string()
                );
            };

            let pid = crate::launch_game_detached_headless(
                &exe_to_run,
                resolved_gid.as_deref(),
                saved_args.as_deref(),
            )
            .await?;

            Ok(json!({
                "launched": true,
                "pid": pid,
                "title": resolved_title,
                "exePath": exe_to_run,
                "launchArgs": saved_args
            }))
        }
        "scan_binary" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if path.is_empty() {
                return Err("Missing required parameter: path".to_string());
            }
            let report = crate::scan_file_security_core(path).await?;
            serde_json::to_value(report).map_err(|e| format!("Serialize error: {}", e))
        }
        "backup_saves" => {
            let game_id = args.get("game_id").and_then(|v| v.as_str()).map(str::to_string);
            let title = args.get("title").and_then(|v| v.as_str()).map(str::to_string);
            let install_dir = args
                .get("install_dir")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let exe_path = args.get("exe_path").and_then(|v| v.as_str()).map(str::to_string);

            let zip_path = crate::ludusavi::backup_game_saves_core(
                game_id.clone(),
                title,
                install_dir,
                exe_path,
            )
            .await?;

            let discovery = if let Some(ref gid) = game_id {
                crate::ludusavi::discover_game_save_paths_core(gid, false)
                    .await
                    .ok()
            } else {
                None
            };

            Ok(json!({
                "zipPath": zip_path,
                "discoveredPaths": discovery.map(|d| d.existing_paths).unwrap_or_default()
            }))
        }
        "import_libraries" => {
            let res = crate::importers::import_existing_libraries_core().await?;
            Ok(json!({
                "added": res.added,
                "updated": res.updated,
                "total": res.total,
                "bySource": res.by_source
            }))
        }
        "discover_saves" => {
            let raw_target = args
                .get("game_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if raw_target.is_empty() {
                return Err("Missing required parameter: game_id".to_string());
            }
            let sync_if_missing = args
                .get("sync_if_missing")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let resolved_id = match crate::ludusavi::discover_game_save_paths_core(
                &raw_target,
                sync_if_missing,
            )
            .await
            {
                Ok(report) => return serde_json::to_value(report).map_err(|e| format!("Serialize error: {}", e)),
                Err(_) => {
                    let library = tokio::task::spawn_blocking(crate::load_library_standalone)
                        .await
                        .map_err(|e| format!("Failed reading library: {}", e))??;
                    library
                        .into_iter()
                        .find(|g| g.title.eq_ignore_ascii_case(&raw_target))
                        .map(|g| g.id)
                        .ok_or_else(|| format!("Game '{}' not found in library.", raw_target))?
                }
            };
            let report = crate::ludusavi::discover_game_save_paths_core(
                &resolved_id,
                sync_if_missing,
            )
            .await?;
            serde_json::to_value(report).map_err(|e| format!("Serialize error: {}", e))
        }
        "diagnose_game_crash" => {
            let game_id = args
                .get("game_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Missing required argument: game_id".to_string())?
                .to_string();
            let extra_log = args
                .get("extra_log_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let report = crate::duke_doctor::diagnose_game_crash_sync(&game_id, extra_log.as_deref())?;
            serde_json::to_value(report).map_err(|e| format!("Serialize error: {}", e))
        }
        "suspend_or_resume_game" => {
            let explicit_pid = args
                .get("pid")
                .and_then(|v| v.as_u64())
                .and_then(|p| u32::try_from(p).ok())
                .filter(|&p| p > 0);
            let game_id = args.get("game_id").and_then(|v| v.as_str()).map(str::trim);
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("toggle")
                .to_lowercase();

            let target_pid = if let Some(p) = explicit_pid {
                p
            } else if let Some(gid) = game_id.filter(|s| !s.is_empty()) {
                if let Some(active_pid) = crate::find_active_session_pid(gid) {
                    active_pid
                } else {
                    let library = tokio::task::spawn_blocking(crate::load_library_standalone)
                        .await
                        .map_err(|e| format!("Failed reading library: {}", e))??;
                    let entry = library
                        .into_iter()
                        .find(|g| g.id == gid || g.title.eq_ignore_ascii_case(gid))
                        .ok_or_else(|| format!("Game '{}' not found in library.", gid))?;
                    let exe_name = Path::new(&entry.exe_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| "Invalid game executable path.".to_string())?;
                    duke_core::win32_ffi::find_pid_by_exe_name(exe_name).ok_or_else(|| {
                        format!("No running process found for '{}' ({}).", entry.title, exe_name)
                    })?
                }
            } else {
                crate::first_active_session_pid().ok_or_else(|| {
                    "No running game found. Provide 'game_id' or 'pid'.".to_string()
                })?
            };

            let currently_suspended = {
                suspended_pid_set().lock().unwrap().contains(&target_pid)
            };

            let do_suspend = match action.as_str() {
                "suspend" => true,
                "resume" => false,
                _ => !currently_suspended,
            };

            if do_suspend {
                let freed_mb = duke_core::win32_ffi::suspend_external_pid(target_pid)?;
                suspended_pid_set().lock().unwrap().insert(target_pid);
                Ok(json!({
                    "pid": target_pid,
                    "isSuspended": true,
                    "freedRamMb": freed_mb
                }))
            } else {
                duke_core::win32_ffi::resume_external_pid(target_pid)?;
                suspended_pid_set().lock().unwrap().remove(&target_pid);
                Ok(json!({
                    "pid": target_pid,
                    "isSuspended": false
                }))
            }
        }
        other => Err(format!("Unknown MCP tool '{}'", other)),
    }
}

pub fn handle_mcp_line(line: &str, rt: &tokio::runtime::Runtime) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
        Ok(r) => r,
        Err(e) => {
            let err_resp = JsonRpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(json!({ "code": -32700, "message": format!("Parse error: {}", e) })),
            };
            return serde_json::to_string(&err_resp).ok();
        }
    };

    // Notifications have no id (or Null when omitted) and MUST NOT send a JSON-RPC reply
    if req.method.starts_with("notifications/") || req.id.is_none() {
        return None;
    }

    let id = req.id.unwrap_or(Value::Null);
    let resp = match req.method.as_str() {
        "initialize" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "duke-launcher",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        },
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({})),
            error: None,
        },
        "tools/list" => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(build_mcp_tools_list()),
            error: None,
        },
        "tools/call" => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            let call_res = rt.block_on(execute_tool_call(tool_name, &args));
            let result_val = match call_res {
                Ok(val) => mcp_text_result(val, false),
                Err(err_msg) => mcp_text_result(Value::String(err_msg), true),
            };
            JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result_val),
                error: None,
            }
        }
        unknown => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(json!({
                "code": -32601,
                "message": format!("Method not found: {}", unknown)
            })),
        },
    };

    serde_json::to_string(&resp).ok()
}

/// Runs headless stdio JSON-RPC 2.0 MCP server (0 MB WebView2 RAM).
pub fn run_mcp_stdio_server() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed initializing tokio runtime for Duke Launcher --mcp");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line_res in stdin.lock().lines() {
        let Ok(line) = line_res else {
            break;
        };
        if let Some(reply_json) = handle_mcp_line(&line, &rt) {
            if writeln!(out, "{}", reply_json).is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_initialize_and_tools_list_return_all_8_tools() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let init_reply = handle_mcp_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
            &rt,
        )
        .unwrap();
        let init_val: Value = serde_json::from_str(&init_reply).unwrap();
        assert_eq!(init_val["result"]["serverInfo"]["name"], "duke-launcher");

        let list_reply = handle_mcp_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            &rt,
        )
        .unwrap();
        let list_val: Value = serde_json::from_str(&list_reply).unwrap();
        let tools = list_val["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                "list_installed_games",
                "search_catalog",
                "launch_game",
                "scan_binary",
                "backup_saves",
                "import_libraries",
                "diagnose_game_crash",
                "suspend_or_resume_game",
                "discover_saves",
            ]
        );
    }

    #[test]
    fn mcp_notification_emits_no_response() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(handle_mcp_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
            &rt
        )
        .is_none());
    }
}
