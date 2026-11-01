//! Multi-terminal session manager & process execution bridge for Oh My Works.

use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Idle,
    Exited(i32),
}

#[derive(Debug, Clone)]
pub struct TerminalSession {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub status: SessionStatus,
    pub history: Vec<String>,
}

#[derive(Debug, Default)]
pub struct TerminalMultiplexer {
    pub sessions: Vec<TerminalSession>,
    pub active_index: usize,
}

impl TerminalMultiplexer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn_session(&mut self, id: impl Into<String>, title: impl Into<String>, cwd: impl Into<String>) -> usize {
        let session = TerminalSession {
            id: id.into(),
            title: title.into(),
            cwd: cwd.into(),
            status: SessionStatus::Active,
            history: Vec::new(),
        };
        self.sessions.push(session);
        self.sessions.len() - 1
    }

    pub fn active_session(&self) -> Option<&TerminalSession> {
        self.sessions.get(self.active_index)
    }

    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.sessions.len() {
            self.active_index = index;
            true
        } else {
            false
        }
    }

    /// Executes a command in the specified session's directory.
    /// ponytail: Synchronous subprocess execution. Upgrade to asynchronous non-blocking stream / PTY loop when interactive TUI (vim/htop) is required.
    pub fn execute(&mut self, session_id: &str, program: &str, args: &[&str]) -> Result<String, String> {
        let session = self.sessions.iter_mut().find(|s| s.id == session_id)
            .ok_or_else(|| format!("Session '{}' not found", session_id))?;

        let output = Command::new(program)
            .args(args)
            .current_dir(&session.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to execute '{}': {}", program, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        session.status = SessionStatus::Exited(exit_code);
        session.history.push(format!("$ {} {}", program, args.join(" ")));

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(format!("Exit code {}: {}", exit_code, stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiplexer_spawn_and_switch() {
        let mut mux = TerminalMultiplexer::new();
        let _idx0 = mux.spawn_session("tab-1", "Bash Main", ".");
        let idx1 = mux.spawn_session("tab-2", "AI Agent Worker", ".");

        assert_eq!(mux.sessions.len(), 2);
        assert_eq!(mux.active_session().unwrap().id, "tab-1");

        assert!(mux.switch_to(idx1));
        assert_eq!(mux.active_session().unwrap().id, "tab-2");
        assert!(!mux.switch_to(99));
    }

    #[test]
    fn test_terminal_execute() {
        let mut mux = TerminalMultiplexer::new();
        mux.spawn_session("exec-tab", "Runner", ".");

        let out = mux.execute("exec-tab", "git", &["--version"]);
        assert!(out.is_ok());
        assert!(out.unwrap().contains("git version"));

        let sess = mux.sessions.iter().find(|s| s.id == "exec-tab").unwrap();
        assert_eq!(sess.status, SessionStatus::Exited(0));
        assert_eq!(sess.history.len(), 1);
    }
}
