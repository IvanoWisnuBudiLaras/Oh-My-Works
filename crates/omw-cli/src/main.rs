use std::env;
use omw_guardian::{Guardian, HealthLevel};
use omw_terminal::TerminalMultiplexer;

fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("status");

    match cmd {
        "status" => match Guardian::assess() {
            Ok((level, pressure)) => {
                let badge = match level {
                    HealthLevel::Safe => "[OK - SAFE]",
                    HealthLevel::Warning => "[WARN - PRESSURE ELEVATED]",
                    HealthLevel::Critical => "[CRITICAL - OOM IMMINENT]",
                };
                println!("OMW Guardian Status: {}", badge);
                println!(
                    "Memory: {:.1}% available ({:.1} MB / {:.1} MB)",
                    pressure.available_percent(),
                    pressure.available_mem_kb as f64 / 1024.0,
                    pressure.total_mem_kb as f64 / 1024.0
                );
                println!(
                    "PSI Stall: some avg10={:.2}%, full avg10={:.2}%",
                    pressure.some_avg10, pressure.full_avg10
                );
            }
            Err(e) => eprintln!("Error reading system metrics: {}", e),
        },
        "terminal" => {
            let mut mux = TerminalMultiplexer::new();
            mux.spawn_session("pty-1", "Main Shell", "/workspace");
            mux.spawn_session("pty-2", "Meta-Harness Agent", "/workspace");
            println!("Active Terminal Sessions: {}", mux.sessions.len());
            for (idx, sess) in mux.sessions.iter().enumerate() {
                println!("  [{}] {} ({}) - {:?}", idx, sess.title, sess.id, sess.status);
            }
        }
        "version" | "--version" | "-v" => {
            println!("Oh My Works (OMW) CLI v0.1.0");
        }
        other => {
            eprintln!("Unknown command: '{}'. Available: status, terminal, version", other);
            std::process::exit(1);
        }
    }
}
