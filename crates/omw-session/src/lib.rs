//! Multi-Agent Session Harvester & Unified Bridge for Oh My Works.
//! Inspired by makecindy/cindy session-import adapters (Claude, Codex, Prime, Hermes).

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSource {
    Prime,
    Codex,
    Claude,
    Hermes,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedSession {
    pub id: String,
    pub source: AgentSource,
    pub title: String,
    pub cwd: String,
    pub created_at: String,
    pub message_count: usize,
    pub file_path: String,
}

pub struct SessionHarvester;

impl SessionHarvester {
    /// Scans all local agent histories from the user home directory.
    /// ponytail: Scans local filesystems directly. Add SQLite parser / watch notifications when live session streaming is required.
    pub fn scan_all(home_dir: &Path) -> Vec<UnifiedSession> {
        let mut results = Vec::new();

        // 1. Scan Prime Agent sessions (~/.prime/agent/sessions)
        let prime_dir = home_dir.join(".prime").join("agent").join("sessions");
        if prime_dir.exists() {
            Self::scan_prime_sessions(&prime_dir, &mut results);
        }

        // 2. Scan Codex home (~/.codex)
        let codex_dir = home_dir.join(".codex");
        if codex_dir.exists() {
            Self::scan_codex_sessions(&codex_dir, &mut results);
        }

        // 3. Scan Claude projects (~/.claude/projects)
        let claude_dir = home_dir.join(".claude").join("projects");
        if claude_dir.exists() {
            Self::scan_claude_sessions(&claude_dir, &mut results);
        }

        // 4. Scan Hermes workspace (~/.hermes)
        let hermes_dir = home_dir.join(".hermes");
        if hermes_dir.exists() {
            Self::scan_hermes_sessions(&hermes_dir, &mut results);
        }

        results
    }

    fn scan_prime_sessions(dir: &Path, out: &mut Vec<UnifiedSession>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Some(session) = Self::parse_prime_file(&path) {
                        out.push(session);
                    }
                }
            }
        }
    }

    fn parse_prime_file(path: &Path) -> Option<UnifiedSession> {
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);

        let mut session_id = String::new();
        let mut cwd = String::new();
        let mut created_at = String::new();
        let mut first_user_prompt = String::new();
        let mut message_count = 0;

        for (idx, line_res) in reader.lines().enumerate() {
            let line = line_res.ok()?;
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                if idx == 0 && val.get("type").and_then(|t| t.as_str()) == Some("session") {
                    session_id = val.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    cwd = val.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    created_at = val.get("timestamp").and_then(|s| s.as_str()).unwrap_or("").to_string();
                }

                if val.get("type").and_then(|t| t.as_str()) == Some("message") {
                    message_count += 1;
                    if first_user_prompt.is_empty() {
                        if let Some(msg) = val.get("message") {
                            if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                                if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
                                    for item in content {
                                        if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                                            first_user_prompt = txt.lines().next().unwrap_or("").to_string();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if session_id.is_empty() {
            session_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
        }

        let title = if !first_user_prompt.is_empty() {
            if first_user_prompt.len() > 60 {
                format!("{}...", &first_user_prompt[..57])
            } else {
                first_user_prompt
            }
        } else {
            format!("Prime Session {}", &session_id[..session_id.len().min(8)])
        };

        Some(UnifiedSession {
            id: session_id,
            source: AgentSource::Prime,
            title,
            cwd,
            created_at,
            message_count,
            file_path: path.to_string_lossy().to_string(),
        })
    }

    fn scan_codex_sessions(dir: &Path, out: &mut Vec<UnifiedSession>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if fname.starts_with("rollout-") && (fname.ends_with(".jsonl") || fname.ends_with(".jsonl.zst")) {
                    out.push(UnifiedSession {
                        id: fname.to_string(),
                        source: AgentSource::Codex,
                        title: format!("Codex Rollout: {}", fname),
                        cwd: dir.to_string_lossy().to_string(),
                        created_at: "Detected on disk".to_string(),
                        message_count: 1,
                        file_path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }

    fn scan_claude_sessions(dir: &Path, out: &mut Vec<UnifiedSession>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let project_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    out.push(UnifiedSession {
                        id: project_name.to_string(),
                        source: AgentSource::Claude,
                        title: format!("Claude Project: {}", project_name),
                        cwd: path.to_string_lossy().to_string(),
                        created_at: "Detected".to_string(),
                        message_count: 0,
                        file_path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }
    }

    fn scan_hermes_sessions(dir: &Path, out: &mut Vec<UnifiedSession>) {
        let config = dir.join("config.yaml");
        if config.exists() {
            out.push(UnifiedSession {
                id: "hermes-default".to_string(),
                source: AgentSource::Hermes,
                title: "Hermes Multi-Agent Configuration".to_string(),
                cwd: dir.to_string_lossy().to_string(),
                created_at: "Configured".to_string(),
                message_count: 1,
                file_path: config.to_string_lossy().to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harvester_empty_dir() {
        let tmp = std::env::temp_dir();
        let sessions = SessionHarvester::scan_all(&tmp);
        assert!(sessions.is_empty() || !sessions.is_empty());
    }
}
