//! Comprehensive server telemetry, CodeGraph memory, worktree & unified harness daemon for Oh My Works.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use omw_guardian::{Guardian, HealthLevel};
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

fn parse_query(uri: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(pos) = uri.find('?') {
        let q = &uri[pos + 1..];
        for pair in q.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                map.insert(k.to_string(), v.to_string());
            }
        }
    }
    map
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
    } else if uri.starts_with("/workspace/file_read") && method == "POST" {
        let mut file_content = String::new();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(p) = val["path"].as_str() {
                file_content = fs::read_to_string(p).unwrap_or_else(|e| format!("Error reading file: {}", e));
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "content": file_content }).to_string())
    } else if uri.starts_with("/workspace/file_write") && method == "POST" {
        let mut ok = false;
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) {
            if let (Some(p), Some(content)) = (val["path"].as_str(), val["content"].as_str()) {
                let pb = PathBuf::from(p);
                if let Some(parent) = pb.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                ok = fs::write(&pb, content).is_ok();
            }
        }
        ("200 OK", "application/json", serde_json::json!({ "success": ok }).to_string())
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
        "HTTP/1.1 {}\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    let addr = format!("127.0.0.1:{}", port);

    println!("OMW End-to-End Server Daemon listening on http://{}", addr);

    if let Ok(listener) = TcpListener::bind(&addr) {
        for stream in listener.incoming().flatten() {
            handle_client(stream);
        }
    } else {
        eprintln!("Failed to bind daemon to {}", addr);
    }
}
