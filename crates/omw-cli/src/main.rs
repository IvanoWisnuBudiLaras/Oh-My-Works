use std::env;
use std::path::PathBuf;
use omw_guardian::{Guardian, HealthLevel};
use omw_terminal::TerminalMultiplexer;
use omw_harness::HarnessOrchestrator;
use omw_memory::{GraphMemory, MemoryLayer};
use omw_sync::WorkspaceSync;
use omw_clone::CloneInspector;
use omw_browser::{BrowserRunner, BrowserEngine};
use omw_emulator::EmulatorManager;
use omw_session::SessionHarvester;

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
            mux.spawn_session("pty-1", "Main Shell", ".");
            mux.spawn_session("pty-2", "Meta-Harness Agent", ".");
            println!("Active Terminal Sessions: {}", mux.sessions.len());
            for (idx, sess) in mux.sessions.iter().enumerate() {
                println!("  [{}] {} ({}) - {:?}", idx, sess.title, sess.id, sess.status);
            }
        }
        "exec" => {
            let program = args.get(2).map(|s| s.as_str()).unwrap_or("git");
            let subargs: Vec<&str> = args.iter().skip(3).map(|s| s.as_str()).collect();
            let mut mux = TerminalMultiplexer::new();
            mux.spawn_session("cli-exec", "CLI Runner", ".");
            match mux.execute("cli-exec", program, &subargs) {
                Ok(output) => print!("{}", output),
                Err(err) => eprintln!("Execution error: {}", err),
            }
        }
        "harness" => {
            let mut orch = HarnessOrchestrator::new();
            orch.register_agent("sub-1", "Code Analyst", "Rust/C++ Code Review");
            let t = orch.create_task("t1", "Audit Kernel PSI", "Check /proc/pressure");
            orch.dispatch(t, "sub-1");
            println!("Meta-AI Harness initialized:");
            for agent in &orch.agents {
                println!("  Agent [{}] {} (Role: {}) -> Status: {:?}", agent.id, agent.name, agent.role, agent.status);
            }
        }
        "memory" => {
            let mut mem = GraphMemory::new();
            mem.insert_node("core", MemoryLayer::Semantic, "Architecture", "Rust Monorepo");
            mem.insert_node("guard", MemoryLayer::Procedural, "PSI Monitor", "Zero-alloc kernel watcher");
            mem.link("core", "guard", "protected_by");
            println!("Graph Memory nodes: {}, edges: {}", mem.nodes.len(), mem.edges.len());
            for edge in &mem.edges {
                println!("  ({}: {}) --[{}]--> ({})", edge.from, mem.nodes[0].title, edge.relation, edge.to);
            }
        }
        "sync" => match WorkspaceSync::check_status(".") {
            Ok(status) => {
                println!("GitHub Sync Status (Branch: {}):", status.branch);
                if status.is_clean {
                    println!("  Working directory clean. Everything backed up.");
                } else {
                    println!("  {} uncommitted changes detected.", status.uncommitted_files);
                    if args.get(2).map(|s| s.as_str()) == Some("--backup") {
                        let msg = args.get(3).map(|s| s.as_str()).unwrap_or("chore: automated OMW workspace backup");
                        match WorkspaceSync::auto_backup(".", msg) {
                            Ok(res) => println!("  Backup successful: {}", res),
                            Err(e) => eprintln!("  Backup failed: {}", e),
                        }
                    } else {
                        println!("  Run 'cargo run -p omw-cli -- sync --backup' to commit snapshot.");
                    }
                }
            }
            Err(e) => eprintln!("Error checking sync status: {}", e),
        },
        "clone" => {
            let target = args.get(2).map(|s| s.as_str()).unwrap_or("https://example.com (React + Tauri)");
            let profile = CloneInspector::inspect_target(target);
            println!("Target Analysis: {}", profile.target);
            println!("  Detected Stack: {}", profile.detected_stack.join(", "));

            let blueprints = CloneInspector::generate_blueprints(&profile);
            println!("  Generated Blueprints (REA Mimic): {}", blueprints.len());
            for b in &blueprints {
                println!("    - [{}] {}: {}", b.artifact_type, b.component_name, b.blueprint_snippet);
            }

            let findings = CloneInspector::run_security_scan(target);
            println!("  Security Vulnerability Scan (Strix Pentest): {} findings", findings.len());
            for f in &findings {
                println!("    [{}] {} -> Fix: {}", f.severity, f.description, f.remediation);
            }
        }
        "browse" => {
            let url = args.get(2).map(|s| s.as_str()).unwrap_or("https://example.com");
            let engine = if args.get(3).map(|s| s.as_str()) == Some("--full") {
                BrowserEngine::FullBrowser
            } else {
                BrowserEngine::Lightpanda
            };

            match BrowserRunner::fetch(url, engine) {
                Ok(snap) => {
                    println!("Browser Snapshot [Engine: {:?}]:", snap.engine_used);
                    println!("  URL: {}", snap.url);
                    println!("  Title: {}", snap.title);
                    println!("  Memory Footprint: {:.2} MB", snap.estimated_memory_kb as f64 / 1024.0);
                    println!("  Text Content: {}", snap.text_content);
                }
                Err(e) => eprintln!("Browser navigation failed: {}", e),
            }
        }
        "emulator" => {
            let state = EmulatorManager::probe_runtime();
            println!("Containerized Android Emulator Runtime:");
            println!("  Driver: {:?}", state.driver);
            println!("  Device ID: {}", state.device_id);
            println!("  Available: {}", state.is_available);

            let app_id = args.get(2).map(|s| s.as_str()).unwrap_or("com.example.app");
            println!("  Declarative Maestro E2E Flow for '{}':", app_id);
            let flow = EmulatorManager::generate_maestro_flow(app_id);
            for line in flow.lines() {
                println!("    {}", line);
            }
        }
        "sessions" => {
            let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
            let sessions = SessionHarvester::scan_all(&home);
            println!("Discovered Agent Sessions on Laptop (Cindy session-import model): {} sessions", sessions.len());
            for (idx, sess) in sessions.iter().take(15).enumerate() {
                println!(
                    "  [{:02}] [{:?}] {} ({} messages) | cwd: {}",
                    idx + 1,
                    sess.source,
                    sess.title,
                    sess.message_count,
                    sess.cwd
                );
            }
            if sessions.len() > 15 {
                println!("  ... and {} more sessions unified under OMW.", sessions.len() - 15);
            }
        }
        "version" | "--version" | "-v" => {
            println!("Oh My Works (OMW) CLI v0.1.0");
        }
        other => {
            eprintln!("Unknown command: '{}'. Available: status, terminal, exec, harness, memory, sync, clone, browse, emulator, sessions, version", other);
            std::process::exit(1);
        }
    }
}
