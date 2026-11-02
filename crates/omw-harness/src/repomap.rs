//! Codebase AST & Symbol RepoMap Generator
//! Provides high-density architectural context with minimal token footprint.

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

pub struct RepoMap;

impl RepoMap {
    /// Recursively scan the workspace root and build a concise symbol map.
    pub fn build(root: &Path, max_chars: usize) -> String {
        let ignore_dirs = [
            ".git", "target", "node_modules", ".omw", "dist", "build", ".cargo", "vendor",
        ];

        let rs_re = Regex::new(r"^\s*(?:pub(?:\([^\)]+\))?\s+)?(?:async\s+)?(?:fn|struct|enum|trait|type)\s+([a-zA-Z0-9_]+)").unwrap();
        let py_re = Regex::new(r"^\s*(?:async\s+)?(?:def|class)\s+([a-zA-Z0-9_]+)").unwrap();
        let ts_re = Regex::new(r"^\s*(?:export\s+)?(?:async\s+)?(?:function|class|interface|type|const)\s+([a-zA-Z0-9_]+)").unwrap();
        let go_re = Regex::new(r"^\s*(?:func|type)\s+([a-zA-Z0-9_]+)").unwrap();

        let mut files = Vec::new();
        Self::collect_files(root, &ignore_dirs, &mut files);
        files.sort();

        let mut out = String::new();
        let mut total_chars = 0;

        for path in files {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let re = match ext {
                "rs" => Some(&rs_re),
                "py" => Some(&py_re),
                "ts" | "js" | "tsx" | "jsx" => Some(&ts_re),
                "go" => Some(&go_re),
                _ => None,
            };

            let Some(re) = re else { continue };

            let Ok(content) = fs::read_to_string(&path) else { continue };
            let mut symbols = Vec::new();

            for line in content.lines() {
                if let Some(caps) = re.captures(line) {
                    if let Some(m) = caps.get(1) {
                        symbols.push(m.as_str().to_string());
                        if symbols.len() >= 8 {
                            break;
                        }
                    }
                }
            }

            if !symbols.is_empty() {
                let rel_path = path.strip_prefix(root).unwrap_or(&path);
                let block = format!("{}:
  {}
", rel_path.display(), symbols.join(", "));
                if total_chars + block.len() > max_chars {
                    out.push_str("... [repo map truncated for token budget]
");
                    break;
                }
                out.push_str(&block);
                total_chars += block.len();
            }
        }

        if out.is_empty() {
            out = "(No supported code symbols found in workspace)".to_string();
        }

        out
    }

    fn collect_files(dir: &Path, ignore_dirs: &[&str], out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                if !ignore_dirs.contains(&file_name.as_str()) {
                    Self::collect_files(&path, ignore_dirs, out);
                }
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_repomap_generation() {
        let tmp = std::env::temp_dir().join("omw_repomap_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();

        fs::write(
            tmp.join("src/lib.rs"),
            "pub struct Engine;\npub fn run() {}\npub enum Status { Ok, Err }",
        )
        .unwrap();

        let map = RepoMap::build(&tmp, 1000);
        assert!(map.contains("src/lib.rs:"));
        assert!(map.contains("Engine"));
        assert!(map.contains("run"));
        assert!(map.contains("Status"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
