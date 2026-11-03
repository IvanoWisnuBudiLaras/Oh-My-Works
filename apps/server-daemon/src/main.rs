//! Comprehensive server telemetry, CodeGraph memory, worktree & unified harness daemon for Oh My Works.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use omw_guardian::{Guardian, HealthLevel};

struct BackgroundJob {
    id: String,
    pid: u32,
    command: String,
    started_at: u64,
    child: std::process::Child,
}

static BACKGROUND_JOBS: Mutex<Vec<BackgroundJob>> = Mutex::new(Vec::new());

struct DeployedProject {
    name: String,
    repo_url: String,
    port: u16,
    pid: u32,
    command: String,
    status: String,
    log_path: PathBuf,
    started_at: u64,
}

static DEPLOYED_PROJECTS: Mutex<Vec<DeployedProject>> = Mutex::new(Vec::new());

fn get_process_rss_mb(pid: u32) -> f64 {
    if let Ok(content) = fs::read_to_string(format!("/proc/{}/statm", pid)) {
        if let Some(res_pages) = content.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
            return (res_pages * 4096) as f64 / (1024.0 * 1024.0);
        }
    }
    0.0
}

fn is_process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

fn detect_system_profile() -> serde_json::Value {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    let is_hyprland = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok();

    let check_tool = |name: &str| -> bool {
        Command::new("which")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let tools = serde_json::json!({
        "hyprctl": check_tool("hyprctl"),
        "grim": check_tool("grim"),
        "slurp": check_tool("slurp"),
        "tesseract": check_tool("tesseract"),
        "wtype": check_tool("wtype"),
        "wl_copy": check_tool("wl-copy"),
        "git": check_tool("git"),
        "cargo": check_tool("cargo"),
        "node": check_tool("node"),
        "mise": check_tool("mise"),
        "pacman": check_tool("pacman"),
    });

    let mut mem_total_mb = 0;
    let mut mem_avail_mb = 0;
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 { mem_total_mb = parts[1].parse::<u64>().unwrap_or(0) / 1024; }
            } else if line.starts_with("MemAvailable:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 { mem_avail_mb = parts[1].parse::<u64>().unwrap_or(0) / 1024; }
            }
        }
    }

    serde_json::json!({
        "os": os,
        "arch": arch,
        "display_server": if is_wayland { "wayland" } else { "x11" },
        "compositor": if is_hyprland { "hyprland" } else { "generic" },
        "memory": {
            "total_mb": mem_total_mb,
            "available_mb": mem_avail_mb
        },
        "tools": tools
    })
}

use omw_memory::{CodeGraphStore, SessionTreeItem, ChatTurn};
use omw_session::SessionHarvester;

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

fn omw_dir() -> PathBuf {
    home_dir().join(".omw")
}

fn db_path() -> PathBuf {
    omw_dir().join("codegraph.db")
}

fn get_db() -> omw_memory::Result<CodeGraphStore> {
    let _ = fs::create_dir_all(omw_dir());
    CodeGraphStore::open(&db_path())
}

fn urldecode(s: &str) -> String {
    let mut res = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                res.push(b);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            res.push(b' ');
            i += 1;
            continue;
        }
        res.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&res).to_string()
}

fn parse_query(uri: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(pos) = uri.find('?') {
        let q = &uri[pos + 1..];
        for pair in q.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                map.insert(k.to_string(), urldecode(v));
            }
        }
    }
    map
}

fn is_sensitive_path(path_str: &str) -> bool {
        let file_name = Path::new(path_str)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    let sensitive_exact = [
        ".env", ".env.local", ".env.production", ".env.development",
        "id_rsa", "id_ed25519", "id_ecdsa", "credentials.json", "auth.json",
    ];
    let sensitive_exts = [
        ".pem", ".key", ".enc", ".gpg", ".pfx", ".p12", ".keystore",
    ];

    if file_name.starts_with(".env") {
        return true;
    }
    for exact in sensitive_exact {
        if file_name == exact {
            return true;
        }
    }
    for ext in sensitive_exts {
        if file_name.ends_with(ext) {
            return true;
        }
    }
    false
}

fn detect_collab_context(target_dir: &Path) -> serde_json::Value {
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(target_dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_git {
        return serde_json::json!({
            "is_git": false,
            "authors_count": 1,
            "suggested_mode": "solo",
            "reason": "Not a Git repository (Local standalone project)"
        });
    }

    let authors_output = Command::new("git")
        .args(["shortlog", "-sn", "--all"])
        .current_dir(target_dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let authors_count = authors_output.lines().filter(|l| !l.trim().is_empty()).count();

    let remote_url = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(target_dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let git_user = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(target_dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let is_multi_author = authors_count > 1;
    let is_external_repo = !remote_url.is_empty() && !git_user.is_empty() && 
        !remote_url.to_lowercase().contains(&git_user.to_lowercase().replace(' ', ""));

    let suggested_mode = if is_multi_author || is_external_repo { "collab" } else { "solo" };
    let reason = if is_multi_author {
        format!("Multi-author repository ({} contributors detected)", authors_count)
    } else if is_external_repo {
        format!("Remote origin ({}) may belong to another team or organization", remote_url)
    } else {
        "Single-author private repository".to_string()
    };

    serde_json::json!({
        "is_git": true,
        "authors_count": authors_count,
        "remote_url": remote_url,
        "git_user": git_user,
        "suggested_mode": suggested_mode,
        "reason": reason
    })
}

fn scan_dir_recursive(dir: &Path, max_depth: usize) -> Vec<serde_json::Value> {
    if max_depth == 0 {
        return Vec::new();
    }
    let mut entries = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        let mut paths: Vec<_> = rd.flatten().collect();
        paths.sort_by_key(|e| e.file_name());

        for entry in paths {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            let is_dir = path.is_dir();
            let mut node = serde_json::json!({
                "name": name,
                "path": path.to_string_lossy(),
                "is_dir": is_dir
            });
            if is_dir {
                node["children"] = serde_json::Value::Array(scan_dir_recursive(&path, max_depth - 1));
            }
            entries.push(node);
        }
    }
    entries
}

fn check_harness_observer() -> serde_json::Value {
    let targets = [
        ("Prime Agent", "prime-agent", "Autonomous RLM continual harness & subagents"),
        ("Claude Code", "claude", "Anthropic Claude Code CLI & subagent orchestrator"),
        ("OpenCode", "opencode", "Open-source terminal AI coding assistant"),
        ("Codex", "codex", "Code intelligence & AST indexer"),
        ("Hermes", "hermes", "Local AI research & prompt harness")
    ];

    let mut list = Vec::new();
    for (name, proc_name, desc) in targets {
        let output = Command::new("pgrep").args(["-a", "-f", proc_name]).output();
        let mut pids = Vec::new();
        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(pid_str) = line.split_whitespace().next() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        if pid != std::process::id() {
                            pids.push(pid);
                        }
                    }
                }
            }
        }

        let is_running = !pids.is_empty();
        list.push(serde_json::json!({
            "name": name,
            "binary": proc_name,
            "running": is_running,
            "pids": pids,
            "description": desc,
            "status": if is_running { "ACTIVE / RUNNING" } else { "IDLE" }
        }));
    }

    serde_json::json!({ "harnesses": list })
}


// ----------------------------------------------------
// SQL Studio & Website Deployer Data Models & Helpers
// ----------------------------------------------------

fn execute_sql_query(db_path: &Path, query: &str) -> serde_json::Value {
    let start_t = std::time::Instant::now();
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => return serde_json::json!({ "error": format!("Cannot open database: {}", e) }),
    };

    let trimmed = query.trim();
    let lower = trimmed.to_ascii_lowercase();
    let is_select = lower.starts_with("select") || lower.starts_with("pragma") || lower.starts_with("explain");

    if is_select {
        let mut stmt = match conn.prepare(trimmed) {
            Ok(s) => s,
            Err(e) => return serde_json::json!({ "error": format!("SQL Prepare error: {}", e) }),
        };

        let col_names: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
        let mut rows = Vec::new();

        let mut query_rows = match stmt.query([]) {
            Ok(r) => r,
            Err(e) => return serde_json::json!({ "error": format!("SQL Query error: {}", e) }),
        };

        while let Ok(Some(row)) = query_rows.next() {
            let mut row_vals = Vec::new();
            for i in 0..col_names.len() {
                let val: serde_json::Value = match row.get_ref(i) {
                    Ok(rusqlite::types::ValueRef::Null) => serde_json::Value::Null,
                    Ok(rusqlite::types::ValueRef::Integer(n)) => serde_json::json!(n),
                    Ok(rusqlite::types::ValueRef::Real(f)) => serde_json::json!(f),
                    Ok(rusqlite::types::ValueRef::Text(s)) => {
                        serde_json::json!(String::from_utf8_lossy(s).to_string())
                    }
                    Ok(rusqlite::types::ValueRef::Blob(b)) => {
                        serde_json::json!(format!("<blob {}B>", b.len()))
                    }
                    Err(_) => serde_json::Value::Null,
                };
                row_vals.push(val);
            }
            rows.push(row_vals);
            if rows.len() >= 500 {
                break;
            }
        }

        let elapsed = start_t.elapsed().as_micros() as f64 / 1000.0;
        serde_json::json!({
            "columns": col_names,
            "rows": rows,
            "row_count": rows.len(),
            "execution_time_ms": elapsed
        })
    } else {
        match conn.execute_batch(trimmed) {
            Ok(_) => {
                let changes = conn.changes();
                let elapsed = start_t.elapsed().as_micros() as f64 / 1000.0;
                serde_json::json!({
                    "success": true,
                    "rows_affected": changes,
                    "execution_time_ms": elapsed
                })
            }
            Err(e) => serde_json::json!({ "error": format!("SQL Execution error: {}", e) }),
        }
    }
}

fn load_websites_list(file_path: &Path) -> Vec<serde_json::Value> {
    if let Ok(c) = fs::read_to_string(file_path) {
        if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&c) {
            return arr;
        }
    }
    // Default initial website: TKA Math Web if present
    vec![serde_json::json!({
        "id": "site-tka-math",
        "name": "TKA Math 2026",
        "domain": "ivanowisnubudilaras.web.id",
        "site_type": "proxy",
        "port": 3000,
        "root_dir": "/home/ivano/tka-math-web",
        "start_cmd": "npm run preview",
        "status": "stopped",
        "pid": null,
        "created_at": "2026-11-01"
    })]
}

fn save_websites_list(file_path: &Path, list: &[serde_json::Value]) {
    let _ = fs::write(file_path, serde_json::to_string_pretty(list).unwrap_or_else(|_| "[]".to_string()));
}

fn load_omniroute_config(file_path: &Path) -> serde_json::Value {
    if let Ok(c) = fs::read_to_string(file_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&c) {
            return val;
        }
    }
    serde_json::json!({
        "version": "1.0",
        "providers": [
            {
                "id": "gemini",
                "name": "Google Gemini",
                "base_url": "https://generativelanguage.googleapis.com/v1beta/openai",
                "api_key": "",
                "models": ["gemini-2.5-flash", "gemini-2.5-pro", "gemini-1.5-flash"],
                "enabled": true,
                "priority": 1
            },
            {
                "id": "openai",
                "name": "OpenAI",
                "base_url": "https://api.openai.com/v1",
                "api_key": "",
                "models": ["gpt-4o", "gpt-4o-mini", "o3-mini"],
                "enabled": true,
                "priority": 2
            },
            {
                "id": "anthropic",
                "name": "Anthropic Claude",
                "base_url": "https://api.anthropic.com/v1",
                "api_key": "",
                "models": ["claude-3-7-sonnet", "claude-3-5-haiku"],
                "enabled": true,
                "priority": 3
            },
            {
                "id": "deepseek",
                "name": "DeepSeek",
                "base_url": "https://api.deepseek.com/v1",
                "api_key": "",
                "models": ["deepseek-chat", "deepseek-reasoner"],
                "enabled": true,
                "priority": 4
            },
            {
                "id": "groq",
                "name": "Groq LPU",
                "base_url": "https://api.groq.com/openai/v1",
                "api_key": "",
                "models": ["llama-3.3-70b-versatile"],
                "enabled": false,
                "priority": 5
            },
            {
                "id": "openrouter",
                "name": "OpenRouter",
                "base_url": "https://openrouter.ai/api/v1",
                "api_key": "",
                "models": ["auto", "anthropic/claude-3.5-sonnet"],
                "enabled": false,
                "priority": 6
            },
            {
                "id": "ollama",
                "name": "Ollama Local",
                "base_url": "http://127.0.0.1:11434/v1",
                "api_key": "ollama",
                "models": ["llama3", "qwen2.5-coder"],
                "enabled": false,
                "priority": 7
            }
        ],
        "aliases": {
            "gpt-4o": "gemini-2.5-pro",
            "gpt-4o-mini": "gemini-2.5-flash",
            "fast": "gemini-2.5-flash",
            "coder": "deepseek-chat"
        },
        "cascade": [
            "ag/gemini-3.8-flash-high",
            "mux/glm-5.3-flash",
            "cbcn/deepseek-v4-pro"
        ],
        "routing_policy": "cascade_failover",
        "timeout_secs": 30,
        "max_retries": 3,
        "token_optimization": true
    })
}

fn save_omniroute_config(file_path: &Path, val: &serde_json::Value) -> bool {
    fs::write(file_path, serde_json::to_string_pretty(val).unwrap_or_else(|_| "{}".to_string())).is_ok()
}

fn handle_client(mut stream: TcpStream) {
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    let mut content_length = 0;
    let mut header_end = None;

    loop {
        let n = match stream.read(&mut buffer) {
            Ok(n) if n > 0 => n,
            _ => break,
        };
        data.extend_from_slice(&buffer[..n]);

        if header_end.is_none() {
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                let header_str = String::from_utf8_lossy(&data[..pos]);
                for line in header_str.lines() {
                    if line.to_ascii_lowercase().starts_with("content-length:") {
                        if let Some(val) = line.split(':').nth(1) {
                            content_length = val.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
            }
        }

        if let Some(pos) = header_end {
            if data.len() >= pos + content_length {
                break;
            }
        }
    }

    if data.is_empty() {
        return;
    }

    let h_end = header_end.unwrap_or(data.len());
    let req_header_str = String::from_utf8_lossy(&data[..h_end]);
    let mut lines = req_header_str.lines();
    let req_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    let method = parts.first().copied().unwrap_or("GET");
    let uri = parts.get(1).copied().unwrap_or("/");

    let payload_bytes = if h_end <= data.len() {
        let end = (h_end + content_length).min(data.len());
        &data[h_end..end]
    } else {
        &[]
    };
    let payload = String::from_utf8_lossy(payload_bytes).to_string();
    let payload = payload.as_str();

    let omw = omw_dir();
    let _ = fs::create_dir_all(&omw);
    let skills_file = omw.join("skills.json");
    let query_map = parse_query(uri);

    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    let (status, content_type, body) = if uri.starts_with("/soul") {
        let soul_path = omw.join("SOUL.md");
        if method == "POST" {
            let _ = fs::write(&soul_path, payload);
            ("200 OK", "application/json", r#"{"status":"saved"}"#.to_string())
        } else {
            let content = fs::read_to_string(&soul_path).unwrap_or_else(|_| "# SOUL.md - Workspace Persona\n".to_string());
            let json = serde_json::json!({ "soul": content });
            ("200 OK", "application/json", json.to_string())
        }
    } else if uri.starts_with("/skills/toggle") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let (Some(name), Some(enabled)) = (val["name"].as_str(), val["enabled"].as_bool()) {
                let mut current: serde_json::Value = fs::read_to_string(&skills_file)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                current[name] = serde_json::json!(enabled);
                let _ = fs::write(&skills_file, current.to_string());
            }
        }
        ("200 OK", "application/json", r#"{"status":"updated"}"#.to_string())
    } else if uri.starts_with("/skills/delete") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(name) = val["name"].as_str() {
                let mut current: serde_json::Value = fs::read_to_string(&skills_file)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = current.as_object_mut() {
                    obj.remove(name);
                }
                let _ = fs::write(&skills_file, current.to_string());
            }
        }
        ("200 OK", "application/json", r#"{"status":"deleted"}"#.to_string())
    } else if uri.starts_with("/skills") {
        let mut skill_map = std::collections::BTreeMap::new();
        let disabled_map: serde_json::Value = fs::read_to_string(&skills_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}));

        let skill_dirs = [
            (home_dir().join(".claude").join("skills"), "claude-code"),
            (home_dir().join(".prime").join("agent").join("skills"), "prime-agent"),
            (home_dir().join(".agents").join("skills"), "agents-core"),
        ];

        for (dir, src) in skill_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let enabled = disabled_map.get(&name).and_then(|v| v.as_bool()).unwrap_or(true);
                    skill_map.entry(name.clone()).or_insert_with(|| {
                        serde_json::json!({
                            "name": name,
                            "source": src,
                            "path": entry.path().to_string_lossy(),
                            "enabled": enabled
                        })
                    });
                }
            }
        }

        let skill_list: Vec<_> = skill_map.into_values().collect();
        let json = serde_json::json!({ "skills": skill_list });
        ("200 OK", "application/json", json.to_string())
    } else if uri.starts_with("/sessions/roots") {
        let list = match get_db() {
            Ok(db) => match db.list_all_roots() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("list_all_roots error: {}", e);
                    Vec::new()
                }
            },
            Err(e) => {
                eprintln!("get_db error: {}", e);
                Vec::new()
            }
        };
        ("200 OK", "application/json", serde_json::json!({ "roots": list }).to_string())
    } else if uri.starts_with("/sessions/tree") {
        let root_id = query_map.get("root_id").map(|s| s.as_str()).unwrap_or("root-default");
        let list = match get_db() {
            Ok(db) => match db.get_session_tree(root_id) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("get_session_tree error: {}", e);
                    Vec::new()
                }
            },
            Err(e) => {
                eprintln!("get_db error: {}", e);
                Vec::new()
            }
        };
        ("200 OK", "application/json", serde_json::json!({ "tree": list }).to_string())
    } else if uri.starts_with("/sessions/branch") && method == "POST" {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let item = SessionTreeItem {
                id: val["id"].as_str().unwrap_or("branch-new").to_string(),
                root_id: val["root_id"].as_str().unwrap_or("root-default").to_string(),
                parent_id: val["parent_id"].as_str().map(|s| s.to_string()),
                title: val["title"].as_str().unwrap_or("New Branch").to_string(),
                branch_label: val["branch_label"].as_str().unwrap_or("thread").to_string(),
                created_at: chrono_like_now(),
                turn_count: 0,
            };
            if let Ok(db) = get_db() {
                let _ = db.insert_session(&item);
            }
        }
        ("200 OK", "application/json", r#"{"status":"branched"}"#.to_string())
    } else if uri.starts_with("/sessions/delete") && method == "POST" {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(id) = val["id"].as_str() {
                if let Ok(db) = get_db() {
                    let _ = db.delete_session(id);
                }
            }
        }
        ("200 OK", "application/json", r#"{"status":"deleted"}"#.to_string())
    } else if uri.starts_with("/sessions/rename") && method == "POST" {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let (Some(id), Some(title)) = (val["id"].as_str(), val["title"].as_str()) {
                if let Ok(db) = get_db() {
                    let _ = db.update_session_title(id, title);
                }
            }
        }
        ("200 OK", "application/json", r#"{"status":"renamed"}"#.to_string())
    } else if uri.starts_with("/sessions/turns") {
        let session_id = query_map.get("session_id").map(|s| s.as_str()).unwrap_or("");
        let turns = match get_db() {
            Ok(db) => db.get_turns(session_id).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        ("200 OK", "application/json", serde_json::json!({ "turns": turns }).to_string())
    } else if uri.starts_with("/sessions/save_turn") && method == "POST" {
        println!("[SAVE_TURN_PAYLOAD] {}", payload);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let turn = ChatTurn {
                id: val["id"].as_str().unwrap_or("turn-new").to_string(),
                session_id: val["session_id"].as_str().unwrap_or("sess-1").to_string(),
                role: val["role"].as_str().unwrap_or("user").to_string(),
                content: val["content"].as_str().unwrap_or("").to_string(),
                author: val["author"].as_str().unwrap_or("USER").to_string(),
                created_at: chrono_like_now(),
            };
            if let Ok(db) = get_db() {
                if let Err(e) = db.save_turn(&turn) {
                    println!("[SAVE_TURN_ERROR] {:?}", e);
                } else {
                    println!("[SAVE_TURN_OK] {}", turn.id);
                }
                let _ = db.auto_refine(&turn.session_id, &turn.content);
            } else {
                println!("[SAVE_TURN_DB_ERROR] Failed to open DB");
            }
        }
        ("200 OK", "application/json", r#"{"status":"turn_saved"}"#.to_string())
    } else if uri.starts_with("/memory/reflections") {
        let list = match get_db() {
            Ok(db) => db.list_active_reflections().unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        ("200 OK", "application/json", serde_json::json!({ "reflections": list }).to_string())
    } else if uri.starts_with("/memory/reflect") && method == "POST" {
        let mut created = None;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let sess = val["session_id"].as_str().unwrap_or("global");
            let text = val["text"].as_str().unwrap_or("");
            if let Ok(db) = get_db() {
                if let Ok(opt) = db.auto_refine(sess, text) {
                    created = opt;
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "refined": created }).to_string())
    } else if uri.starts_with("/worktrees/create") && method == "POST" {
        let mut path_res = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(name) = val["name"].as_str() {
                let wt_dir = omw.join("worktrees").join(name);
                let _ = fs::create_dir_all(&wt_dir);
                path_res = wt_dir.to_string_lossy().to_string();
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "status": "created", "path": path_res }).to_string())
    } else if uri.starts_with("/worktrees") {
        let wt_parent = omw.join("worktrees");
        let _ = fs::create_dir_all(&wt_parent);
        let mut list = Vec::new();
        // default repo workspace
        list.push(serde_json::json!({
            "name": "Oh-My-Works (Main)",
            "path": std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| ".".to_string())
        }));
        if let Ok(rd) = fs::read_dir(wt_parent) {
            for entry in rd.flatten() {
                if entry.path().is_dir() {
                    list.push(serde_json::json!({
                        "name": entry.file_name().to_string_lossy(),
                        "path": entry.path().to_string_lossy()
                    }));
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "worktrees": list }).to_string())
    } else if uri.starts_with("/workspace/files") {
        let target_path = query_map.get("path")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let tree = scan_dir_recursive(&target_path, 3);
        ("200 OK", "application/json", serde_json::json!({ "files": tree }).to_string())
    } else if uri.starts_with("/workspace/collab_context") {
        let target_path = query_map.get("path")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let ctx = detect_collab_context(&target_path);
        ("200 OK", "application/json", ctx.to_string())
    } else if uri.starts_with("/workspace/diff") && method == "POST" {
        let mut res = serde_json::json!({ "error": "Invalid payload" });
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let (Some(p), Some(proposed)) = (val["path"].as_str(), val["content"].as_str()) {
                let is_protected = is_sensitive_path(p);
                if is_protected {
                    res = serde_json::json!({
                        "path": p,
                        "is_protected": true,
                        "error": "Access denied: protected sensitive/encrypted file."
                    });
                } else {
                    let original = fs::read_to_string(p).unwrap_or_default();
                    let exists = Path::new(p).exists();
                    res = serde_json::json!({
                        "path": p,
                        "exists": exists,
                        "is_protected": false,
                        "original": original,
                        "proposed": proposed
                    });
                }
            }
        }
        ("200 OK", "application/json", res.to_string())
    } else if uri.starts_with("/workspace/file_read") && method == "POST" {
        let mut file_content = String::new();
        let mut is_protected = false;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(p) = val["path"].as_str() {
                if is_sensitive_path(p) {
                    is_protected = true;
                } else if let Ok(c) = fs::read_to_string(p) {
                    file_content = c;
                }
            }
        }
        if is_protected {
            ("403 Forbidden", "application/json", serde_json::json!({ "error": "Protected: encrypted or sensitive credential file cannot be read directly." }).to_string())
        } else {
            ("200 OK", "application/json", serde_json::json!({ "content": file_content }).to_string())
        }
    } else if uri.starts_with("/workspace/file_write") && method == "POST" {
        let mut ok = false;
        let mut err_msg = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let (Some(p), Some(content)) = (val["path"].as_str(), val["content"].as_str()) {
                if is_sensitive_path(p) {
                    err_msg = "Access denied: file is protected (contains sensitive/encrypted credentials or keys).".to_string();
                } else {
                    let pb = PathBuf::from(p);
                    if let Some(parent) = pb.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    ok = fs::write(&pb, content).is_ok();
                    if !ok {
                        err_msg = "Failed writing file to disk".to_string();
                    }
                }
            }
        }
        if !err_msg.is_empty() && !ok {
            ("403 Forbidden", "application/json", serde_json::json!({ "success": false, "error": err_msg }).to_string())
        } else {
            ("200 OK", "application/json", serde_json::json!({ "success": ok }).to_string())
        }
    } else if uri.starts_with("/workspace/repomap") {
        let target_path = query_map.get("path")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let max_chars = query_map.get("max_chars")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(3000);
        let map = omw_harness::RepoMap::build(&target_path, max_chars);
        ("200 OK", "application/json", serde_json::json!({ "repomap": map, "chars": map.len() }).to_string())
    } else if uri.starts_with("/workspace/search") {
        let q = query_map.get("q").map(|s| s.as_str()).unwrap_or("");
        let cwd = query_map.get("path")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let res = omw_harness::ToolEngine::execute("search", &serde_json::json!({ "query": q }), &cwd);
        ("200 OK", "application/json", serde_json::json!({ "query": q, "results": res.output, "error": res.error }).to_string())
    } else if uri.starts_with("/agent/tool_exec") && method == "POST" {
        let mut tool_res = omw_harness::ToolResult {
            success: false,
            output: String::new(),
            error: Some("Invalid payload".to_string()),
        };
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let tool = val["tool"].as_str().unwrap_or("");
            let args = val.get("args").unwrap_or(&serde_json::json!({})).clone();
            let cwd = val["cwd"].as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            tool_res = omw_harness::ToolEngine::execute(tool, &args, &cwd);
        }
        ("200 OK", "application/json", serde_json::json!({
            "success": tool_res.success,
            "output": tool_res.output,
            "error": tool_res.error
        }).to_string())
    } else if uri.starts_with("/agent/pipeline/verify") && method == "POST" {
        let mut passed = false;
        let mut output = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let cmd = val["test_command"].as_str().unwrap_or("cargo test");
            let cwd = val["cwd"].as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let (ok, out) = omw_harness::PipelineExecution::run_verification(cmd, &cwd);
            passed = ok;
            output = out;
        }
        ("200 OK", "application/json", serde_json::json!({ "passed": passed, "output": output }).to_string())
    } else if uri.starts_with("/agent/pipeline/commit") && method == "POST" {
        let mut success = false;
        let mut output = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let msg = val["message"].as_str().unwrap_or("chore: auto-commit by OMW agent");
            let cwd = val["cwd"].as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let (ok, out) = omw_harness::PipelineExecution::auto_commit(msg, &cwd);
            success = ok;
            output = out;
        }
        ("200 OK", "application/json", serde_json::json!({ "success": success, "output": output }).to_string())
    } else if uri.starts_with("/terminal/exec") && method == "POST" {
        let mut cmd_output = String::new();
        let mut exit_code = 0;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let cmd = val["command"].as_str().unwrap_or("ls -la");
            let cwd = val["cwd"].as_str().map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            let out = Command::new("bash")
                .arg("-c")
                .arg(cmd)
                .current_dir(cwd)
                .output();

            match out {
                Ok(o) => {
                    exit_code = o.status.code().unwrap_or(0);
                    let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                    let err_text = String::from_utf8_lossy(&o.stderr).to_string();
                    if !err_text.is_empty() {
                        text.push_str("\n[STDERR]\n");
                        text.push_str(&err_text);
                    }
                    cmd_output = text;
                }
                Err(e) => {
                    exit_code = 1;
                    cmd_output = format!("Process launch failure: {}", e);
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "output": cmd_output, "exit_code": exit_code }).to_string())
    } else if uri.starts_with("/jobs/spawn") && method == "POST" {
        let mut ok = false;
        let mut job_id = String::new();
        let mut pid = 0;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let cmd = val["command"].as_str().unwrap_or("sleep 1").to_string();
            let cwd = val["cwd"].as_str().map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            if let Ok(child) = Command::new("bash")
                .arg("-c")
                .arg(&cmd)
                .current_dir(cwd)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                pid = child.id();
                job_id = format!("job-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis());
                if let Ok(mut jobs) = BACKGROUND_JOBS.lock() {
                    jobs.push(BackgroundJob {
                        id: job_id.clone(),
                        pid,
                        command: cmd,
                        started_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        child,
                    });
                    ok = true;
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "success": ok, "job_id": job_id, "pid": pid }).to_string())
    } else if uri.starts_with("/jobs/list") {
        let mut res = Vec::new();
        if let Ok(mut jobs) = BACKGROUND_JOBS.lock() {
            for j in jobs.iter_mut() {
                let status = match j.child.try_wait() {
                    Ok(Some(status)) => format!("exit: {}", status.code().unwrap_or(0)),
                    Ok(None) => "running".to_string(),
                    Err(e) => format!("error: {}", e),
                };
                res.push(serde_json::json!({
                    "id": j.id,
                    "pid": j.pid,
                    "command": j.command,
                    "started_at": j.started_at,
                    "status": status,
                }));
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "jobs": res }).to_string())
    } else if uri.starts_with("/jobs/kill") && method == "POST" {
        let mut killed = false;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let target_id = val["id"].as_str().unwrap_or("");
            if let Ok(mut jobs) = BACKGROUND_JOBS.lock() {
                if let Some(pos) = jobs.iter().position(|j| j.id == target_id) {
                    let mut j = jobs.remove(pos);
                    let _ = j.child.kill();
                    let _ = j.child.wait();
                    killed = true;
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "success": killed }).to_string())
    } else if uri.starts_with("/system/profile") {
        ("200 OK", "application/json", detect_system_profile().to_string())
    } else if uri.starts_with("/app/rebuild") && method == "POST" {
        let out = Command::new("cargo")
            .args(&["build", "-p", "omw-desktop-app"])
            .current_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
            .output();
        let (success, log) = match out {
            Ok(o) => (o.status.success(), format!("{}
{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
            Err(e) => (false, format!("Failed to invoke cargo: {}", e)),
        };
        ("200 OK", "application/json", serde_json::json!({ "success": success, "log": log }).to_string())
    } else if uri.starts_with("/agent/inbox") {
        let inbox_file = omw.join("agent_inbox.json");
        if method == "POST" {
            let mut prompt_val = String::new();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(p) = v["prompt"].as_str() {
                    prompt_val = p.to_string();
                }
            }
            if prompt_val.is_empty() && !payload.is_empty() {
                prompt_val = payload.to_string();
            }
            let _ = fs::write(&inbox_file, serde_json::json!({ "prompt": prompt_val }).to_string());
            ("200 OK", "application/json", serde_json::json!({ "status": "queued", "prompt": prompt_val }).to_string())
        } else {
            let consume = query_map.get("consume").map(|s| s.as_str()) == Some("true");
            let res = if let Ok(content) = fs::read_to_string(&inbox_file) {
                if consume {
                    let _ = fs::remove_file(&inbox_file);
                }
                content
            } else {
                r#"{"prompt":null}"#.to_string()
            };
            ("200 OK", "application/json", res)
        }
    } else if uri.starts_with("/agent/action") {
        let action_file = omw.join("agent_action.json");
        if method == "POST" {
            let _ = fs::write(&action_file, payload);
            ("200 OK", "application/json", serde_json::json!({ "status": "queued" }).to_string())
        } else {
            let consume = query_map.get("consume").map(|s| s.as_str()) == Some("true");
            let res = if let Ok(content) = fs::read_to_string(&action_file) {
                if consume {
                    let _ = fs::remove_file(&action_file);
                }
                content
            } else {
                r#"{"action":null}"#.to_string()
            };
            ("200 OK", "application/json", res)
        }
    } else if uri.starts_with("/agent/state") {
        let state_file = omw.join("agent_state.json");
        if method == "POST" {
            let _ = fs::write(&state_file, payload);
            ("200 OK", "application/json", serde_json::json!({ "status": "saved" }).to_string())
        } else {
            let res = fs::read_to_string(&state_file).unwrap_or_else(|_| r#"{"state":"offline"}"#.to_string());
            ("200 OK", "application/json", res)
        }
    } else if uri.starts_with("/harness/observer") {
        let observer_data = check_harness_observer();
        ("200 OK", "application/json", observer_data.to_string())
    } else if uri.starts_with("/settings") {
        let settings_path = omw.join("config.json");
        if method == "POST" {
            let _ = fs::write(&settings_path, payload);
            ("200 OK", "application/json", r#"{"status":"synced"}"#.to_string())
        } else {
            let content = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
            ("200 OK", "application/json", content)
        }
    } else if uri.starts_with("/sessions") {
        let sessions = SessionHarvester::scan_all(&home_dir());
        let s = serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string());
        ("200 OK", "application/json", s)
    } else if uri == "/dashboard" || uri == "/panel" || (uri == "/" && req_header_str.to_ascii_lowercase().contains("accept: text/html")) {
        ("200 OK", "text/html; charset=utf-8", include_str!("panel.html").to_string())
    } else if uri.starts_with("/deploy/list") {
        let mut list = Vec::new();
        if let Ok(mut projects) = DEPLOYED_PROJECTS.lock() {
            for p in projects.iter_mut() {
                let alive = is_process_alive(p.pid);
                if !alive && p.status == "running" {
                    p.status = "stopped".to_string();
                }
                let rss = if alive { get_process_rss_mb(p.pid) } else { 0.0 };
                list.push(serde_json::json!({
                    "name": p.name,
                    "repo_url": p.repo_url,
                    "port": p.port,
                    "pid": p.pid,
                    "command": p.command,
                    "status": p.status,
                    "rss_mb": rss,
                    "started_at": p.started_at,
                }));
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "projects": list }).to_string())
    } else if uri.starts_with("/deploy/spawn") && method == "POST" {
        if let Ok((HealthLevel::Critical, _)) = Guardian::assess() {
            let err_json = serde_json::json!({
                "success": false,
                "error": "Anti-OOM Guardian: Host memory pressure CRITICAL. Deployment aborted to prevent crash."
            });
            ("200 OK", "application/json", err_json.to_string())
        } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let name = val["name"].as_str().unwrap_or("project").to_string();
            let repo = val["repo_url"].as_str().unwrap_or("").to_string();
            let build = val["build_cmd"].as_str().unwrap_or("").to_string();
            let start = val["start_cmd"].as_str().unwrap_or("").to_string();
            let port = val["port"].as_u64().unwrap_or(0) as u16;

            let proj_dir = omw.join("projects").join(&name);
            let _ = fs::create_dir_all(&proj_dir);
            let log_file = proj_dir.join("app.log");

            if !repo.is_empty() {
                if !proj_dir.join(".git").exists() {
                    let _ = Command::new("git")
                        .args(["clone", &repo, "."])
                        .current_dir(&proj_dir)
                        .output();
                } else {
                    let _ = Command::new("git")
                        .args(["pull"])
                        .current_dir(&proj_dir)
                        .output();
                }
            }

            if !build.is_empty() {
                let _ = Command::new("bash")
                    .arg("-c")
                    .arg(&build)
                    .current_dir(&proj_dir)
                    .output();
            }

            let out_file = fs::File::create(&log_file).unwrap_or_else(|_| fs::File::create("/dev/null").unwrap());
            let err_file = out_file.try_clone().unwrap_or_else(|_| fs::File::create("/dev/null").unwrap());

            match Command::new("bash")
                .arg("-c")
                .arg(&start)
                .current_dir(&proj_dir)
                .stdout(out_file)
                .stderr(err_file)
                .spawn()
            {
                Ok(child) => {
                    let pid = child.id();
                    if let Ok(mut projs) = DEPLOYED_PROJECTS.lock() {
                        projs.retain(|p| p.name != name);
                        projs.push(DeployedProject {
                            name: name.clone(),
                            repo_url: repo,
                            port,
                            pid,
                            command: start,
                            status: "running".to_string(),
                            log_path: log_file,
                            started_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        });
                    }
                    ("200 OK", "application/json", serde_json::json!({ "success": true, "pid": pid, "name": name }).to_string())
                }
                Err(e) => {
                    ("200 OK", "application/json", serde_json::json!({ "success": false, "error": e.to_string() }).to_string())
                }
            }
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "success": false, "error": "Invalid payload" }).to_string())
        }
    } else if uri.starts_with("/deploy/kill") && method == "POST" {
        let mut ok = false;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let pid = val["pid"].as_u64().unwrap_or(0) as u32;
            let name = val["name"].as_str().unwrap_or("");
            if pid > 0 {
                let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
                if let Ok(mut projs) = DEPLOYED_PROJECTS.lock() {
                    for p in projs.iter_mut() {
                        if p.pid == pid || p.name == name {
                            p.status = "stopped".to_string();
                            ok = true;
                        }
                    }
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "success": ok }).to_string())
    } else if uri.starts_with("/deploy/logs") {
        let name = query_map.get("name").map(|s| s.as_str()).unwrap_or("");
        let mut logs = String::new();
        if let Ok(projs) = DEPLOYED_PROJECTS.lock() {
            if let Some(p) = projs.iter().find(|p| p.name == name) {
                if let Ok(c) = fs::read_to_string(&p.log_path) {
                    let lines: Vec<&str> = c.lines().collect();
                    let start = lines.len().saturating_sub(100);
                    logs = lines[start..].join("\n");
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "name": name, "logs": logs }).to_string())
        } else if uri.starts_with("/sql/databases") {
        let mut dbs = Vec::new();
        let cg_db = omw.join("codegraph.db");
        if cg_db.exists() {
            let sz = fs::metadata(&cg_db).map(|m| m.len() / 1024).unwrap_or(0);
            dbs.push(serde_json::json!({
                "name": "codegraph.db (OMW Core)",
                "path": cg_db.to_string_lossy(),
                "size_kb": sz,
                "is_default": true
            }));
        }
        if let Ok(entries) = fs::read_dir(&omw) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p != cg_db && (p.extension().map_or(false, |ext| ext == "db" || ext == "sqlite" || ext == "sqlite3")) {
                    let sz = fs::metadata(&p).map(|m| m.len() / 1024).unwrap_or(0);
                    let fname = p.file_name().unwrap_or_default().to_string_lossy();
                    dbs.push(serde_json::json!({
                        "name": fname,
                        "path": p.to_string_lossy(),
                        "size_kb": sz,
                        "is_default": false
                    }));
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "databases": dbs }).to_string())
    } else if uri.starts_with("/sql/tables") {
        let db_path_str = if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            val["db_path"].as_str().unwrap_or("").to_string()
        } else {
            String::new()
        };
        let db_path = if !db_path_str.is_empty() {
            PathBuf::from(db_path_str)
        } else {
            omw.join("codegraph.db")
        };

        let mut tables = Vec::new();
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            if let Ok(mut stmt) = conn.prepare("SELECT name, type, sql FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name") {
                let rows_res = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?.unwrap_or_default()
                    ))
                });
                if let Ok(rows) = rows_res {
                    for r in rows.flatten() {
                        let count_query = format!("SELECT count(*) FROM \"{}\"", r.0.replace('"', "\"\""));
                        let row_count: i64 = conn.query_row(&count_query, [], |row| row.get(0)).unwrap_or(0);
                        tables.push(serde_json::json!({
                            "name": r.0,
                            "type": r.1,
                            "sql": r.2,
                            "row_count": row_count
                        }));
                    }
                }
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "tables": tables, "db_path": db_path.to_string_lossy() }).to_string())
    } else if uri.starts_with("/sql/query") && method == "POST" {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let db_path_str = val["db_path"].as_str().unwrap_or("");
            let query = val["query"].as_str().unwrap_or("");
            let db_path = if !db_path_str.is_empty() {
                PathBuf::from(db_path_str)
            } else {
                omw.join("codegraph.db")
            };
            let res = execute_sql_query(&db_path, query);
            ("200 OK", "application/json", res.to_string())
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid payload" }).to_string())
        }
    } else if uri.starts_with("/websites/list") {
        let sites_file = omw.join("websites.json");
        let mut list = load_websites_list(&sites_file);
        for item in list.iter_mut() {
            if let Some(obj) = item.as_object_mut() {
                let pid = obj.get("pid").and_then(|v| v.as_u64()).map(|n| n as u32);
                let is_alive = pid.map_or(false, is_process_alive);
                if !is_alive && obj.get("status").and_then(|s| s.as_str()) == Some("running") {
                    obj.insert("status".to_string(), serde_json::json!("stopped"));
                }
                let rss = pid.map_or(0.0, |p| if is_alive { get_process_rss_mb(p) } else { 0.0 });
                obj.insert("rss_mb".to_string(), serde_json::json!(rss));
            }
        }
        save_websites_list(&sites_file, &list);
        ("200 OK", "application/json", serde_json::json!({ "websites": list }).to_string())
    } else if uri.starts_with("/websites/create") && method == "POST" {
        let sites_file = omw.join("websites.json");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let name = val["name"].as_str().unwrap_or("New Website").to_string();
            let domain = val["domain"].as_str().unwrap_or("localhost").to_string();
            let site_type = val["site_type"].as_str().unwrap_or("proxy").to_string();
            let port = val["port"].as_u64().unwrap_or(8080) as u16;
            let root_dir = val["root_dir"].as_str().unwrap_or("").to_string();
            let start_cmd = val["start_cmd"].as_str().unwrap_or("").to_string();
            let id = format!("site-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis());

            let mut pid = None;
            let mut status = "stopped".to_string();

            if !start_cmd.is_empty() {
                let mut cmd = Command::new("bash");
                cmd.arg("-c").arg(&start_cmd);
                if !root_dir.is_empty() {
                    cmd.current_dir(&root_dir);
                }
                if let Ok(child) = cmd.spawn() {
                    pid = Some(child.id());
                    status = "running".to_string();
                }
            }

            let new_site = serde_json::json!({
                "id": id,
                "name": name,
                "domain": domain,
                "site_type": site_type,
                "port": port,
                "root_dir": root_dir,
                "start_cmd": start_cmd,
                "status": status,
                "pid": pid,
                "created_at": chrono_like_now()
            });

            let mut list = load_websites_list(&sites_file);
            list.push(new_site.clone());
            save_websites_list(&sites_file, &list);

            ("200 OK", "application/json", serde_json::json!({ "success": true, "website": new_site }).to_string())
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid payload" }).to_string())
        }
    } else if uri.starts_with("/websites/delete") && method == "POST" {
        let sites_file = omw.join("websites.json");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let id = val["id"].as_str().unwrap_or("");
            let mut list = load_websites_list(&sites_file);
            let mut removed = false;
            list.retain(|s| {
                if s["id"].as_str().unwrap_or("") == id {
                    if let Some(pid) = s["pid"].as_u64().map(|p| p as u32) {
                        let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
                    }
                    removed = true;
                    false
                } else {
                    true
                }
            });
            save_websites_list(&sites_file, &list);
            ("200 OK", "application/json", serde_json::json!({ "success": removed }).to_string())
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid payload" }).to_string())
        }
    } else if uri.starts_with("/websites/toggle") && method == "POST" {
        let sites_file = omw.join("websites.json");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let id = val["id"].as_str().unwrap_or("");
            let mut list = load_websites_list(&sites_file);
            let mut updated = false;
            for s in list.iter_mut() {
                if s["id"].as_str().unwrap_or("") == id {
                    let cur_status = s["status"].as_str().unwrap_or("stopped");
                    if cur_status == "running" {
                        if let Some(pid) = s["pid"].as_u64().map(|p| p as u32) {
                            let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
                        }
                        s["status"] = serde_json::json!("stopped");
                        s["pid"] = serde_json::Value::Null;
                    } else {
                        let start_cmd = s["start_cmd"].as_str().unwrap_or("");
                        let root_dir = s["root_dir"].as_str().unwrap_or("");
                        if !start_cmd.is_empty() {
                            let mut cmd = Command::new("bash");
                            cmd.arg("-c").arg(start_cmd);
                            if !root_dir.is_empty() {
                                cmd.current_dir(root_dir);
                            }
                            if let Ok(child) = cmd.spawn() {
                                s["pid"] = serde_json::json!(child.id());
                                s["status"] = serde_json::json!("running");
                            }
                        } else {
                            s["status"] = serde_json::json!("running");
                        }
                    }
                    updated = true;
                    break;
                }
            }
            if updated {
                save_websites_list(&sites_file, &list);
            }
            ("200 OK", "application/json", serde_json::json!({ "success": updated }).to_string())
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid payload" }).to_string())
        }
    } else if uri.starts_with("/omniroute/status") {
        let omni_file = omw.join("omniroute.json");
        let cfg = load_omniroute_config(&omni_file);
        let providers_cnt = cfg.get("providers").and_then(|p| p.as_array()).map_or(0, |a| a.len());
        let aliases_cnt = cfg.get("aliases").and_then(|p| p.as_object()).map_or(0, |o| o.len());
        let status = serde_json::json!({
            "active_router": "OmniRoute Native Rust Hub",
            "proxy_endpoint": "http://127.0.0.1:9180/v1/chat/completions",
            "lead_model": "ag/gemini-3.8-flash-high",
            "coder_model": "mux/glm-5.3-flash",
            "verifier_model": "cbcn/deepseek-v4-pro",
            "total_providers": providers_cnt,
            "total_aliases": aliases_cnt,
            "architecture": "OmniRoute Unified Hub (9Router/OmniRouter Parity)",
            "health": "healthy",
            "uptime_seconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        });
        ("200 OK", "application/json", status.to_string())
    } else if uri.starts_with("/omniroute/config") {
        let omni_file = omw.join("omniroute.json");
        if method == "POST" {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
                save_omniroute_config(&omni_file, &val);
                ("200 OK", "application/json", serde_json::json!({ "success": true, "message": "OmniRoute configuration saved" }).to_string())
            } else {
                ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid JSON" }).to_string())
            }
        } else {
            let cfg = load_omniroute_config(&omni_file);
            ("200 OK", "application/json", cfg.to_string())
        }
    } else if uri.starts_with("/omniroute/backup") {
        let omni_file = omw.join("omniroute.json");
        let cfg = load_omniroute_config(&omni_file);
        ("200 OK", "application/json", cfg.to_string())
    } else if uri.starts_with("/omniroute/restore") && method == "POST" {
        let omni_file = omw.join("omniroute.json");
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            save_omniroute_config(&omni_file, &val);
            let providers_cnt = val.get("providers").and_then(|p| p.as_array()).map_or(0, |a| a.len());
            ("200 OK", "application/json", serde_json::json!({ "success": true, "status": "RESTORED", "providers": providers_cnt }).to_string())
        } else {
            ("400 Bad Request", "application/json", serde_json::json!({ "error": "Invalid backup JSON format" }).to_string())
        }
    } else if uri.starts_with("/omniroute/test") && method == "POST" {
        let mut ok = false;
        let mut latency_ms = 0u64;
        let mut err_msg = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            let base_url = val["base_url"].as_str().unwrap_or("https://api.openai.com/v1");
            let api_key = val["api_key"].as_str().unwrap_or("");
            let test_url = format!("{}/models", base_url.trim_end_matches('/'));
            let t0 = std::time::Instant::now();
            let mut cmd = Command::new("curl");
            cmd.args(["-s", "-m", "5", "-X", "GET", &test_url]);
            if !api_key.is_empty() {
                cmd.args(["-H", &format!("Authorization: Bearer {}", api_key)]);
            }
            if let Ok(out) = cmd.output() {
                latency_ms = t0.elapsed().as_millis() as u64;
                if out.status.success() {
                    ok = true;
                } else {
                    err_msg = format!("HTTP {}", out.status.code().unwrap_or(0));
                }
            } else {
                err_msg = "Failed to run curl".to_string();
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "ok": ok, "latency_ms": latency_ms, "error": err_msg }).to_string())
} else if uri.starts_with("/api/keys") {
        let keys_file = omw.join("keys.json");
        if method == "POST" {
            let _ = fs::write(&keys_file, payload);
            ("200 OK", "application/json", serde_json::json!({ "success": true }).to_string())
        } else {
            let content = fs::read_to_string(&keys_file).unwrap_or_else(|_| "{}".to_string());
            ("200 OK", "application/json", content)
        }
    } else if uri.starts_with("/v1/models") || uri.starts_with("/api/models") {
        let models = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "ag/gemini-3.8-flash-high", "object": "model", "owned_by": "omw" },
                { "id": "mux/glm-5.3-flash", "object": "model", "owned_by": "omw" },
                { "id": "cbcn/deepseek-v4-pro", "object": "model", "owned_by": "omw" },
                { "id": "anthropic/claude-3-5-sonnet", "object": "model", "owned_by": "omw" },
                { "id": "meta-llama/llama-3.3-70b", "object": "model", "owned_by": "omw" },
                { "id": "openrouter/auto", "object": "model", "owned_by": "omw" }
            ]
        });
        ("200 OK", "application/json", models.to_string())
    } else if uri.starts_with("/v1/chat/completions") && method == "POST" {
        let target_router = "http://127.0.0.1:20128/api/v1/chat/completions";
        let mut forward_res = serde_json::json!({ "error": "Router bridge inactive" }).to_string();
        if let Ok(output) = Command::new("curl")
            .args(["-s", "-X", "POST", target_router, "-H", "Content-Type: application/json", "-d", payload])
            .output()
        {
            forward_res = String::from_utf8_lossy(&output.stdout).to_string();
        }
        ("200 OK", "application/json", forward_res)
    } else {
        let (status_str, pressure) = match Guardian::assess() {
            Ok((level, p)) => {
                let l_str = match level {
                    HealthLevel::Safe => "SAFE",
                    HealthLevel::Warning => "WARNING",
                    HealthLevel::Critical => "CRITICAL",
                };
                (l_str, p)
            }
            Err(_) => ("UNKNOWN", omw_core::MemoryPressure::default()),
        };

        let body = format!(
            r#"{{"status":"{}","available_percent":{:.2},"some_avg10":{:.2},"full_avg10":{:.2}}}"#,
            status_str,
            pressure.available_percent(),
            pressure.some_avg10,
            pressure.full_avg10
        );
        ("200 OK", "application/json", body)
    };

    let response = format!(
        "HTTP/1.1 {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache, no-store, must-revalidate\r\nPragma: no-cache\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        content_type,
        body.len(),
        body
    );

    let _ = stream.write_all(response.as_bytes());
}

fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now();
    let d = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    // Simple readable ISO-like UTC timestamp without external chrono crate
    let _days = secs / 86400;
    let rem_secs = secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;
    format!("T{:02}:{:02}:{:02}Z", hours, minutes, seconds)
}

fn main() {
    let port = std::env::var("OMW_DAEMON_PORT").unwrap_or_else(|_| "9180".to_string());
    let host = std::env::var("OMW_DAEMON_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let addr = format!("{}:{}", host, port);

    println!("OMW End-to-End Server Daemon listening on http://{}", addr);

    if let Ok(listener) = TcpListener::bind(&addr) {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));
            std::thread::spawn(move || {
                handle_client(stream);
            });
        }
    } else {
        eprintln!("Failed to bind daemon to {}", addr);
    }
}
