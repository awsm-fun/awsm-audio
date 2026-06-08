//! Node + port geometry, derived analytically from a node's [`NodeKind`].
//!
//! Port anchor positions are computed from the node's world position plus fixed
//! layout constants — never measured from the DOM — so wires stay correct under
//! any pan/zoom without a layout round-trip.

use awsm_audio_schema::{NodeKind, ParamId};

/// Node box width, in world units.
pub const NODE_WIDTH: f64 = 172.0;
/// Title-bar height.
pub const HEADER_H: f64 = 30.0;
/// Vertical stride between port rows.
pub const PORT_ROW_H: f64 = 22.0;
/// Y of the first port row's center, measured from the node's top.
pub const PORT_TOP: f64 = HEADER_H + 13.0;
/// Port dot radius.
pub const PORT_R: f64 = 6.0;

/// Which side of a node a port lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSide {
    In,
    Out,
}

/// `(inputs, outputs)` audio-port counts for a node kind. Param (modulation)
/// inlets are not surfaced yet — audio I/O only for the first cut.
pub fn port_counts(kind: &NodeKind) -> (u32, u32) {
    match kind {
        // Pure sources: no audio input.
        NodeKind::Oscillator(_)
        | NodeKind::AudioBufferSource(_)
        | NodeKind::ConstantSource(_)
        | NodeKind::Noise(_)
        | NodeKind::MediaElementSource(_)
        | NodeKind::MediaStreamSource(_) => (0, 1),

        // Single in / single out processors.
        NodeKind::Gain(_)
        | NodeKind::BiquadFilter(_)
        | NodeKind::IirFilter(_)
        | NodeKind::Delay(_)
        | NodeKind::DynamicsCompressor(_)
        | NodeKind::WaveShaper(_)
        | NodeKind::Convolver(_)
        | NodeKind::Panner(_)
        | NodeKind::StereoPanner(_)
        | NodeKind::Analyser(_) => (1, 1),

        // Routing fan-out / fan-in.
        NodeKind::ChannelSplitter(n) => (1, n.number_of_outputs.max(1)),
        NodeKind::ChannelMerger(n) => (n.number_of_inputs.max(1), 1),

        // WASM worklet: mono 1-in/1-out (v1).
        NodeKind::AudioWorklet(_) => (1, 1),

        // The output sinks: one input, no output.
        NodeKind::Output(_) | NodeKind::SpatialOutput(_) => (1, 0),

        // A referenced sub-sample: default to 1/1 until we resolve its declared
        // inlets/outlets from the library.
        NodeKind::Sample(_) => (1, 1),

        // The sequencer: no audio input; one (trigger) output per part. At least
        // one so a fresh node has a port to wire from.
        // Sequencers: no audio input; one (trigger/control) output per sound /
        // lane / zone (at least one so a fresh node is wirable).
        NodeKind::NoteSequencer(s) => (0, s.outputs.len().max(1) as u32),
        NodeKind::ControlSequencer(s) => (0, s.lanes.len().max(1) as u32),
        // A bus: sums many inputs into one output.
        NodeKind::Bus(_) => (1, 1),
    }
}

/// Left-edge port-row index for a param's modulation dot: it sits on the same
/// fixed-pitch left-edge grid as the audio inputs, on its parameter's body row
/// (after the `ins` audio inputs). `None` if the node has no such param. Both
/// the node renderer (the dot) and the wire renderer (the landing point) call
/// this, so a wire always meets its dot exactly.
pub fn param_inlet_index(kind: &NodeKind, param: &ParamId) -> Option<u32> {
    let (ins, _) = port_counts(kind);
    crate::fields::param_row_index(kind, &param.0).map(|i| ins + i as u32)
}

/// Total node height for `kind`, sized to fit its body (header + all field rows)
/// and its audio I/O ports. Used for fit/bounding-box math.
pub fn node_height(kind: &NodeKind) -> f64 {
    let (ins, outs) = port_counts(kind);
    let rows = crate::fields::fields(kind).len() as u32;
    // Body: header + top reservation for the input rows + the field rows.
    let body = HEADER_H + 2.0 + (ins.max(rows) as f64 + 1.0) * PORT_ROW_H;
    body.max(node_height_io(ins, outs, 0))
}

/// Node height from explicit port counts (for boundary / Sample-ref nodes whose
/// counts aren't derivable from the static `NodeKind`).
pub fn node_height_io(ins: u32, outs: u32, _mod_inlets: u32) -> f64 {
    // Modulation inlets now live inside the body's field rows, so only the
    // audio in/out ports drive the minimum height.
    let rows = ins.max(outs).max(1) as f64;
    PORT_TOP + (rows - 1.0) * PORT_ROW_H + 16.0
}

/// Offset of a port's center from the node's top-left origin, in world units.
pub fn port_offset(side: PortSide, index: u32) -> (f64, f64) {
    let dx = match side {
        PortSide::In => 0.0,
        PortSide::Out => NODE_WIDTH,
    };
    let dy = PORT_TOP + index as f64 * PORT_ROW_H;
    (dx, dy)
}

/// Whether a node is a sequencer (its outputs are keyed `SeqOut` bindings, not
/// audio): a Note or Control Sequencer.
pub fn is_sequencer(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::NoteSequencer(_) | NodeKind::ControlSequencer(_)
    )
}

/// The stable `SeqKey` of a sequencer's output port at `idx` (sound / lane /
/// zone), if any. The identity a `SeqOut` wire binds to (never the index).
pub fn seq_key_at(kind: &NodeKind, idx: usize) -> Option<String> {
    match kind {
        NodeKind::NoteSequencer(s) => s.outputs.get(idx).map(|o| o.key.clone()),
        NodeKind::ControlSequencer(s) => s.lanes.get(idx).map(|l| l.key.clone()),
        _ => None,
    }
}

/// The output-port index of the sequencer output whose key is `key` — the
/// inverse of [`seq_key_at`], for resolving a `SeqOut` wire back to a port.
pub fn seq_index_of(kind: &NodeKind, key: &str) -> Option<u32> {
    let pos = match kind {
        NodeKind::NoteSequencer(s) => s.outputs.iter().position(|o| o.key == key),
        NodeKind::ControlSequencer(s) => s.lanes.iter().position(|l| l.key == key),
        _ => None,
    };
    pos.map(|p| p as u32)
}

/// Human-readable node type name for the title bar + palette.
pub fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Oscillator(_) => "Oscillator",
        NodeKind::AudioBufferSource(_) => "Buffer Source",
        NodeKind::ConstantSource(_) => "Constant Source",
        NodeKind::Noise(_) => "Noise",
        NodeKind::MediaElementSource(_) => "Media Element",
        NodeKind::MediaStreamSource(_) => "Media Stream",
        NodeKind::Gain(_) => "Gain",
        NodeKind::BiquadFilter(_) => "Biquad Filter",
        NodeKind::IirFilter(_) => "IIR Filter",
        NodeKind::Delay(_) => "Delay",
        NodeKind::DynamicsCompressor(_) => "Compressor",
        NodeKind::WaveShaper(_) => "Wave Shaper",
        NodeKind::Convolver(_) => "Convolver",
        NodeKind::Panner(_) => "Panner",
        NodeKind::StereoPanner(_) => "Stereo Panner",
        NodeKind::Analyser(_) => "Analyser",
        NodeKind::ChannelSplitter(_) => "Channel Splitter",
        NodeKind::ChannelMerger(_) => "Channel Merger",
        NodeKind::AudioWorklet(_) => "Audio Worklet",
        NodeKind::Output(_) => "Output",
        NodeKind::SpatialOutput(_) => "Spatial Output",
        NodeKind::Sample(_) => "Sample",
        NodeKind::NoteSequencer(s) => {
            if s.mode.is_drum() {
                "Drum Sequencer"
            } else {
                "Melodic Sequencer"
            }
        }
        NodeKind::ControlSequencer(_) => "Control Sequencer",
        NodeKind::Bus(_) => "Bus",
    }
}
