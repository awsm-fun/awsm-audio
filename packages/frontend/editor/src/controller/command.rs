//! [`EditorCommand`] — the serializable description of every editor *document*
//! mutation.
//!
//! Every change to the saved document goes through one command, dispatched to
//! [`EditorController::dispatch`](super::EditorController::dispatch). Because the
//! enum is serde-derived, this same command stream is exactly what a future
//! MCP/websocket transport feeds in — the transport is a thin adapter over
//! `dispatch` (the [`editor_dispatch_toml`](crate::editor_dispatch_toml) seam is
//! the in-tree proof of that). Authoring a whole song over MCP is "send these
//! commands."
//!
//! What is *not* a command, by design (still all routed through the controller,
//! just not as serde document edits):
//! - **Browser-IO gestures** that carry non-serializable handles — file pickers
//!   (`load_midi_file`, `load_midi_cc`, `load_wasm_file`, buffer loads) and the
//!   microphone. An MCP host supplies asset bytes by other means.
//! - **Transient interaction** — the in-flight wire/box-drag and inspector
//!   envelope drag, which *resolve* into commands (`Connect`/`Bind`/`Modulate`/
//!   `SetAutomation`) on release.
//! - **View / navigation + transport** — which sample/view is shown, the open
//!   piano roll, play/stop/export, undo/redo. These are session state, not
//!   document content.

use awsm_audio_schema::{ControlPoint, NodeId, NodeKind, NoteEvent, SampleId, SampleKind};
use serde::{Deserialize, Serialize};

use super::node::{BoundaryPort, ConnId};
use super::Clipboard;
use crate::fields::FieldValue;

/// Adjacently tagged (`cmd` + `args`) so it round-trips through TOML/JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "cmd", content = "args")]
pub enum EditorCommand {
    /// Create a node of `kind` at world position `(x, y)` and select it.
    AddNode { kind: NodeKind, x: f64, y: f64 },
    /// Move a node to a new world position (transient — fires continuously
    /// during a drag).
    MoveNode { id: NodeId, x: f64, y: f64 },
    /// Remove a node and every wire touching it.
    RemoveNode { id: NodeId },
    /// Duplicate a node (same kind + settings) offset from the original.
    CloneNode { id: NodeId },
    /// Set one editable setting on a node (see [`crate::fields`]).
    SetField {
        id: NodeId,
        key: String,
        value: FieldValue,
    },
    /// Replace the automation timeline of a node's named AudioParam (envelope).
    SetAutomation {
        id: NodeId,
        param: String,
        events: Vec<awsm_audio_schema::AutomationEvent>,
    },
    /// Wire an output port to an input port.
    Connect {
        from: NodeId,
        from_output: u32,
        to: NodeId,
        to_input: u32,
    },
    /// Wire an output port to a node's automatable parameter (modulation).
    Modulate {
        from: NodeId,
        from_output: u32,
        to: NodeId,
        param: awsm_audio_schema::ParamId,
    },
    /// Bind a sequencer's keyed output (`from_output` = its sound/lane/zone port)
    /// to an instrument-ref's trigger inlet — a `SeqOut → Trigger` wire.
    Bind {
        from: NodeId,
        from_output: u32,
        to: NodeId,
    },
    /// Remove a single wire by its editor id.
    Disconnect { id: ConnId },
    /// Edit a Note Sequencer node's song / sound outputs (see [`SongOp`]).
    EditSong { node: NodeId, op: SongOp },
    /// Edit a Control Sequencer node's lanes / breakpoints (see [`ControlOp`]).
    EditControl { node: NodeId, op: ControlOp },
    /// Edit the active Arrangement sample's tracks / clips (see [`ArrangeOp`]).
    /// Unlike Song/Control, an arrangement isn't a canvas node — it lives on the
    /// active sample — so this op carries no node id.
    EditArrange { op: ArrangeOp },
    /// Render a Sound (`sample`) offline to a PCM buffer and store it as that
    /// sample's [`Bounce`](awsm_audio_schema::Bounce). Mutates the document (the
    /// bounce + embedded buffer), so it's a command — and MCP-drivable. The render
    /// itself is async; this kicks it off.
    Bounce { sample: awsm_audio_schema::SampleId },
    /// Replace (or, with `additive`, extend) the selection.
    SelectNodes { ids: Vec<NodeId>, additive: bool },
    /// Clear the selection.
    ClearSelection,
    /// Set the canvas camera (pan in screen px, zoom factor).
    SetCamera { pan_x: f64, pan_y: f64, zoom: f64 },

    // ---- Document structure: samples (the project's instruments/arrangements) ----
    /// Create a new empty sample of `kind` and make it active.
    AddSample { kind: SampleKind },
    /// Delete a sample (never the last one); repoints root/active if needed.
    RemoveSample { id: SampleId },
    /// Duplicate a sample (graph, trigger, arrangement, bounce) under a new id
    /// with " (clone)" appended to the name, and make the copy active.
    CloneSample { id: SampleId },
    /// Rename a sample.
    RenameSample { id: SampleId, name: String },
    /// Mark a sample as the project root (the one that plays / exports).
    SetRoot { id: SampleId },

    // ---- Canvas extras (nodes that aren't plain `AddNode` kinds) ----
    /// Add an inlet/outlet boundary node at world `(x, y)`.
    AddBoundary { port: BoundaryPort, x: f64, y: f64 },
    /// Add a Sample-reference node targeting `sample` at world `(x, y)`.
    AddSampleRef { sample: SampleId, x: f64, y: f64 },
    /// Point an existing Sample-reference node at a different sample.
    SetSampleRef { node: NodeId, sample: SampleId },
    /// Rename a node (empty label clears it back to the type name).
    RenameNode { id: NodeId, label: String },
    /// Set an inlet boundary node's default value.
    SetInputDefault { node: NodeId, value: f32 },
    /// Set (or add) a per-instance inlet override on a Sample-reference node.
    SetInputValue { node: NodeId, port: String, value: f32 },

    // ---- Scene ----
    /// Set the spatial listener position.
    SetListener { x: f32, y: f32, z: f32 },

    // ---- Composite canvas edits ----
    /// Encapsulate the given nodes into a new sub-sample, wiring a Sample-ref in
    /// their place (auto-creating inlets/outlets at the cut wires).
    Encapsulate { ids: Vec<NodeId> },
    /// Paste a clipboard payload onto the canvas (new ids, offset, selected).
    Paste { clip: Clipboard },
}

/// A clip plus the track it belongs on — the serde-friendly element of a
/// multi-clip paste (a named struct, not a tuple, so it round-trips through TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedClip {
    pub track: usize,
    pub clip: awsm_audio_schema::Clip,
}

/// A single edit to a Note Sequencer node. Sound outputs are auto-derived from
/// the song (one per melodic track / per drum note) and addressed by index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op", content = "args")]
pub enum SongOp {
    SetBpm(f64),
    SetStart(f64),
    /// Playback-window stop in beats; `None` plays to the song's end.
    SetEnd(Option<f64>),
    /// Authored grid length in beats (`0` = auto-fit content).
    SetLength(f64),
    SetLooping(bool),
    /// Append an empty track (and regenerate its sound output).
    AddTrack,
    AddNote { track: usize, event: NoteEvent },
    UpdateNote { track: usize, index: usize, event: NoteEvent },
    RemoveNote { track: usize, index: usize },
    SetOutputTranspose { index: usize, semitones: i32 },
    SetOutputGain { index: usize, gain: f32 },
    SetOutputLabel { index: usize, label: String },
}

/// A single edit to a Control Sequencer node. Lanes (and their breakpoints) are
/// addressed by index; each lane is an output wired to a parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op", content = "args")]
pub enum ControlOp {
    SetBpm(f64),
    SetStart(f64),
    SetLooping(bool),
    AddLane,
    RemoveLane { index: usize },
    SetLaneLabel { index: usize, label: String },
    AddPoint { lane: usize, beat: f64, value: f32 },
    RemovePoint { lane: usize, index: usize },
    SetPoints { lane: usize, points: Vec<ControlPoint> },
    /// Set the curve shape of the segment *reaching* point `index` from the
    /// previous point. Cycled in the lane editor; drivable for MCP.
    SetPointCurve { lane: usize, index: usize, curve: awsm_audio_schema::Curve },
}

/// A single edit to the active Arrangement. Tracks and clips are addressed by
/// index. Structural edits (add/remove/split/move) push undo; continuous drags
/// (move/resize) are transient — the UI pushes one undo on drag start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op", content = "args")]
pub enum ArrangeOp {
    SetBpm(f64),
    /// Timeline length in seconds.
    SetLengthSecs(f64),
    AddTrack,
    RemoveTrack { track: usize },
    SetTrackName { track: usize, name: String },
    SetTrackGain { track: usize, gain: f32 },
    SetTrackMute { track: usize, mute: bool },
    /// Solo a track. If any track is soloed, only soloed (non-muted) tracks play.
    SetTrackSolo { track: usize, solo: bool },
    /// Drop a bounced Sound (`source`) as a clip on `track` at `start` seconds.
    /// `length` (timeline seconds) defaults to the full bounce duration; the Draw
    /// tool passes a shorter length so a long Sound needn't fill the whole bar.
    AddClip {
        track: usize,
        start: f64,
        source: awsm_audio_schema::SampleId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<f64>,
    },
    RemoveClip { track: usize, clip: usize },
    /// Insert a fully-specified clip (paste) on `track`; `clip.start` is where it
    /// lands. Preserves source/offset/length/gain/loop/name — the copy/paste path.
    PasteClip { track: usize, clip: awsm_audio_schema::Clip },
    /// Insert several clips at once (multi-clip paste) — one undo.
    PasteClips { clips: Vec<PlacedClip> },
    /// Move a clip — possibly to another track (`new_track`) and a new `start` (s).
    MoveClip { track: usize, clip: usize, new_track: usize, start: f64 },
    /// Change a clip's timeline length in seconds (right-edge trim).
    ResizeClip { track: usize, clip: usize, length: f64 },
    /// Time-stretch: set a clip's timeline `length` and playback `speed` together
    /// (the same buffer content scaled to a new length; pitch shifts with speed).
    StretchClip { track: usize, clip: usize, length: f64, speed: f32 },
    /// Set a clip's start offset into its buffer in seconds (left-edge trim).
    SetClipOffset { track: usize, clip: usize, offset: f64 },
    /// Atomic left-edge trim: drag the clip's start later while keeping its right
    /// edge fixed. Sets `start` (timeline secs) and `offset` (into buffer) together.
    TrimStart { track: usize, clip: usize, start: f64, offset: f64 },
    /// Split a clip at `at` (timeline seconds) into two.
    SplitClip { track: usize, clip: usize, at: f64 },
    /// Set a clip's gain (linear).
    SetClipGain { track: usize, clip: usize, gain: f32 },
    /// Loop the clip's buffer to fill its length.
    SetClipLoop { track: usize, clip: usize, looping: bool },
}

impl EditorCommand {
    /// Whether `dispatch` should skip its automatic undo snapshot for this
    /// command. True for continuous/view-only gestures (move, select, camera),
    /// and for the structured `Edit{Song,Control,Live}` commands — those manage
    /// their own snapshots internally (only structural edits push undo, so a
    /// value tweak doesn't spam the stack).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            EditorCommand::MoveNode { .. }
                | EditorCommand::SelectNodes { .. }
                | EditorCommand::ClearSelection
                | EditorCommand::SetCamera { .. }
                | EditorCommand::EditSong { .. }
                | EditorCommand::EditControl { .. }
                | EditorCommand::EditArrange { .. }
                | EditorCommand::Bounce { .. }
                // Sample-list + scene state aren't in the undo snapshot (which
                // captures the active canvas), so they don't push one — but they
                // still flow through `dispatch` for MCP.
                | EditorCommand::AddSample { .. }
                | EditorCommand::RemoveSample { .. }
                | EditorCommand::CloneSample { .. }
                | EditorCommand::RenameSample { .. }
                | EditorCommand::SetRoot { .. }
                | EditorCommand::SetListener { .. }
        )
    }
}

/// The **read** half of the controller surface — the counterpart to
/// [`EditorCommand`]. A serde-tagged query an MCP/websocket transport (or a
/// headless driver) sends to inspect editor state; the controller answers with a
/// [`QueryResult`] (see [`EditorController::query`](super::EditorController::query)
/// and the [`editor_query_toml`](crate::editor_query_toml) seam).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "query", content = "args")]
pub enum EditorQuery {
    /// The full editor snapshot (graph + layout + camera + selection + arrangement).
    Snapshot,
    /// The saveable project (library + layout + camera).
    Project,
    /// Every sample (id, name, kind, root/active flags).
    Samples,
    /// Every bounceable Sound with its bounce status + bounced duration.
    Assets,
    /// One Sound's bounce status.
    BounceStatus { sample: SampleId },
    /// The active sample's arrangement (if it is one).
    Arrangement,
    /// Live transport state (playing / peak / playhead / audio-context state).
    Transport,
}

/// The answer to an [`EditorQuery`]. Serialized back to the caller.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "result", content = "data")]
pub enum QueryResult {
    Snapshot(Box<super::snapshot::EditorSnapshot>),
    Project(Box<super::snapshot::EditorProject>),
    Samples(Vec<SampleInfo>),
    Assets(Vec<AssetInfo>),
    BounceStatus(String),
    Arrangement(Option<awsm_audio_schema::Arrangement>),
    Transport(TransportInfo),
}

#[derive(Debug, Clone, Serialize)]
pub struct SampleInfo {
    pub id: SampleId,
    pub name: String,
    pub kind: SampleKind,
    pub is_root: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetInfo {
    pub id: SampleId,
    pub name: String,
    /// `"none"` / `"clean"` / `"dirty"`.
    pub bounce: String,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransportInfo {
    pub playing: bool,
    pub peak: f32,
    pub playhead: f64,
    pub audio_state: String,
}
