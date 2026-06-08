//! Structural validation of a [`SampleLibrary`]. This catches the wiring
//! mistakes the type system can't — dangling references, sample-ref cycles, a
//! custom oscillator with no waveform — before the player tries to build a real
//! audio graph from the document.

use crate::connection::{ConnectionSink, ConnectionSource};
use crate::enums::OscillatorType;
use crate::graph::Graph;
use crate::ids::{NodeId, PortId, SampleId};
use crate::library::SampleLibrary;
use crate::nodes::NodeKind;
use crate::sample::Sample;

/// A structural defect found by [`SampleLibrary::validate`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    #[error("sample {sample} connection references unknown node {node}")]
    UnknownNode { sample: SampleId, node: NodeId },

    #[error("sample {sample} connection references undeclared inlet {port}")]
    UnknownInlet { sample: SampleId, port: PortId },

    #[error("sample {sample} connection references undeclared outlet {port}")]
    UnknownOutlet { sample: SampleId, port: PortId },

    #[error("sample {sample} references unknown sample {target}")]
    UnknownSampleRef { sample: SampleId, target: SampleId },

    #[error("sample reference cycle through sample {sample}")]
    SampleCycle { sample: SampleId },

    #[error("custom oscillator on node {node} (sample {sample}) has no periodic wave")]
    MissingPeriodicWave { sample: SampleId, node: NodeId },

    #[error("library root references unknown sample {0}")]
    UnknownRoot(SampleId),

    #[error("sample {sample} has an incompatible wire (signal kinds don't match)")]
    IncompatibleWire { sample: SampleId },
}

impl SampleLibrary {
    /// Validate every sample's wiring and the cross-sample reference graph.
    /// Returns all defects found (empty = valid).
    pub fn validate(&self) -> Vec<SchemaError> {
        let mut errors = Vec::new();

        if let Some(root) = self.root {
            if self.sample(root).is_none() {
                errors.push(SchemaError::UnknownRoot(root));
            }
        }

        for sample in &self.samples {
            validate_graph(sample, &sample.graph, self, &mut errors);
        }

        // Sample-reference cycle detection (3-color DFS over the ref graph).
        let mut state: Vec<(SampleId, Color)> =
            self.samples.iter().map(|s| (s.id, Color::White)).collect();
        for sample in &self.samples {
            detect_cycle(sample.id, self, &mut state, &mut errors);
        }

        errors
    }
}

fn validate_graph(
    sample: &Sample,
    graph: &Graph,
    lib: &SampleLibrary,
    errors: &mut Vec<SchemaError>,
) {
    let has_node = |id: NodeId| graph.nodes.iter().any(|n| n.id == id);
    let has_inlet = |port: &PortId| graph.inlets.iter().any(|p| &p.id == port);
    let has_outlet = |port: &PortId| graph.outlets.iter().any(|p| &p.id == port);

    for conn in &graph.connections {
        match &conn.from {
            ConnectionSource::NodeOutput { node, .. } if !has_node(*node) => {
                errors.push(SchemaError::UnknownNode {
                    sample: sample.id,
                    node: *node,
                });
            }
            ConnectionSource::Inlet { port } if !has_inlet(port) => {
                errors.push(SchemaError::UnknownInlet {
                    sample: sample.id,
                    port: port.clone(),
                });
            }
            _ => {}
        }
        match &conn.to {
            ConnectionSink::NodeInput { node, .. } | ConnectionSink::NodeParam { node, .. }
                if !has_node(*node) =>
            {
                errors.push(SchemaError::UnknownNode {
                    sample: sample.id,
                    node: *node,
                });
            }
            ConnectionSink::Outlet { port } if !has_outlet(port) => {
                errors.push(SchemaError::UnknownOutlet {
                    sample: sample.id,
                    port: port.clone(),
                });
            }
            _ => {}
        }

        // Signal-kind compatibility (the typed port matrix). Skips wires with a
        // dangling endpoint — those are already reported above.
        if !graph.can_wire(conn) {
            errors.push(SchemaError::IncompatibleWire { sample: sample.id });
        }
    }

    for node in &graph.nodes {
        match &node.kind {
            NodeKind::Oscillator(osc)
                if osc.oscillator_type == OscillatorType::Custom && osc.harmonics.is_empty() =>
            {
                errors.push(SchemaError::MissingPeriodicWave {
                    sample: sample.id,
                    node: node.id,
                });
            }
            NodeKind::Sample(sref) if lib.sample(sref.sample).is_none() => {
                errors.push(SchemaError::UnknownSampleRef {
                    sample: sample.id,
                    target: sref.sample,
                });
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

fn color_of(state: &[(SampleId, Color)], id: SampleId) -> Option<Color> {
    state.iter().find(|(s, _)| *s == id).map(|(_, c)| *c)
}

fn set_color(state: &mut [(SampleId, Color)], id: SampleId, color: Color) {
    if let Some(entry) = state.iter_mut().find(|(s, _)| *s == id) {
        entry.1 = color;
    }
}

/// DFS marking gray-on-enter / black-on-exit; a back-edge to a gray node is a
/// cycle. Unknown refs are ignored here (reported separately by graph validation).
fn detect_cycle(
    id: SampleId,
    lib: &SampleLibrary,
    state: &mut Vec<(SampleId, Color)>,
    errors: &mut Vec<SchemaError>,
) {
    match color_of(state, id) {
        Some(Color::Black) | None => return,
        Some(Color::Gray) => {
            errors.push(SchemaError::SampleCycle { sample: id });
            return;
        }
        Some(Color::White) => {}
    }
    set_color(state, id, Color::Gray);

    if let Some(sample) = lib.sample(id) {
        for node in &sample.graph.nodes {
            if let NodeKind::Sample(sref) = &node.kind {
                detect_cycle(sref.sample, lib, state, errors);
            }
        }
    }

    set_color(state, id, Color::Black);
}
