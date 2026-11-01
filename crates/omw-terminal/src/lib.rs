//! Lightweight multi-terminal session manager for Oh My Works.

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiplexer_spawn_and_switch() {
        let mut mux = TerminalMultiplexer::new();
        let _idx0 = mux.spawn_session("tab-1", "Bash Main", "/home");
        let idx1 = mux.spawn_session("tab-2", "AI Agent Worker", "/home/project");

        assert_eq!(mux.sessions.len(), 2);
        assert_eq!(mux.active_session().unwrap().id, "tab-1");

        assert!(mux.switch_to(idx1));
        assert_eq!(mux.active_session().unwrap().id, "tab-2");
        assert!(!mux.switch_to(99));
    }
}
