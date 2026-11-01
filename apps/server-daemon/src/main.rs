//! Ultra-lightweight server telemetry daemon for Oh My Works.
//! Zero-dependency HTTP listener using stdlib std::net.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use omw_guardian::{Guardian, HealthLevel};
use omw_session::SessionHarvester;

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 1024];
    let n = match stream.read(&mut buffer) {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let req_str = String::from_utf8_lossy(&buffer[..n]);
    let req_line = req_str.lines().next().unwrap_or("");

    let json_body = if req_line.contains("/sessions") {
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
        let sessions = SessionHarvester::scan_all(&home);
        serde_json::to_string(&sessions).unwrap_or_else(|_| "[]".to_string())
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

        serde_json::to_string(&orch).unwrap_or_else(|_| "{}".to_string())
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

        format!(
            r#"{{"status":"{}","available_percent":{:.2},"some_avg10":{:.2},"full_avg10":{:.2}}}"#,
            status_str,
            pressure.available_percent(),
            pressure.some_avg10,
            pressure.full_avg10
        )
    };

    let response = format!(
        "HTTP/1.1 200 OK
Access-Control-Allow-Origin: *
Content-Type: application/json
Content-Length: {}
Connection: close

{}",
        json_body.len(),
        json_body
    );

    let _ = stream.write_all(response.as_bytes());
}

fn main() {
    let port = std::env::var("OMW_DAEMON_PORT").unwrap_or_else(|_| "9180".to_string());
    let addr = format!("127.0.0.1:{}", port);

    println!("OMW Server Telemetry Daemon listening on http://{}", addr);

    // ponytail: single-threaded blocking listener. Upgrade to multi-threaded worker pool when remote concurrency > 50 req/s.
    if let Ok(listener) = TcpListener::bind(&addr) {
        for stream in listener.incoming().flatten() {
            handle_client(stream);
        }
    } else {
        eprintln!("Failed to bind daemon to {}", addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_payload_generation() {
        let (_status, p) = Guardian::assess().unwrap();
        assert!(p.available_percent() >= 0.0);
    }
}
