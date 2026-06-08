//! Simple left-to-right layered placement for loaded graphs (the schema stores
//! no positions). Each node's column = its longest distance from a source; rows
//! stack nodes that share a column. Good enough to make loaded examples legible.

use std::collections::HashMap;

use awsm_audio_schema::{ConnectionSink, ConnectionSource, Graph, NodeId};

const COL_W: f64 = 240.0;
const ROW_H: f64 = 150.0;
const ORIGIN_X: f64 = 70.0;
const ORIGIN_Y: f64 = 80.0;

/// World position for every node id in `graph`.
pub fn auto_layout(graph: &Graph) -> HashMap<NodeId, (f64, f64)> {
    // Directed node→node edges (ignore param/boundary targets).
    let edges: Vec<(NodeId, NodeId)> = graph
        .connections
        .iter()
        .filter_map(|c| match (&c.from, &c.to) {
            (
                ConnectionSource::NodeOutput { node: from, .. },
                ConnectionSink::NodeInput { node: to, .. },
            ) => Some((*from, *to)),
            _ => None,
        })
        .collect();

    // Longest-path depth via bounded relaxation (handles DAGs; caps cycles).
    let mut depth: HashMap<NodeId, usize> = graph.nodes.iter().map(|n| (n.id, 0)).collect();
    for _ in 0..graph.nodes.len() {
        let mut changed = false;
        for (from, to) in &edges {
            let d = depth.get(from).copied().unwrap_or(0) + 1;
            if d > depth.get(to).copied().unwrap_or(0) {
                depth.insert(*to, d);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Assign rows per column in node declaration order (stable).
    let mut row_in_col: HashMap<usize, usize> = HashMap::new();
    let mut out = HashMap::new();
    for node in &graph.nodes {
        let col = depth.get(&node.id).copied().unwrap_or(0);
        let row = row_in_col.entry(col).or_insert(0);
        let x = ORIGIN_X + col as f64 * COL_W;
        let y = ORIGIN_Y + *row as f64 * ROW_H;
        *row += 1;
        out.insert(node.id, (x, y));
    }
    out
}
