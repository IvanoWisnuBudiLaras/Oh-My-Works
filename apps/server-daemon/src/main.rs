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
