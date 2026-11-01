//! GitHub auto-backup and workspace snapshot engine for Oh My Works.

use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    pub is_clean: bool,
    pub branch: String,
    pub uncommitted_files: usize,
}

pub struct WorkspaceSync;

impl WorkspaceSync {
    /// ponytail: Calls git CLI via std::process. Upgrade to libgit2 (git2-rs) when embedded zero-git-runtime environments are required.
    pub fn check_status(repo_dir: &str) -> Result<SyncStatus, String> {
        let branch_out = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .map_err(|e| format!("Failed to read git branch: {}", e))?;

        let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();

        let status_out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(repo_dir)
            .output()
            .map_err(|e| format!("Failed to read git status: {}", e))?;

        let status_str = String::from_utf8_lossy(&status_out.stdout);
        let uncommitted_files = status_str.lines().filter(|l| !l.trim().is_empty()).count();

        Ok(SyncStatus {
            is_clean: uncommitted_files == 0,
            branch,
            uncommitted_files,
        })
    }

    pub fn auto_backup(repo_dir: &str, commit_message: &str) -> Result<String, String> {
        let status = Self::check_status(repo_dir)?;
        if status.is_clean {
            return Ok("Workspace clean, nothing to back up".to_string());
        }

        // git add .
        let add_res = Command::new("git")
            .args(["add", "."])
            .current_dir(repo_dir)
            .status()
            .map_err(|e| format!("Git add failed: {}", e))?;

        if !add_res.success() {
            return Err("Failed staging files for backup".to_string());
        }

        // git commit -m
        let commit_res = Command::new("git")
            .args(["commit", "-m", commit_message])
            .current_dir(repo_dir)
            .output()
            .map_err(|e| format!("Git commit failed: {}", e))?;

        if !commit_res.status.success() {
            return Err(format!("Commit error: {}", String::from_utf8_lossy(&commit_res.stderr)));
        }

        Ok(format!("Backed up {} uncommitted changes on branch {}", status.uncommitted_files, status.branch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_in_current_repo() {
        let status = WorkspaceSync::check_status(".");
        assert!(status.is_ok());
        let s = status.unwrap();
        assert!(!s.branch.is_empty());
    }
}
