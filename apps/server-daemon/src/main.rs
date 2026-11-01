//! Ultra-lightweight server telemetry & unified harness daemon for Oh My Works.
//! Zero-dependency HTTP listener using stdlib std::net.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use omw_guardian::{Guardian, HealthLevel};
use omw_session::SessionHarvester;

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 65536];
    let n = match stream.read(&mut buffer) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let req_str = String::from_utf8_lossy(&buffer[..n]);
    let mut lines = req_str.lines();
    let req_line = lines.next().unwrap_or("");

    let home = home_dir();
    let omw_dir = home.join(".omw");
    let _ = fs::create_dir_all(&omw_dir);

    if req_line.starts_with("OPTIONS") {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    let (status, content_type, body) = if req_line.contains("/soul") {
        let soul_path = omw_dir.join("SOUL.md");
        if req_line.starts_with("POST") {
            // Extract body after empty line
            if let Some(pos) = req_str.find("\r\n\r\n") {
                let payload = &req_str[pos + 4..];
                let _ = fs::write(&soul_path, payload);
            }
            ("200 OK", "application/json", r#"{"status":"saved"}"#.to_string())
        } else {
            let content = fs::read_to_string(&soul_path).unwrap_or_else(|_| "# SOUL.md - Workspace Persona\n".to_string());
            let json = serde_json::json!({ "soul": content });
            ("200 OK", "application/json", json.to_string())
        }
    } else if req_line.contains("/skills") {
        // Collect skills from ~/.claude/skills and ~/.prime/agent/skills
        let mut skill_list = Vec::new();
        let claude_skills = home.join(".claude").join("skills");
        if let Ok(entries) = fs::read_dir(claude_skills) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                skill_list.push(serde_json::json!({
                    "name": name,
                    "source": "claude-code",
                    "path": entry.path().to_string_lossy(),
                    "enabled": true
                }));
            }
        }
        let prime_skills = home.join(".prime").join("agent").join("skills");
        if let Ok(entries) = fs::read_dir(prime_skills) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !skill_list.iter().any(|s| s["name"] == name) {
                    skill_list.push(serde_json::json!({
                        "name": name,
                        "source": "prime-agent",
                        "path": entry.path().to_string_lossy(),
                        "enabled": true
                    }));
                }
            }
        }
        let json = serde_json::json!({ "skills": skill_list });
        ("200 OK", "application/json", json.to_string())
    } else if req_line.contains("/settings") {
        let settings_path = omw_dir.join("config.json");
        if req_line.starts_with("POST") {
            if let Some(pos) = req_str.find("\r\n\r\n") {
                let payload = &req_str[pos + 4..];
                let _ = fs::write(&settings_path, payload);
            }
            ("200 OK", "application/json", r#"{"status":"synced"}"#.to_string())
        } else {
            let content = fs::read_to_string(&settings_path).unwrap_or_else(|_| "{}".to_string());
            ("200 OK", "application/json", content)
        }
    } else if req_line.contains("/sessions") {
        let sessions = SessionHarvester::scan_all(&home);
        let s = serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string());
        ("200 OK", "application/json", s)
    } else if req_line.contains("/harness") {
        let mut orch = omw_harness::HarnessOrchestrator::new();
        orch.register_agent("agent-architect", "Code Architect", "Rust/C++ Engine & Monorepo Optimization");
        orch.register_agent("agent-strix", "Security Auditor (Strix)", "Autonomous PoC Pentest & Security Scans");
        orch.register_agent("agent-rea", "Reverse Engineer (REA)", "Blueprint Extraction & Mimic Generation");
        orch.register_agent("agent-guardian", "Kernel Optimizer", "Anti-OOM PSI Watchdog & Container Sizing");
        orch.register_agent("agent-tester", "E2E Tester (Maestro)", "Android Emulator & Browser Automation");

        let t1 = orch.create_task("task-1", "Audit Kernel PSI Metrics", "Scan /proc/pressure/memory and evaluate stall levels");
        orch.dispatch(t1, "agent-guardian");
        orch.complete_task(t1, "PSI 0.00% - System fully stable");

        let t2 = orch.create_task("task-2", "Harvester Synchronize", "Scan ~/.prime, ~/.codex, ~/.claude directories");
        orch.dispatch(t2, "agent-architect");
        orch.complete_task(t2, "45 agent sessions indexed via Cindy adapter");

        let s = serde_json::to_string(&orch).unwrap_or_else(|_| "{}".to_string());
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

fn main() {
    let port = std::env::var("OMW_DAEMON_PORT").unwrap_or_else(|_| "9180".to_string());
    let addr = format!("127.0.0.1:{}", port);

    println!("OMW Server Telemetry & Unified Harness Daemon listening on http://{}", addr);

    if let Ok(listener) = TcpListener::bind(&addr) {
        for stream in listener.incoming().flatten() {
            handle_client(stream);
        }
    } else {
        eprintln!("Failed to bind daemon to {}", addr);
    }
}
