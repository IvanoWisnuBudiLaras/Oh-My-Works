//! Ultra-lightweight server telemetry daemon for Oh My Works.
//! Zero-dependency HTTP listener using stdlib std::net.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use omw_guardian::{Guardian, HealthLevel};

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 512];
    if stream.read(&mut buffer).is_err() {
        return;
    }

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

    let json_body = format!(
        r#"{{"status":"{}","available_percent":{:.2},"some_avg10":{:.2},"full_avg10":{:.2}}}"#,
        status_str,
        pressure.available_percent(),
        pressure.some_avg10,
        pressure.full_avg10
    );

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                handle_client(stream);
            }
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
