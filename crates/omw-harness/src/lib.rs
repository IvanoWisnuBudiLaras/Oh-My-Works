//! Meta-AI Harness: Multi-agent coordination, Autonomous Agent Loops,
//! Codebase AST RepoMap, Deterministic Tools, and Verification Pipelines.

pub mod repomap;
pub mod tools;
pub mod pipeline;

pub use repomap::RepoMap;
pub use tools::{ToolCall, ToolEngine, ToolResult};
pub use pipeline::{PipelineExecution, PipelineStep};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Running,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subagent {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub assigned_agent_id: Option<String>,
    pub result: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HarnessOrchestrator {
    pub agents: Vec<Subagent>,
    pub tasks: Vec<Task>,
}

impl HarnessOrchestrator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_agent(&mut self, id: impl Into<String>, name: impl Into<String>, role: impl Into<String>) {
        self.agents.push(Subagent {
            id: id.into(),
            name: name.into(),
            role: role.into(),
            status: AgentStatus::Idle,
        });
    }

    pub fn create_task(&mut self, id: impl Into<String>, title: impl Into<String>, prompt: impl Into<String>) -> usize {
        let task = Task {
            id: id.into(),
            title: title.into(),
            prompt: prompt.into(),
            assigned_agent_id: None,
            result: None,
        };
        self.tasks.push(task);
        self.tasks.len() - 1
    }

    pub fn dispatch(&mut self, task_index: usize, agent_id: &str) -> bool {
        let agent_exists = self.agents.iter().any(|a| a.id == agent_id);
        if !agent_exists {
            return false;
        }

        if let Some(task) = self.tasks.get_mut(task_index) {
            task.assigned_agent_id = Some(agent_id.to_string());
            if let Some(agent) = self.agents.iter_mut().find(|a| a.id == agent_id) {
                agent.status = AgentStatus::Running;
            }
            true
        } else {
            false
        }
    }

    pub fn complete_task(&mut self, task_index: usize, result: impl Into<String>) -> bool {
        if let Some(task) = self.tasks.get_mut(task_index) {
            let res_str = result.into();
            task.result = Some(res_str.clone());
            if let Some(ref agent_id) = task.assigned_agent_id {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.id == *agent_id) {
                    agent.status = AgentStatus::Completed;
                }
            }
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
    fn test_orchestrator_lifecycle() {
        let mut orch = HarnessOrchestrator::new();
        orch.register_agent("worker-1", "Coder", "Rust Developer");
        let task_idx = orch.create_task("t1", "Fix OOM", "Analyze PSI metrics");

        assert!(orch.dispatch(task_idx, "worker-1"));
        assert_eq!(orch.agents[0].status, AgentStatus::Running);

        assert!(orch.complete_task(task_idx, "Earlyoom policy tuned"));
        assert_eq!(orch.agents[0].status, AgentStatus::Completed);
        assert_eq!(orch.tasks[0].result.as_deref(), Some("Earlyoom policy tuned"));
    }
}
