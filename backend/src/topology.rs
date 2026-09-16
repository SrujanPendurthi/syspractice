//! The canvas topology — the contract shared by the frontend, the DB, and the critique prompt.
//!
//! ponytail: hand-mirrored in `frontend/src/types.ts`. If the two drift, switch to `ts-rs`
//! and derive the TS from these types instead of maintaining both by hand.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Client,
    Cdn,
    LoadBalancer,
    ApiGateway,
    Service,
    Database,
    Cache,
    Queue,
    ObjectStore,
}

impl NodeKind {
    /// How the node is named to the critic. Kept separate from the wire format so the
    /// serde tag can change without rewriting the prompt.
    pub fn describe(self) -> &'static str {
        match self {
            NodeKind::Client => "client",
            NodeKind::Cdn => "CDN",
            NodeKind::LoadBalancer => "load balancer",
            NodeKind::ApiGateway => "API gateway",
            NodeKind::Service => "service",
            NodeKind::Database => "database",
            NodeKind::Cache => "cache",
            NodeKind::Queue => "message queue",
            NodeKind::ObjectStore => "object store",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topology {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Topology {
    fn label_of(&self, id: &str) -> &str {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.label.as_str())
            .unwrap_or("<dangling>")
    }

    /// Flatten to the plain-text form the model reads. Coordinates are dropped on purpose —
    /// layout is not part of the design being critiqued.
    pub fn describe(&self) -> String {
        let mut out = String::new();

        out.push_str("Components:\n");
        if self.nodes.is_empty() {
            out.push_str("  (none)\n");
        }
        for n in &self.nodes {
            out.push_str(&format!("  - {} [{}]\n", n.label, n.kind.describe()));
        }

        out.push_str("\nConnections:\n");
        if self.edges.is_empty() {
            out.push_str("  (none)\n");
        }
        for e in &self.edges {
            let arrow = match &e.label {
                Some(l) if !l.is_empty() => format!(" ({l})"),
                _ => String::new(),
            };
            out.push_str(&format!(
                "  - {} -> {}{}\n",
                self.label_of(&e.source),
                self.label_of(&e.target),
                arrow
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Topology {
        Topology {
            nodes: vec![
                Node { id: "n1".into(), kind: NodeKind::LoadBalancer, label: "LB".into(), x: 0.0, y: 0.0 },
                Node { id: "n2".into(), kind: NodeKind::Service, label: "Shortener".into(), x: 200.0, y: 0.0 },
            ],
            edges: vec![Edge {
                id: "e1".into(),
                source: "n1".into(),
                target: "n2".into(),
                label: Some("HTTP".into()),
            }],
        }
    }

    #[test]
    fn round_trips_through_the_wire_format() {
        let json = serde_json::to_string(&sample()).unwrap();
        // The frontend sends camelCase; a rename slip here silently breaks the canvas.
        assert!(json.contains(r#""kind":"loadBalancer""#), "got {json}");
        let back: Topology = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.edges[0].source, "n1");
    }

    #[test]
    fn describes_edges_by_label_not_id() {
        let text = sample().describe();
        assert!(text.contains("LB -> Shortener (HTTP)"), "got {text}");
        assert!(!text.contains("n1"), "ids must not leak into the prompt: {text}");
    }

    #[test]
    fn dangling_edges_do_not_panic() {
        let mut t = sample();
        t.nodes.clear();
        assert!(t.describe().contains("<dangling>"));
    }
}
