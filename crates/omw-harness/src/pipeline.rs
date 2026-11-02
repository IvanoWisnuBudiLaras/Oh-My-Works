//! Multi-Agent Reviewer & Autonomous Pipeline
//! Planner (Architect) -> Specialist Coder (Tool Loop) -> Verifier (Test/Linter) -> Git Commit.

use crate::tools::ToolEngine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub stage: String,
    pub title: String,
    pub status: String, // "pending", "running", "completed", "failed"
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineExecution {
    pub goal: String,
    pub steps: Vec<PipelineStep>,
    pub current_step: usize,
    pub verification_passed: bool,
    pub commit_hash: Option<String>,
}

impl PipelineExecution {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            steps: vec![
                PipelineStep {
                    stage: "planner".to_string(),
                    title: "Architect Planning & Symbol Discovery".to_string(),
                    status: "pending".to_string(),
                    summary: None,
                },
                PipelineStep {
                    stage: "coder".to_string(),
                    title: "Specialist Coder Autonomous Tool Loop".to_string(),
                    status: "pending".to_string(),
                    summary: None,
                },
                PipelineStep {
                    stage: "verifier".to_string(),
                    title: "Test Verification & Compiler Diagnostics".to_string(),
                    status: "pending".to_string(),
                    summary: None,
                },
                PipelineStep {
                    stage: "commit".to_string(),
                    title: "Git Conventional Auto-Commit".to_string(),
                    status: "pending".to_string(),
                    summary: None,
                },
            ],
            current_step: 0,
            verification_passed: false,
            commit_hash: None,
        }
    }

    /// Run the verification step using a specified test or linter command.
    pub fn run_verification(test_cmd: &str, workspace_root: &Path) -> (bool, String) {
        let res = ToolEngine::execute("bash", &json!({ "command": test_cmd }), workspace_root);
        (res.success, res.output)
    }

    /// Auto-commit when verification is green.
    pub fn auto_commit(commit_msg: &str, workspace_root: &Path) -> (bool, String) {
        let res = ToolEngine::execute("git_commit", &json!({ "message": commit_msg }), workspace_root);
        (res.success, res.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_instantiation() {
        let pipe = PipelineExecution::new("Fix memory leak in parser");
        assert_eq!(pipe.steps.len(), 4);
        assert_eq!(pipe.steps[0].stage, "planner");
        assert_eq!(pipe.steps[1].stage, "coder");
        assert_eq!(pipe.steps[2].stage, "verifier");
        assert_eq!(pipe.steps[3].stage, "commit");
    }
}
