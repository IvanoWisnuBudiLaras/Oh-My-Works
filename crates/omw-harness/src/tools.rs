//! Autonomous Agent Tool Execution Engine
//! Implements deterministic tools: read_file, edit_file, write_file, bash, search, git_diff, git_commit.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

pub struct ToolEngine;

impl ToolEngine {
    /// Execute a tool call against the given workspace root directory.
    pub fn execute(name: &str, args: &Value, workspace_root: &Path) -> ToolResult {
        match name {
            "read_file" => Self::read_file(args, workspace_root),
            "edit_file" => Self::edit_file(args, workspace_root),
            "write_file" => Self::write_file(args, workspace_root),
            "bash" | "exec" => Self::bash(args, workspace_root),
            "search" | "grep" => Self::search(args, workspace_root),
            "git_diff" => Self::git_diff(workspace_root),
            "git_commit" => Self::git_commit(args, workspace_root),
            _ => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Unknown tool: {}", name)),
            },
        }
    }

    fn resolve_path(p: &str, root: &Path) -> PathBuf {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            pb
        } else {
            root.join(pb)
        }
    }

    fn read_file(args: &Value, root: &Path) -> ToolResult {
        let Some(p) = args["path"].as_str() else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'path' argument".to_string()),
            };
        };

        let target = Self::resolve_path(p, root);
        let content = match fs::read_to_string(&target) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read {}: {}", target.display(), e)),
                }
            }
        };

        let start_line = args["start_line"].as_u64().unwrap_or(1) as usize;
        let end_line = args["end_line"].as_u64().unwrap_or(usize::MAX as u64) as usize;

        let mut numbered = String::new();
        for (i, line) in content.lines().enumerate() {
            let line_num = i + 1;
            if line_num >= start_line && line_num <= end_line {
                numbered.push_str(&format!("{:4} | {}\n", line_num, line));
            }
        }

        ToolResult {
            success: true,
            output: numbered,
            error: None,
        }
    }

    fn edit_file(args: &Value, root: &Path) -> ToolResult {
        let (Some(p), Some(old_str), Some(new_str)) = (
            args["path"].as_str(),
            args["old_str"].as_str(),
            args["new_str"].as_str(),
        ) else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some("edit_file requires 'path', 'old_str', and 'new_str'".to_string()),
            };
        };

        let target = Self::resolve_path(p, root);
        let content = match fs::read_to_string(&target) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read {}: {}", target.display(), e)),
                }
            }
        };

        let matches: Vec<_> = content.match_indices(old_str).collect();
        if matches.is_empty() {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("old_str not found in {}", target.display())),
            };
        }
        if matches.len() > 1 {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "old_str occurs {} times in {}. Provide more surrounding context to disambiguate.",
                    matches.len(),
                    target.display()
                )),
            };
        }

        let updated = content.replacen(old_str, new_str, 1);
        if let Err(e) = fs::write(&target, updated) {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write {}: {}", target.display(), e)),
            };
        }

        ToolResult {
            success: true,
            output: format!("Successfully replaced target in {}", target.display()),
            error: None,
        }
    }

    fn write_file(args: &Value, root: &Path) -> ToolResult {
        let (Some(p), Some(content)) = (args["path"].as_str(), args["content"].as_str()) else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some("write_file requires 'path' and 'content'".to_string()),
            };
        };

        let target = Self::resolve_path(p, root);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Err(e) = fs::write(&target, content) {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to write {}: {}", target.display(), e)),
            };
        }

        ToolResult {
            success: true,
            output: format!("Wrote {} bytes to {}", content.len(), target.display()),
            error: None,
        }
    }

    fn bash(args: &Value, root: &Path) -> ToolResult {
        let Some(cmd) = args["command"].as_str() else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some("bash requires 'command'".to_string()),
            };
        };

        let cwd = args["cwd"]
            .as_str()
            .map(|c| Self::resolve_path(c, root))
            .unwrap_or_else(|| root.to_path_buf());

        let out = Command::new("bash")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .output();

        match out {
            Ok(o) => {
                let code = o.status.code().unwrap_or(0);
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();

                let mut combined = stdout;
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push_str("\n[STDERR]\n");
                    }
                    combined.push_str(&stderr);
                }

                ToolResult {
                    success: code == 0,
                    output: combined,
                    error: if code == 0 {
                        None
                    } else {
                        Some(format!("Command exited with status {}", code))
                    },
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to spawn process: {}", e)),
            },
        }
    }

    fn search(args: &Value, root: &Path) -> ToolResult {
        let Some(query) = args["query"].as_str() else {
            return ToolResult {
                success: false,
                output: String::new(),
                error: Some("search requires 'query'".to_string()),
            };
        };

        let target_dir = args["path"]
            .as_str()
            .map(|p| Self::resolve_path(p, root))
            .unwrap_or_else(|| root.to_path_buf());

        // Try ripgrep first
        let out = Command::new("rg")
            .arg("-n")
            .arg("-C")
            .arg("1")
            .arg("--max-count")
            .arg("30")
            .arg("--glob")
            .arg("!target")
            .arg("--glob")
            .arg("!.git")
            .arg("--glob")
            .arg("!node_modules")
            .arg(query)
            .current_dir(&target_dir)
            .output();

        match out {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                if stdout.trim().is_empty() {
                    ToolResult {
                        success: true,
                        output: format!("No matches found for query: '{}'", query),
                        error: None,
                    }
                } else {
                    ToolResult {
                        success: true,
                        output: stdout,
                        error: None,
                    }
                }
            }
            Err(_) => {
                // Fallback to simple line-scan if rg missing
                ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("ripgrep (rg) not available on host".to_string()),
                }
            }
        }
    }

    fn git_diff(root: &Path) -> ToolResult {
        let out = Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(root)
            .output();

        let status_out = Command::new("git")
            .args(["status", "-s"])
            .current_dir(root)
            .output();

        let diff_stat = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let stat_s = status_out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();

        let combined = format!("Git Status:\n{}\nGit Diff Stat:\n{}", stat_s, diff_stat);

        ToolResult {
            success: true,
            output: combined,
            error: None,
        }
    }

    fn git_commit(args: &Value, root: &Path) -> ToolResult {
        let msg = args["message"].as_str().unwrap_or("chore: auto-commit by OMW agent");

        let _ = Command::new("git").args(["add", "-A"]).current_dir(root).output();
        let commit_res = Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(root)
            .output();

        match commit_res {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).to_string();
                let err = String::from_utf8_lossy(&o.stderr).to_string();
                let success = o.status.success();
                ToolResult {
                    success,
                    output: format!("Commit result:\n{}\n{}", text, err),
                    error: if success { None } else { Some("Commit failed".to_string()) },
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to execute git commit: {}", e)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use std::fs;

    #[test]
    fn test_tools_lifecycle() {
        let tmp = std::env::temp_dir().join("omw_tools_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // 1. Write file
        let res_write = ToolEngine::execute(
            "write_file",
            &json!({ "path": "test.txt", "content": "hello world\nline 2" }),
            &tmp,
        );
        assert!(res_write.success);

        // 2. Read file
        let res_read = ToolEngine::execute(
            "read_file",
            &json!({ "path": "test.txt", "start_line": 1, "end_line": 2 }),
            &tmp,
        );
        assert!(res_read.success);
        assert!(res_read.output.contains("hello world"));

        // 3. Edit file
        let res_edit = ToolEngine::execute(
            "edit_file",
            &json!({ "path": "test.txt", "old_str": "hello world", "new_str": "hello rust" }),
            &tmp,
        );
        assert!(res_edit.success);

        // Read again to verify
        let res_read2 = ToolEngine::execute("read_file", &json!({ "path": "test.txt" }), &tmp);
        assert!(res_read2.output.contains("hello rust"));

        // 4. Bash tool
        let res_bash = ToolEngine::execute(
            "bash",
            &json!({ "command": "cat test.txt | grep rust" }),
            &tmp,
        );
        assert!(res_bash.success);
        assert!(res_bash.output.contains("hello rust"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
