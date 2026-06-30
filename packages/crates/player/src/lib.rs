//! awsm-audio-player — the WebAudio playback engine for the awsm-audio editor.
//!
//! [`Player`] owns the live `AudioContext` and a fixed master chain
//! (`master gain → analyser → destination`). [`Player::play`] instantiates an
//! authored [`Graph`] onto the context (see the `build` module) and routes it into the
//! master bus; [`Player::stop`] tears the instance down. The analyser exposes
//! time-domain samples for the editor's waveform view.

pub mod bounce;
mod build;
pub mod document;
mod noise;
pub mod worklet;

use std::collections::HashMap;

use anyhow::Result;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{
    AnalyserNode, AudioBuffer, AudioBufferSourceNode, AudioContext, AudioNode,
    AudioScheduledSourceNode, GainNode,
};

use awsm_audio_schema::{AssetId, Graph, Listener, NodeId};

/// Version string baked from the crate manifest (handy link-check symbol).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Owns the `AudioContext` and the persistent master chain, plus whatever graph
/// instance is currently playing.
pub struct Player {
    ctx: AudioContext,
    master: GainNode,
    analyser: AnalyserNode,
    /// Nodes of the currently-playing graph, kept alive until `stop`.
    inner: Vec<AudioNode>,
    /// Source nodes (oscillators etc.) that were `start()`ed.
    sources: Vec<AudioScheduledSourceNode>,
    /// Per-node automatable params (by WebAudio name) of the currently-playing
    /// graph, so a param can be nudged live (MIDI CC) without a full rebuild.
    params: Vec<(NodeId, Vec<(&'static str, web_sys::AudioParam)>)>,
    /// Voices materialized for a scheduled song (one per note), kept alive for
    /// the whole song; cleared on `stop`.
    song_voices: Vec<Voice>,
    /// When an arrangement is playing: every node's live `AudioNode` by id, so
    /// the trigger scheduler can spawn voices into an instrument-ref's voice-bus
    /// gain. Empty otherwise; cleared on `stop`.
    bus_nodes: Vec<(NodeId, AudioNode)>,
    /// Decoded audio buffers, keyed by the schema asset id a buffer source
    /// references. Survives play/stop so a clip only decodes once.
    buffers: HashMap<AssetId, AudioBuffer>,
    /// Compiled WASM DSP modules, keyed by the asset id an AudioWorklet node
    /// references. Survives play/stop so a module only compiles once.
    modules: HashMap<AssetId, js_sys::WebAssembly::Module>,
    /// Whether the generic `awsm-wasm` worklet shim has finished `addModule`
    /// (worklet nodes can't be constructed until it has).
    worklet_ready: bool,
    /// The captured microphone stream, if the user granted access — fed to any
    /// MediaStream source node.
    mic: Option<web_sys::MediaStream>,
    /// The spatial listener applied each play (position/orientation).
    listener: Option<Listener>,
    /// Independent concurrent voices started by [`play_voice`](Self::play_voice),
    /// each its own graph fanning into the shared master. Oldest-first, so a voice
    /// cap can steal the oldest.
    voices: Vec<GraphVoice>,
    /// Monotonic id source for [`VoiceHandle`]s.
    next_voice: u64,
    /// Optional polyphony cap: when set, starting a voice past the cap tears down
    /// the oldest one (voice stealing). `None` = unbounded.
    max_voices: Option<usize>,
}

/// An opaque handle to one concurrent voice started by
/// [`Player::play_voice`]. Pass it to [`Player::stop_voice`] /
/// [`Player::set_voice_param_live`] to address that voice specifically. Cheap to
/// copy; addressing a voice that has already been stopped is a harmless no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceHandle(u64);

/// One independent concurrent voice: a fully-built graph routed into the shared
/// master bus, kept alive until [`Player::stop_voice`] (or a global
/// [`Player::stop`] / voice-steal) tears it down.
struct GraphVoice {
    id: u64,
    inner: Vec<AudioNode>,
    sources: Vec<AudioScheduledSourceNode>,
    params: Vec<(NodeId, Vec<(&'static str, web_sys::AudioParam)>)>,
}

impl GraphVoice {
    /// Stop sources now and disconnect everything from the master bus.
    fn teardown(self) {
        for s in &self.sources {
            let _ = s.stop();
        }
        for n in &self.inner {
            let _ = n.disconnect();
        }
    }
}

/// Nudge a live param toward `value` over `glide` seconds (jump if `glide <= 0`),
/// measured from `now`. No-op if the `(node, param)` pair isn't present. Shared by
/// the main-graph and per-voice live setters.
fn nudge_param(
    params: &[(NodeId, Vec<(&'static str, web_sys::AudioParam)>)],
    node: NodeId,
    param: &str,
    value: f32,
    glide: f64,
    now: f64,
) {
    if let Some(p) = params
        .iter()
        .find(|(id, _)| *id == node)
        .and_then(|(_, ps)| ps.iter().find(|(name, _)| *name == param).map(|(_, p)| p))
    {
        if glide <= 0.0 {
            let _ = p.set_value_at_time(value, now);
        } else {
            // time-constant ≈ glide/3 → near-complete move within `glide`.
            let _ = p.set_target_at_time(value, now, glide / 3.0);
        }
    }
}

/// Apply per-trigger param overrides as each node's **base value**, before its
/// sources are started — so the very first render quantum already reflects them
/// (no one-block window at the authored defaults). Used by the `*_with` play
/// paths. A `(node, param)` not in `params` is silently skipped.
fn apply_overrides(
    params: &[(NodeId, Vec<(&'static str, web_sys::AudioParam)>)],
    overrides: &[(NodeId, &str, f32)],
) {
    for (node, name, value) in overrides {
        if let Some(p) = params
            .iter()
            .find(|(id, _)| id == node)
            .and_then(|(_, ps)| ps.iter().find(|(n, _)| n == name).map(|(_, p)| p))
        {
            p.set_value(*value);
        }
    }
}

/// Upper bound on simultaneously-scheduled song notes — a backstop against
/// pathological MIDI files. Excess notes are dropped (the caller logs it).
const MAX_SONG_VOICES: usize = 4096;

/// One sound's worth of triggered notes within an arrangement: the instrument
/// to instantiate, the arrangement node whose voice-bus its voices feed, and the
/// notes (already resolved to seconds + transpose + gain).
pub struct TriggerPart {
    /// The arrangement node id (an instrument-ref) whose voice-bus gain receives
    /// this part's voices. Its audio then flows on through the arrangement graph.
    pub target: NodeId,
    /// The flattened instrument sample, instantiated once per note.
    pub instrument: Graph,
    pub notes: Vec<SongVoiceSpec>,
}

/// One control lane to automate: a target node's AudioParam plus its breakpoints
/// (already resolved to seconds-from-start + absolute value + the curve reaching
/// each point from the previous one).
pub struct ControlLanePart {
    pub target: NodeId,
    pub param: String,
    pub points: Vec<(f64, f32, awsm_audio_schema::Curve)>,
}

/// One bounced audio clip to schedule on the arrangement timeline. Times are in
/// seconds; `start` is relative to the playback origin (the controller applies the
/// scrub seek), `offset` is into the buffer, `length` is how long to play.
pub struct AudioClipPart {
    pub buffer: AssetId,
    pub start: f64,
    pub offset: f64,
    pub length: f64,
    pub gain: f32,
    /// Gain automation points relative to this clip part's audible start:
    /// `(seconds_from_part_start, absolute_linear_gain)`.
    pub gain_curve: Vec<(f64, f32)>,
    pub looping: bool,
    /// Playback rate (1.0 = normal). The clip occupies `length` seconds on the
    /// timeline but consumes `length * speed` seconds of buffer.
    pub speed: f64,
}

/// One scheduled note within a [`TriggerPart`].
pub struct SongVoiceSpec {
    /// Onset, in seconds from the song's (seek-adjusted) start.
    pub start: f64,
    /// Note-off, in seconds (release tail extends past this).
    pub end: f64,
    /// Semitone transpose of the instrument for this note (60 = unison → 0).
    pub semitones: i32,
    /// Linear amplitude (velocity × part gain), 0..=1.
    pub velocity: f32,
}

/// One sounding polyphonic voice: an independent instance of the patch routed
/// through its own `gain` (velocity + release envelope) into the master bus.
struct Voice {
    gain: GainNode,
    /// All inner nodes, kept alive while the voice sounds.
    nodes: Vec<AudioNode>,
    sources: Vec<AudioScheduledSourceNode>,
    /// When the sources are scheduled to stop.
    stop_at: f64,
}

impl Voice {
    /// Stop sources now and disconnect everything from the graph.
    fn teardown(self) {
        for s in &self.sources {
            let _ = s.stop();
        }
        for n in &self.nodes {
            let _ = n.disconnect();
        }
        let _ = self.gain.disconnect();
    }
}

/// Spawn a voice per note of each [`TriggerPart`] into its bus node, on any
/// context (live or offline). `t0` is the absolute start time; voices are pushed
/// to `out` (kept alive by the caller). Returns the latest stop time. Shared by
/// the live scheduler and the offline bounce renderer.
#[allow(clippy::too_many_arguments)]
fn spawn_voices(
    ctx: &web_sys::BaseAudioContext,
    bus_nodes: &[(NodeId, AudioNode)],
    buffers: &HashMap<AssetId, AudioBuffer>,
    modules: &HashMap<AssetId, js_sys::WebAssembly::Module>,
    worklet_ready: bool,
    mic: Option<&web_sys::MediaStream>,
    parts: &[TriggerPart],
    t0: f64,
    out: &mut Vec<Voice>,
    room: usize,
) -> Result<f64> {
    const ATTACK: f64 = 0.004;
    const RELEASE: f64 = 0.08;
    let mut end_time = t0;
    'outer: for part in parts {
        let Some(target) = bus_nodes
            .iter()
            .find(|(id, _)| *id == part.target)
            .map(|(_, n)| n.clone())
        else {
            continue;
        };
        for note in &part.notes {
            if out.len() >= room {
                break 'outer;
            }
            let on = t0 + note.start;
            let off = t0 + note.end.max(note.start);
            let gain = ctx
                .create_gain()
                .map_err(|e| anyhow::anyhow!("song gain: {e:?}"))?;
            let g = gain.gain();
            let _ = g.set_value_at_time(0.0, on);
            let _ = g.set_target_at_time(note.velocity.clamp(0.0, 1.0), on, ATTACK);
            let _ = g.set_target_at_time(0.0, off, RELEASE / 3.0);
            gain.connect_with_audio_node(&target)
                .map_err(|e| anyhow::anyhow!("song voice→bus: {e:?}"))?;
            let graph = part.instrument.transposed(note.semitones);
            let built = build::build_graph(
                ctx,
                &graph,
                &gain,
                buffers,
                modules,
                mic,
                worklet_ready,
                false,
                on,
            )?;
            let stop_at = off + RELEASE * 3.0;
            end_time = end_time.max(stop_at);
            for s in &built.sources {
                let _ = s.start_with_when(on);
                let _ = s.stop_with_when(stop_at);
            }
            out.push(Voice {
                gain,
                nodes: built.inner,
                sources: built.sources,
                stop_at,
            });
        }
    }
    Ok(end_time)
}

/// Apply control-lane automation onto already-built params, on any context.
/// `at` is the absolute start time. Shared by the live scheduler and bounce.
fn apply_control(
    params: &[(NodeId, Vec<(&'static str, web_sys::AudioParam)>)],
    parts: &[ControlLanePart],
    at: f64,
) {
    use awsm_audio_schema::Curve;
    const EPS: f32 = 1e-4;
    for part in parts {
        let Some(param) = params
            .iter()
            .find(|(id, _)| *id == part.target)
            .and_then(|(_, ps)| {
                ps.iter()
                    .find(|(n, _)| *n == part.param)
                    .map(|(_, p)| p.clone())
            })
        else {
            continue;
        };
        let mut pts = part.points.clone();
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut prev: Option<(f64, f32)> = None;
        for (i, (secs, value, curve)) in pts.iter().enumerate() {
            let t = at + secs.max(0.0);
            let v = *value;
            if i == 0 {
                let _ = param.set_value_at_time(v, t);
                prev = Some((t, v));
                continue;
            }
            match curve {
                Curve::Step => {
                    let _ = param.set_value_at_time(v, t);
                }
                Curve::Linear => {
                    let _ = param.linear_ramp_to_value_at_time(v, t);
                }
                Curve::Exponential => {
                    if let Some((pt, pv)) = prev {
                        if pv.abs() < EPS {
                            let _ = param.set_value_at_time(EPS, pt);
                        }
                    }
                    let target = if v.abs() < EPS { EPS } else { v };
                    let _ = param.exponential_ramp_to_value_at_time(target, t);
                }
                Curve::Smooth => {
                    if let Some((pt, pv)) = prev {
                        const N: usize = 24;
                        let mut curve_vals = vec![0.0f32; N];
                        for (k, slot) in curve_vals.iter_mut().enumerate() {
                            let x = k as f32 / (N - 1) as f32;
                            let s = x * x * (3.0 - 2.0 * x);
                            *slot = pv + (v - pv) * s;
                        }
                        let dur = (t - pt).max(0.001);
                        let _ = param.set_value_curve_at_time(&mut curve_vals, pt, dur);
                    } else {
                        let _ = param.linear_ramp_to_value_at_time(v, t);
                    }
                }
            }
            prev = Some((t, v));
        }
    }
}

fn apply_clip_gain_curve(
    param: &web_sys::AudioParam,
    fallback: f32,
    points: &[(f64, f32)],
    at: f64,
) {
    if points.is_empty() {
        param.set_value(fallback);
        return;
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let (t0, v0) = pts[0];
    let _ = param.set_value_at_time(v0, at + t0.max(0.0));
    for (secs, gain) in pts.into_iter().skip(1) {
        let _ = param.linear_ramp_to_value_at_time(gain, at + secs.max(0.0));
    }
}

impl Player {
    /// Create a player with `master → analyser → destination` wired up. The
    /// context starts suspended until [`play`](Self::play) resumes it (a click
    /// satisfies the browser's gesture requirement).
    pub fn new() -> Result<Self> {
        let ctx = AudioContext::new().map_err(|e| anyhow::anyhow!("AudioContext: {e:?}"))?;
        Self::with_context(ctx)
    }

    /// Create a player on a **host-supplied** `AudioContext` instead of
    /// constructing its own. Use this to (a) build the context yourself inside a
    /// real user-gesture handler (then `resume()` the one you own before the first
    /// `play`), or (b) share one context across several systems — every
    /// [`Player`] on the same context shares its clock, its
    /// [`AudioListener`](Self::set_listener_live), and the autoplay-gesture state.
    ///
    /// The player still owns its own `master → analyser → destination` chain on
    /// that context (so multiple players can coexist on one context, each a
    /// separate sub-mix), but it does **not** close the context on drop — the host
    /// owns its lifetime.
    pub fn with_context(ctx: AudioContext) -> Result<Self> {
        let master = ctx
            .create_gain()
            .map_err(|e| anyhow::anyhow!("master gain: {e:?}"))?;
        let analyser = ctx
            .create_analyser()
            .map_err(|e| anyhow::anyhow!("analyser: {e:?}"))?;
        analyser.set_fft_size(2048);
        master
            .connect_with_audio_node(&analyser)
            .map_err(|e| anyhow::anyhow!("master→analyser: {e:?}"))?;
        analyser
            .connect_with_audio_node(&ctx.destination())
            .map_err(|e| anyhow::anyhow!("analyser→destination: {e:?}"))?;
        Ok(Self {
            ctx,
            master,
            analyser,
            inner: Vec::new(),
            sources: Vec::new(),
            params: Vec::new(),
            song_voices: Vec::new(),
            bus_nodes: Vec::new(),
            buffers: HashMap::new(),
            modules: HashMap::new(),
            worklet_ready: false,
            mic: None,
            listener: None,
            voices: Vec::new(),
            next_voice: 0,
            max_voices: None,
        })
    }

    /// The underlying `AudioContext` — for sharing it with another [`Player`]
    /// (via [`with_context`](Self::with_context)) or another audio system, or to
    /// `resume()` it from a gesture handler before the first play.
    pub fn context(&self) -> &AudioContext {
        &self.ctx
    }

    /// Begin loading the generic WASM worklet shim into this context (idempotent
    /// once ready). Returns the `addModule` promise; await it, then call
    /// [`mark_worklet_ready`](Self::mark_worklet_ready). Done as a Blob URL so no
    /// static file needs serving.
    pub fn add_worklet_shim(&self) -> Result<js_sys::Promise> {
        let parts = js_sys::Array::new();
        parts.push(&JsValue::from_str(&worklet::shim_source()));
        let bag = web_sys::BlobPropertyBag::new();
        bag.set_type("text/javascript");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &bag)
            .map_err(|e| anyhow::anyhow!("blob: {e:?}"))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|e| anyhow::anyhow!("blob url: {e:?}"))?;
        let wl = self
            .ctx
            .audio_worklet()
            .map_err(|e| anyhow::anyhow!("audioWorklet: {e:?}"))?;
        wl.add_module(&url)
            .map_err(|e| anyhow::anyhow!("addModule: {e:?}"))
    }

    /// Mark the worklet shim ready (after its `addModule` promise resolved).
    pub fn mark_worklet_ready(&mut self) {
        self.worklet_ready = true;
    }

    /// Whether the worklet shim is loaded.
    pub fn worklet_ready(&self) -> bool {
        self.worklet_ready
    }

    /// Compile raw `.wasm` bytes into a `WebAssembly.Module`. Returns the
    /// `WebAssembly.compile` promise (resolves to the module).
    pub fn compile_module(bytes: &js_sys::Uint8Array) -> js_sys::Promise {
        js_sys::WebAssembly::compile(bytes.as_ref())
    }

    /// Register a compiled module under `id` (referenced by an AudioWorklet node).
    pub fn store_module(&mut self, id: AssetId, module: js_sys::WebAssembly::Module) {
        self.modules.insert(id, module);
    }

    /// Whether a compiled module is registered for `id`.
    pub fn has_module(&self, id: &AssetId) -> bool {
        self.modules.contains_key(id)
    }

    /// Decode encoded audio (mp3/wav/flac/…) into an `AudioBuffer` via the
    /// context. Returns the `decodeAudioData` promise for the caller to await.
    pub fn decode(&self, data: &js_sys::ArrayBuffer) -> Result<js_sys::Promise> {
        self.ctx
            .decode_audio_data(data)
            .map_err(|e| anyhow::anyhow!("decodeAudioData: {e:?}"))
    }

    /// Register a decoded buffer under `id` (referenced by a buffer-source node).
    pub fn store_buffer(&mut self, id: AssetId, buffer: AudioBuffer) {
        self.buffers.insert(id, buffer);
    }

    /// Build an `AudioBuffer` from raw PCM (one `Vec<f32>` per channel) and
    /// register it under `id`.
    pub fn store_pcm(
        &mut self,
        id: AssetId,
        sample_rate: f32,
        channels: &[Vec<f32>],
    ) -> Result<()> {
        let ch = channels.len().max(1) as u32;
        let len = channels.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32;
        let buffer = self
            .ctx
            .create_buffer(ch, len, sample_rate)
            .map_err(|e| anyhow::anyhow!("create_buffer: {e:?}"))?;
        for (i, data) in channels.iter().enumerate() {
            // Via an owned Float32Array — a shared-memory (wasm-threads) build
            // otherwise hands `copyToChannel` a `SharedArrayBuffer`-backed view,
            // which WebAudio rejects. See [`build::copy_to_channel`].
            build::copy_to_channel(&buffer, data, i as i32)
                .map_err(|e| anyhow::anyhow!("copy_to_channel: {e:?}"))?;
        }
        self.buffers.insert(id, buffer);
        Ok(())
    }

    /// Begin a `getUserMedia({audio:true})` request; returns the promise
    /// (resolves to a `MediaStream`). The caller awaits + [`set_mic`](Self::set_mic).
    pub fn request_mic(&self) -> Result<js_sys::Promise> {
        let nav = web_sys::window()
            .ok_or_else(|| anyhow::anyhow!("no window"))?
            .navigator();
        let devices = nav
            .media_devices()
            .map_err(|e| anyhow::anyhow!("mediaDevices: {e:?}"))?;
        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_audio(&JsValue::TRUE);
        devices
            .get_user_media_with_constraints(&constraints)
            .map_err(|e| anyhow::anyhow!("getUserMedia: {e:?}"))
    }

    /// Store the captured microphone stream (fed to MediaStream source nodes).
    pub fn set_mic(&mut self, stream: web_sys::MediaStream) {
        self.mic = Some(stream);
    }

    /// Set the spatial listener applied on each play/render. Takes effect at the
    /// next `play`/render; for a per-frame update of an already-playing scene use
    /// [`set_listener_live`](Self::set_listener_live).
    pub fn set_listener(&mut self, listener: Option<Listener>) {
        self.listener = listener;
    }

    /// Write the context `AudioListener`'s position + orientation **immediately**,
    /// without re-triggering any sound — the mirror of
    /// [`set_param_live`](Self::set_param_live) for the ears. Call it each frame to
    /// keep a camera-relative listener following the camera while sounds keep
    /// ringing. Also updates the stored listener so subsequent plays start from
    /// the same pose.
    ///
    /// (The write is immediate, not glided: stable web-sys exposes the listener's
    /// older `setPosition`/`setOrientation` setters rather than its per-axis
    /// `AudioParam`s, so there's no `setTargetAtTime` glide path yet. Call it at
    /// frame rate and the motion is already smooth.)
    pub fn set_listener_live(&mut self, listener: &Listener) {
        build::apply_listener(&self.ctx, listener, self.ctx.current_time());
        self.listener = Some(listener.clone());
    }

    /// Set the persistent master-bus gain (0..1+), live. Used for MIDI velocity
    /// sensitivity — it survives `play`/`stop` since the master chain is fixed.
    pub fn set_master_gain(&self, gain: f32) {
        self.master.gain().set_value(gain);
    }

    /// Tear down any running instance, build `graph`, route its terminals to the
    /// master bus, start every source, and resume the context.
    pub fn play(&mut self, graph: &Graph, looping: bool) -> Result<()> {
        self.play_with(graph, looping, &[])
    }

    /// As [`play`](Self::play), but apply per-trigger param `overrides` —
    /// `(node, param_name, value)` — as the graph's **base values at build time**,
    /// before any source starts. This makes a one-shot correct from sample 0
    /// (level/cutoff/pan set to where the trigger happened) instead of starting at
    /// the authored defaults and only catching up a render-quantum later via
    /// [`set_param_live`](Self::set_param_live).
    ///
    /// Overrides set the param's intrinsic value, so they're the right tool for
    /// the per-trigger statics — impact intensity → a gain, hardness → a filter
    /// cutoff, world position → a panner's `positionX/Y/Z`. A param that also
    /// carries its own scheduled automation still follows that automation.
    ///
    /// ```no_run
    /// # use awsm_audio_player::Player;
    /// # use awsm_audio_schema::{Graph, NodeId};
    /// # fn demo(player: &mut Player, graph: &Graph, level: NodeId, panner: NodeId) -> anyhow::Result<()> {
    /// player.play_with(graph, false, &[
    ///     (level, "gain", 0.8),
    ///     (panner, "positionX", 3.0),
    ///     (panner, "positionZ", -1.0),
    /// ])?;
    /// # Ok(()) }
    /// ```
    pub fn play_with(
        &mut self,
        graph: &Graph,
        looping: bool,
        overrides: &[(NodeId, &str, f32)],
    ) -> Result<()> {
        self.stop();
        // Note-on time: automation in the graph is scheduled relative to this.
        let t0 = self.ctx.current_time();
        let built = build::build_graph(
            &self.ctx,
            graph,
            &self.master,
            &self.buffers,
            &self.modules,
            self.mic.as_ref(),
            self.worklet_ready,
            looping,
            t0,
        )?;
        apply_overrides(&built.params, overrides);
        self.inner = built.inner;
        self.sources = built.sources;
        self.params = built.params;
        // Keep the id→node map so per-node Analyser scopes can read their data.
        self.bus_nodes = built.nodes;
        if let Some(l) = &self.listener {
            build::apply_listener(&self.ctx, l, t0);
        }
        for s in &self.sources {
            // A source can only be started once; these are freshly built.
            let _ = s.start();
        }
        let _ = self.ctx.resume();
        Ok(())
    }

    /// Resume the audio context — call it from a user-gesture handler (click /
    /// keypress) to satisfy the browser's autoplay policy before/at the first
    /// [`play`](Self::play). Idempotent; harmless once running.
    pub fn resume(&self) {
        let _ = self.ctx.resume();
    }

    /// Time-domain samples (0..255, 128 = silence) of the Analyser node `id` in
    /// the live graph — for a per-node oscilloscope. Empty if `id` isn't a live
    /// Analyser.
    pub fn scope(&self, id: NodeId) -> Vec<u8> {
        let Some((_, node)) = self.bus_nodes.iter().find(|(n, _)| *n == id) else {
            return Vec::new();
        };
        if let Some(an) = node.dyn_ref::<AnalyserNode>() {
            let mut buf = vec![0u8; an.fft_size() as usize];
            an.get_byte_time_domain_data(&mut buf);
            buf
        } else {
            Vec::new()
        }
    }

    /// Stop and disconnect **everything** (the master chain stays intact): the
    /// current instance, every scheduled song voice, and every concurrent
    /// [`play_voice`](Self::play_voice) voice. To stop a single concurrent voice,
    /// use [`stop_voice`](Self::stop_voice) instead.
    pub fn stop(&mut self) {
        for s in self.sources.drain(..) {
            let _ = s.stop();
        }
        for n in self.inner.drain(..) {
            let _ = n.disconnect();
        }
        self.params.clear();
        self.bus_nodes.clear();
        for v in self.song_voices.drain(..) {
            v.teardown();
        }
        self.stop_all_voices();
    }

    /// The audio context's current time (seconds) — the clock the song scheduler
    /// and loop re-arm measure against.
    pub fn current_time(&self) -> f64 {
        self.ctx.current_time()
    }

    /// The context sample rate (Hz).
    pub fn sample_rate(&self) -> u32 {
        self.ctx.sample_rate() as u32
    }

    /// A clone of the decoded/rendered buffer registry (AudioBuffers are
    /// context-independent), for handing to the offline arrangement renderer
    /// ([`bounce::render_clips`]).
    pub fn clip_buffers(&self) -> std::collections::HashMap<AssetId, AudioBuffer> {
        self.buffers.clone()
    }

    /// Whether a decoded/rendered buffer is registered under `id`.
    pub fn has_buffer(&self, id: AssetId) -> bool {
        self.buffers.contains_key(&id)
    }

    /// Assemble an offline [`bounce`] job from the live state. Clones the buffer
    /// and module registries so the returned future owns everything (no borrow of
    /// the player across `await`). `await crate::bounce::render(job)` to get PCM.
    pub fn bounce_job(
        &self,
        graph: Graph,
        parts: Vec<TriggerPart>,
        control: Vec<ControlLanePart>,
        duration_secs: f64,
        loop_secs: Option<f64>,
    ) -> bounce::BounceJob {
        bounce::BounceJob {
            graph,
            parts,
            control,
            duration_secs,
            loop_secs,
            sample_rate: self.ctx.sample_rate(),
            buffers: self.buffers.clone(),
            modules: self.modules.clone(),
            shim_source: worklet::shim_source(),
        }
    }

    /// Begin an audio-clip arrangement: tear down any prior instance and resume.
    pub fn arrange_audio_begin(&mut self) {
        self.stop();
        let _ = self.ctx.resume();
    }

    /// Schedule one pass of audio clips at absolute time `at` (additive — the
    /// transport-loop re-arm calls this again for the next pass). Reclaims
    /// finished sources first. Returns the latest end time.
    pub fn schedule_audio_clips(&mut self, clips: &[AudioClipPart], at: f64) -> Result<f64> {
        let now = self.ctx.current_time();
        let mut i = 0;
        while i < self.song_voices.len() {
            if self.song_voices[i].stop_at <= now {
                self.song_voices.swap_remove(i).teardown();
            } else {
                i += 1;
            }
        }
        let mut end = at;
        for c in clips {
            let Some(buf) = self.buffers.get(&c.buffer).cloned() else {
                continue;
            };
            let when = at + c.start.max(0.0);
            let dur = c.length.max(0.0);
            let off = c.offset.max(0.0);
            let speed = if c.speed > 0.0 { c.speed } else { 1.0 };
            if dur <= 0.0 {
                continue;
            }
            let buf_dur = buf.duration();
            // Buffer seconds consumed = timeline length × speed.
            let span = dur * speed;
            let stretched = c.looping && span > (buf_dur - off) + 1e-3;

            let (src, g) = self.new_clip_source(&buf)?;
            apply_clip_gain_curve(&g.gain(), c.gain, &c.gain_curve, when);
            if (speed - 1.0).abs() > 1e-6 {
                src.playback_rate().set_value(speed as f32);
            }
            let sched: AudioScheduledSourceNode = src.clone().unchecked_into();
            if stretched {
                // Native loop. The bounce is rendered as an exact loop region
                // (with its wrap-around tail folded back onto the start), so the
                // seam is seamless without any crossfade. Playback rate scales it.
                src.set_loop(true);
                src.set_loop_start(off);
                src.set_loop_end(buf_dur);
                let _ = src.start_with_when_and_grain_offset(when, off);
                let _ = sched.stop_with_when(when + dur);
            } else {
                // grain_duration is in buffer seconds (`span`); at `speed` it plays
                // for `dur` real seconds.
                let _ = src.start_with_when_and_grain_offset_and_grain_duration(when, off, span);
            }
            let stop_at = when + dur + 0.05;
            end = end.max(stop_at);
            self.song_voices.push(Voice {
                gain: g,
                nodes: Vec::new(),
                sources: vec![sched],
                stop_at,
            });
        }
        let _ = self.ctx.resume();
        Ok(end)
    }

    /// Create a clip buffer source wired `source → gain → master` (gain left at
    /// its default 1.0 for the caller to set or automate).
    fn new_clip_source(&self, buf: &AudioBuffer) -> Result<(AudioBufferSourceNode, GainNode)> {
        let src = self
            .ctx
            .create_buffer_source()
            .map_err(|e| anyhow::anyhow!("buffer source: {e:?}"))?;
        src.set_buffer(Some(buf));
        let g = self
            .ctx
            .create_gain()
            .map_err(|e| anyhow::anyhow!("clip gain: {e:?}"))?;
        src.connect_with_audio_node(&g)
            .map_err(|e| anyhow::anyhow!("clip src→gain: {e:?}"))?;
        g.connect_with_audio_node(&self.master)
            .map_err(|e| anyhow::anyhow!("clip gain→master: {e:?}"))?;
        Ok((src, g))
    }

    /// Build an **arrangement** graph as the persistent instance and route it to
    /// the master bus. Unlike [`play`](Self::play), this keeps the per-node map so
    /// [`schedule_triggers`](Self::schedule_triggers) can spawn voices into an
    /// instrument-ref's voice-bus gain. Tears down any previous instance first.
    pub fn play_arrangement(&mut self, arrangement: &Graph, looping: bool) -> Result<()> {
        self.play_arrangement_with(arrangement, looping, &[])
    }

    /// As [`play_arrangement`](Self::play_arrangement), but apply per-trigger param
    /// `overrides` as base values before the sources start (see
    /// [`play_with`](Self::play_with)).
    pub fn play_arrangement_with(
        &mut self,
        arrangement: &Graph,
        looping: bool,
        overrides: &[(NodeId, &str, f32)],
    ) -> Result<()> {
        self.stop();
        let t0 = self.ctx.current_time();
        let built = build::build_graph(
            &self.ctx,
            arrangement,
            &self.master,
            &self.buffers,
            &self.modules,
            self.mic.as_ref(),
            self.worklet_ready,
            looping,
            t0,
        )?;
        apply_overrides(&built.params, overrides);
        self.bus_nodes = built.nodes;
        self.inner = built.inner;
        self.sources = built.sources;
        self.params = built.params;
        if let Some(l) = &self.listener {
            build::apply_listener(&self.ctx, l, t0);
        }
        for s in &self.sources {
            let _ = s.start();
        }
        let _ = self.ctx.resume();
        Ok(())
    }

    /// Schedule one pass of an arrangement's triggered notes starting at absolute
    /// context time `at`. Each [`TriggerPart`] spawns a voice per note — an
    /// instance of its instrument graph — feeding the part's target voice-bus gain
    /// (found in the arrangement built by [`play_arrangement`](Self::play_arrangement)), whose audio then
    /// flows through the arrangement to the Output. Scheduled on WebAudio's
    /// sample-accurate clock; finished voices are reclaimed first; capped at
    /// `MAX_SONG_VOICES`. Returns `(scheduled, end_time)`.
    pub fn schedule_triggers(&mut self, parts: &[TriggerPart], at: f64) -> Result<(usize, f64)> {
        // Reclaim song voices that have already finished, so a long loop doesn't
        // accumulate dead nodes.
        let now = self.ctx.current_time();
        let mut i = 0;
        while i < self.song_voices.len() {
            if self.song_voices[i].stop_at <= now {
                self.song_voices.swap_remove(i).teardown();
            } else {
                i += 1;
            }
        }

        let before = self.song_voices.len();
        let end_time = spawn_voices(
            self.ctx.as_ref(),
            &self.bus_nodes,
            &self.buffers,
            &self.modules,
            self.worklet_ready,
            self.mic.as_ref(),
            parts,
            at,
            &mut self.song_voices,
            MAX_SONG_VOICES,
        )?;
        let count = self.song_voices.len() - before;
        let _ = self.ctx.resume();
        Ok((count, end_time))
    }

    /// Apply a pass of control-lane automation to the live arrangement starting at
    /// absolute context time `at`. Each [`ControlLanePart`] targets a node's
    /// AudioParam (resolved from the arrangement built by `play_arrangement`) and
    /// writes its points as a `setValueAtTime` anchor plus per-segment curves
    /// (step / linear / exponential / smooth) over playback.
    pub fn schedule_control(&self, parts: &[ControlLanePart], at: f64) {
        apply_control(&self.params, parts, at);
    }

    /// Nudge a live AudioParam toward `value` while audio keeps playing — gliding
    /// over ~`glide` seconds (`setTargetAtTime`, so sweeps are smooth and
    /// click-free; pass `glide <= 0.0` to jump). No rebuild, so a held note / a
    /// running drone keeps sounding. No-op where the node/param isn't present.
    ///
    /// This is the hook for **driving a playing sound from live application state**
    /// — move a sound in 3D from a game entity's position, bend an oscillator's
    /// pitch from a gauge, open a filter as something charges up. `node` is the
    /// [`NodeId`] from the document; `param` is the WebAudio param name. Call
    /// [`live_params`](Self::live_params) to discover the exact `(node, param)`
    /// pairs currently controllable (or pick a node out of the document's graph by
    /// kind).
    ///
    /// Controllable params by node kind:
    /// - **Oscillator** — `"frequency"`, `"detune"`
    /// - **Gain** — `"gain"`
    /// - **BiquadFilter** — `"frequency"`, `"detune"`, `"Q"`, `"gain"`
    /// - **Panner / SpatialOutput** — `"positionX"`, `"positionY"`, `"positionZ"`
    ///   (SpatialOutput also `"gain"`); for the *listener*, use
    ///   [`set_listener`](Self::set_listener)
    /// - **AudioBufferSource** — `"playbackRate"`, `"detune"`
    /// - **AudioWorklet** — every declared param, by its name
    ///
    /// ```no_run
    /// # use awsm_audio_player::Player;
    /// # use awsm_audio_schema::{NodeKind, SampleLibrary, SampleId};
    /// # fn demo(player: &Player, lib: &SampleLibrary, sample: SampleId) {
    /// // Find the spatial output node in the played sample, then steer it each frame.
    /// if let Some(out) = lib.sample(sample).and_then(|s|
    ///     s.graph.nodes.iter().find(|n| matches!(n.kind, NodeKind::SpatialOutput(_))))
    /// {
    ///     player.set_param_live(out.id, "positionX", 3.0, 0.05); // glide to x=3
    /// }
    /// # }
    /// ```
    pub fn set_param_live(&self, node: NodeId, param: &str, value: f32, glide: f64) {
        nudge_param(
            &self.params,
            node,
            param,
            value,
            glide,
            self.ctx.current_time(),
        );
    }

    // ─── Concurrent voices ───────────────────────────────────────────────────
    //
    // A `Player` owns one master chain (`master → analyser → destination`) and
    // *one* "current" instance driven by `play`/`play_arrangement`. Voices add
    // **polyphony on that same master**: any number of independent graphs sounding
    // at once — a sustaining drone *and* transient one-shots — without cutting
    // each other off and without a second `AudioContext` per sound. This is what
    // lets a game keep a single context (one listener, one master, one
    // gesture-resume) instead of hitting the browser's ~6-context ceiling.

    /// Set the polyphony cap for concurrent voices. When more than `n` voices are
    /// sounding, starting another tears down the **oldest** (voice stealing).
    /// `None` (the default) is unbounded.
    pub fn set_max_voices(&mut self, n: Option<usize>) {
        self.max_voices = n;
        self.enforce_voice_cap();
    }

    /// Start `graph` as an **independent concurrent voice** fanning into the shared
    /// master — *without* stopping the current instance or any other voice — and
    /// return a [`VoiceHandle`] to stop or steer it. This is the building block for
    /// game audio: one `Player`, one context, many simultaneous sounds.
    ///
    /// The voice runs until you [`stop_voice`](Self::stop_voice) it, until a global
    /// [`stop`](Self::stop), or until it's stolen by the
    /// [`max_voices`](Self::set_max_voices) cap. A self-decaying one-shot goes
    /// silent on its own but keeps its (idle) nodes until one of those — so free it
    /// when it's done (e.g. drive a timer off [`measure_sound`](Self::measure_sound)).
    pub fn play_voice(&mut self, graph: &Graph, looping: bool) -> Result<VoiceHandle> {
        self.play_voice_with(graph, looping, &[])
    }

    /// As [`play_voice`](Self::play_voice), but apply per-trigger param `overrides`
    /// as base values before the voice's sources start (see
    /// [`play_with`](Self::play_with)) — so a spatial one-shot pans to where it
    /// happened on its very first sample.
    pub fn play_voice_with(
        &mut self,
        graph: &Graph,
        looping: bool,
        overrides: &[(NodeId, &str, f32)],
    ) -> Result<VoiceHandle> {
        let t0 = self.ctx.current_time();
        let built = build::build_graph(
            &self.ctx,
            graph,
            &self.master,
            &self.buffers,
            &self.modules,
            self.mic.as_ref(),
            self.worklet_ready,
            looping,
            t0,
        )?;
        apply_overrides(&built.params, overrides);
        if let Some(l) = &self.listener {
            build::apply_listener(&self.ctx, l, t0);
        }
        for s in &built.sources {
            let _ = s.start();
        }
        let id = self.next_voice;
        self.next_voice = self.next_voice.wrapping_add(1);
        self.voices.push(GraphVoice {
            id,
            inner: built.inner,
            sources: built.sources,
            params: built.params,
        });
        self.enforce_voice_cap();
        let _ = self.ctx.resume();
        Ok(VoiceHandle(id))
    }

    /// Stop and tear down one concurrent voice. No-op if the handle is unknown
    /// (already stopped / stolen).
    pub fn stop_voice(&mut self, handle: VoiceHandle) {
        if let Some(i) = self.voices.iter().position(|v| v.id == handle.0) {
            self.voices.remove(i).teardown();
        }
    }

    /// Stop and tear down every concurrent voice (leaves the current
    /// `play`/`play_arrangement` instance untouched).
    pub fn stop_all_voices(&mut self) {
        for v in self.voices.drain(..) {
            v.teardown();
        }
    }

    /// Whether a voice handle is still sounding (present in the voice pool).
    pub fn voice_alive(&self, handle: VoiceHandle) -> bool {
        self.voices.iter().any(|v| v.id == handle.0)
    }

    /// Number of concurrent voices currently alive.
    pub fn active_voices(&self) -> usize {
        self.voices.len()
    }

    /// Nudge a live param of one specific concurrent `voice` — the per-voice mirror
    /// of [`set_param_live`](Self::set_param_live). Needed because the same
    /// [`NodeId`] can recur across polyphonic voices instantiated from the same
    /// graph; the handle disambiguates which voice to steer. No-op if the voice or
    /// `(node, param)` isn't present.
    pub fn set_voice_param_live(
        &self,
        voice: VoiceHandle,
        node: NodeId,
        param: &str,
        value: f32,
        glide: f64,
    ) {
        if let Some(v) = self.voices.iter().find(|v| v.id == voice.0) {
            nudge_param(
                &v.params,
                node,
                param,
                value,
                glide,
                self.ctx.current_time(),
            );
        }
    }

    /// The controllable `(node, [param names])` of one concurrent voice — the
    /// per-voice form of [`live_params`](Self::live_params). Empty if the voice
    /// isn't present.
    pub fn voice_live_params(&self, voice: VoiceHandle) -> Vec<(NodeId, Vec<&'static str>)> {
        self.voices
            .iter()
            .find(|v| v.id == voice.0)
            .map(|v| {
                v.params
                    .iter()
                    .map(|(id, ps)| (*id, ps.iter().map(|(name, _)| *name).collect()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Enforce [`max_voices`](Self::set_max_voices) by stealing oldest-first.
    fn enforce_voice_cap(&mut self) {
        if let Some(max) = self.max_voices {
            while self.voices.len() > max {
                self.voices.remove(0).teardown();
            }
        }
    }

    /// Every live, controllable `(node, [param names])` in the currently-playing
    /// graph — exactly the targets [`set_param_live`](Self::set_param_live)
    /// accepts. Empty until something is playing.
    ///
    /// This is the **discoverable** way to do live control: after
    /// [`play_document`](Self::play_document), ask the engine what's adjustable
    /// instead of inspecting the document or memorizing per-node params. Pair it
    /// with the node kinds in the document to build, say, a slider per param.
    ///
    /// ```no_run
    /// # use awsm_audio_player::Player;
    /// # fn demo(player: &Player) {
    /// for (node, params) in player.live_params() {
    ///     for name in params {
    ///         // e.g. surface a control, or drive it from app state:
    ///         player.set_param_live(node, name, 1.0, 0.02);
    ///     }
    /// }
    /// # }
    /// ```
    ///
    /// (Reflects the main graph's nodes — a prewired sound, an arrangement graph.
    /// Per-note voices spawned by a sequencer aren't listed individually.)
    pub fn live_params(&self) -> Vec<(NodeId, Vec<&'static str>)> {
        self.params
            .iter()
            .map(|(id, ps)| (*id, ps.iter().map(|(name, _)| *name).collect()))
            .collect()
    }

    /// The current (base) value of a live param — for initializing a UI control to
    /// the sound's actual setting. `None` if the node/param isn't live.
    pub fn param_value(&self, node: NodeId, param: &str) -> Option<f32> {
        self.params
            .iter()
            .find(|(id, _)| *id == node)
            .and_then(|(_, ps)| {
                ps.iter()
                    .find(|(name, _)| *name == param)
                    .map(|(_, p)| p.value())
            })
    }

    /// Number of scheduled song voices currently alive (for "is sound playing").
    pub fn voice_count(&self) -> usize {
        self.song_voices.len()
    }

    /// Number of time-domain samples the analyser exposes per frame.
    pub fn waveform_len(&self) -> usize {
        self.analyser.fft_size() as usize
    }

    /// Peak output level right now, 0..1 (analyser deviation from silence). A
    /// reliable "is sound coming out" probe that doesn't depend on the canvas.
    pub fn peak(&self) -> f32 {
        let mut buf = vec![128u8; self.analyser.fft_size() as usize];
        self.analyser.get_byte_time_domain_data(&mut buf);
        buf.iter()
            .map(|&b| (f32::from(b) - 128.0).abs() / 128.0)
            .fold(0.0, f32::max)
    }

    /// The context's playback state (`"suspended"` / `"running"` / `"closed"`).
    pub fn context_state(&self) -> String {
        format!("{:?}", self.ctx.state())
    }

    /// Copy the latest time-domain waveform (0..=255, 128 = silence) into `buf`.
    pub fn read_waveform(&self, buf: &mut [u8]) {
        self.analyser.get_byte_time_domain_data(buf);
    }
}
