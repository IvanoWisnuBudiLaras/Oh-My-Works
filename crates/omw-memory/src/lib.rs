//! Hybrid Tri-Layer Graph Memory for Oh My Works.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryLayer {
    Episodic,   // Raw logs, turns, chat events
    Semantic,   // Concepts, rules, architectural facts
    Procedural, // Reusable workflows, skills, scripts
}

#[derive(Debug, Clone)]
pub struct MemoryNode {
    pub id: String,
    pub layer: MemoryLayer,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct MemoryEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Default)]
pub struct GraphMemory {
    pub nodes: Vec<MemoryNode>,
    pub edges: Vec<MemoryEdge>,
}

impl GraphMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_node(&mut self, id: impl Into<String>, layer: MemoryLayer, title: impl Into<String>, content: impl Into<String>) {
        self.nodes.push(MemoryNode {
            id: id.into(),
            layer,
            title: title.into(),
            content: content.into(),
        });
    }

    pub fn link(&mut self, from: impl Into<String>, to: impl Into<String>, relation: impl Into<String>) {
        self.edges.push(MemoryEdge {
            from: from.into(),
            to: to.into(),
            relation: relation.into(),
        });
    }

    pub fn find_related(&self, node_id: &str) -> Vec<&MemoryNode> {
        let target_ids: Vec<&str> = self.edges.iter()
            .filter(|e| e.from == node_id)
            .map(|e| e.to.as_str())
            .collect();

        self.nodes.iter().filter(|n| target_ids.contains(&n.id.as_str())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_memory_link_and_query() {
        let mut gm = GraphMemory::new();
        gm.insert_node("p1", MemoryLayer::Semantic, "Anti-OOM", "Earlyoom via PSI");
        gm.insert_node("p2", MemoryLayer::Procedural, "Restart Daemon", "systemctl restart omw");
        gm.link("p1", "p2", "remediates_with");

        let related = gm.find_related("p1");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].title, "Restart Daemon");
    }
}
