//! CodeGraph Memory, Vector SQL & Hierarchical Session Engine for Oh My Works.
//! Inspired by websines/codegraph-mcp & AST knowledge-graph MCP servers.

use rusqlite::{params, Connection};
pub use rusqlite::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeItem {
    pub id: String,
    pub root_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub branch_label: String,
    pub created_at: String,
    pub turn_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub author: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSymbol {
    pub id: String,
    pub file_path: String,
    pub name: String,
    pub symbol_type: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningReflection {
    pub id: String,
    pub session_id: String,
    pub category: String, // "pattern", "failure", "convention"
    pub rule: String,
    pub evidence: String,
    pub confidence: f64,
}

pub struct CodeGraphStore {
    conn: Connection,
}

impl CodeGraphStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                root_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                branch_label TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS turns (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                author TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );

            CREATE TABLE IF NOT EXISTS code_symbols (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                signature TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS code_edges (
                from_symbol TEXT NOT NULL,
                to_symbol TEXT NOT NULL,
                relation TEXT NOT NULL,
                PRIMARY KEY (from_symbol, to_symbol, relation)
            );

            CREATE TABLE IF NOT EXISTS learning_reflections (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                category TEXT NOT NULL,
                rule TEXT NOT NULL,
                evidence TEXT NOT NULL,
                confidence REAL DEFAULT 1.0,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_root ON sessions(root_id);
            CREATE INDEX IF NOT EXISTS idx_turns_session ON turns(session_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);
            "#,
        )?;
        Ok(())
    }

    // --- Hierarchical Sessions ---

    pub fn insert_session(&self, item: &SessionTreeItem) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (id, root_id, parent_id, title, branch_label, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![item.id, item.root_id, item.parent_id, item.title, item.branch_label, item.created_at],
        )?;
        Ok(())
    }

    pub fn get_session_tree(&self, root_id: &str) -> Result<Vec<SessionTreeItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.root_id, s.parent_id, s.title, s.branch_label, CAST(s.created_at AS TEXT), COUNT(t.id) as cnt
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             WHERE s.root_id = ?1
             GROUP BY s.id
             ORDER BY s.created_at ASC"
        )?;

        let rows = stmt.query_map(params![root_id], |row| {
            Ok(SessionTreeItem {
                id: row.get(0)?,
                root_id: row.get(1)?,
                parent_id: row.get(2)?,
                title: row.get(3)?,
                branch_label: row.get(4)?,
                created_at: row.get(5)?,
                turn_count: { let c: i64 = row.get(6)?; c as usize },
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_all_roots(&self) -> Result<Vec<SessionTreeItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.root_id, s.parent_id, s.title, s.branch_label, CAST(s.created_at AS TEXT), COUNT(t.id) as cnt
             FROM sessions s
             LEFT JOIN turns t ON s.id = t.session_id
             WHERE s.parent_id IS NULL OR s.id = s.root_id
             GROUP BY s.id
             ORDER BY s.created_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SessionTreeItem {
                id: row.get(0)?,
                root_id: row.get(1)?,
                parent_id: row.get(2)?,
                title: row.get(3)?,
                branch_label: row.get(4)?,
                created_at: row.get(5)?,
                turn_count: { let c: i64 = row.get(6)?; c as usize },
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM turns WHERE session_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM sessions WHERE id = ?1 OR root_id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_session_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE sessions SET title = ?1 WHERE id = ?2", params![title, id])?;
        Ok(())
    }

    // --- Turns & History ---

    pub fn save_turn(&self, turn: &ChatTurn) -> Result<()> {
        self.conn.execute(
            "INSERT INTO turns (id, session_id, role, content, author, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![turn.id, turn.session_id, turn.role, turn.content, turn.author, turn.created_at],
        )?;
        Ok(())
    }

    pub fn get_turns(&self, session_id: &str) -> Result<Vec<ChatTurn>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, author, CAST(created_at AS TEXT) FROM turns WHERE session_id = ?1 ORDER BY created_at ASC"
        )?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok(ChatTurn {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                author: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // --- CodeGraph Symbols & Traversal ---

    pub fn insert_symbol(&self, sym: &CodeSymbol) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO code_symbols (id, file_path, name, symbol_type, signature) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sym.id, sym.file_path, sym.name, sym.symbol_type, sym.signature],
        )?;
        Ok(())
    }

    pub fn link_symbols(&self, from: &str, to: &str, relation: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO code_edges (from_symbol, to_symbol, relation) VALUES (?1, ?2, ?3)",
            params![from, to, relation],
        )?;
        Ok(())
    }

    pub fn find_related_symbols(&self, symbol_name: &str) -> Result<Vec<CodeSymbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.file_path, s.name, s.symbol_type, s.signature
             FROM code_symbols s
             JOIN code_edges e ON s.id = e.to_symbol
             WHERE e.from_symbol = ?1"
        )?;

        let rows = stmt.query_map(params![symbol_name], |row| {
            Ok(CodeSymbol {
                id: row.get(0)?,
                file_path: row.get(1)?,
                name: row.get(2)?,
                symbol_type: row.get(3)?,
                signature: row.get(4)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // --- Learning & Reflection (Auto-Refinement) ---

    pub fn record_reflection(&self, ref_item: &LearningReflection) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO learning_reflections (id, session_id, category, rule, evidence, confidence, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)",
            params![ref_item.id, ref_item.session_id, ref_item.category, ref_item.rule, ref_item.evidence, ref_item.confidence],
        )?;
        Ok(())
    }

    pub fn list_active_reflections(&self) -> Result<Vec<LearningReflection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, category, rule, evidence, confidence FROM learning_reflections ORDER BY confidence DESC LIMIT 25"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(LearningReflection {
                id: row.get(0)?,
                session_id: row.get(1)?,
                category: row.get(2)?,
                rule: row.get(3)?,
                evidence: row.get(4)?,
                confidence: row.get(5)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Auto-refines knowledge by extracting rules from successful/failed execution evidence
    pub fn auto_refine(&self, session_id: &str, turn_text: &str) -> Result<Option<LearningReflection>> {
        let lower = turn_text.to_lowercase();
        // Pattern detection: ponytail: heuristics on output, test results, or error messages
        if lower.contains("error") || lower.contains("panic") || lower.contains("failed") {
            let item = LearningReflection {
                id: format!("ref-fail-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()),
                session_id: session_id.to_string(),
                category: "failure".to_string(),
                rule: "Avoid patterns causing panics or test regressions".to_string(),
                evidence: turn_text.chars().take(200).collect(),
                confidence: 1.0,
            };
            self.record_reflection(&item)?;
            return Ok(Some(item));
        }

        if lower.contains("finished") || lower.contains("passed") || lower.contains("ponytail:") || lower.contains("cyberpunk") {
            let item = LearningReflection {
                id: format!("ref-pat-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()),
                session_id: session_id.to_string(),
                category: "pattern".to_string(),
                rule: "Prefer standard library zero-dependency execution with ponytail markers".to_string(),
                evidence: turn_text.chars().take(200).collect(),
                confidence: 1.5,
            };
            self.record_reflection(&item)?;
            return Ok(Some(item));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegraph_hierarchical_sessions() -> Result<()> {
        let store = CodeGraphStore::in_memory()?;

        // Create root session
        let root = SessionTreeItem {
            id: "root-1".to_string(),
            root_id: "root-1".to_string(),
            parent_id: None,
            title: "Project Architecture".to_string(),
            branch_label: "main".to_string(),
            created_at: "2026-11-01 10:00:00".to_string(),
            turn_count: 0,
        };
        store.insert_session(&root)?;

        // Create child branch thread
        let child = SessionTreeItem {
            id: "branch-1".to_string(),
            root_id: "root-1".to_string(),
            parent_id: Some("root-1".to_string()),
            title: "Optimize PSI Guardian".to_string(),
            branch_label: "feat/guardian".to_string(),
            created_at: "2026-11-01 10:15:00".to_string(),
            turn_count: 0,
        };
        store.insert_session(&child)?;

        // Save turns
        let t1 = ChatTurn {
            id: "t-1".to_string(),
            session_id: "branch-1".to_string(),
            role: "user".to_string(),
            content: "Check PSI stall".to_string(),
            author: "USER".to_string(),
            created_at: "2026-11-01 10:15:30".to_string(),
        };
        store.save_turn(&t1)?;

        let tree = store.get_session_tree("root-1")?;
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[1].id, "branch-1");
        assert_eq!(tree[1].parent_id, Some("root-1".to_string()));

        let turns = store.get_turns("branch-1")?;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].content, "Check PSI stall");

        // Code symbols link
        store.insert_symbol(&CodeSymbol {
            id: "sym_guardian".to_string(),
            file_path: "crates/omw-guardian/src/lib.rs".to_string(),
            name: "Guardian::assess".to_string(),
            symbol_type: "fn".to_string(),
            signature: "pub fn assess() -> Result<(HealthLevel, MemoryPressure)>".to_string(),
        })?;

        store.insert_symbol(&CodeSymbol {
            id: "sym_mitigate".to_string(),
            file_path: "crates/omw-guardian/src/lib.rs".to_string(),
            name: "Guardian::mitigate".to_string(),
            symbol_type: "fn".to_string(),
            signature: "pub fn mitigate() -> Result<()>".to_string(),
        })?;

        store.link_symbols("sym_guardian", "sym_mitigate", "calls")?;
        let related = store.find_related_symbols("sym_guardian")?;
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].name, "Guardian::mitigate");

        // Reflection
        let ref_res = store.auto_refine("branch-1", "Finished release [optimized] target in 0.4s. ponytail: minimal")?;
        assert!(ref_res.is_some());
        let reflections = store.list_active_reflections()?;
        assert_eq!(reflections.len(), 1);

        Ok(())
    }
}
