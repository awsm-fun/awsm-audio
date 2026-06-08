# awsm-audio

A node-graph WebAudio editor + player, in Rust → WebAssembly. Build, parameterize,
and play WebAudio graphs in a Max/MSP–style canvas — including your own DSP
compiled to WASM.

## Quick start

```sh
task dev      # build the example worklets, then run the editor dev server
```

Then open the printed localhost URL. `task lint` runs fmt + clippy;
`task build` produces a production bundle; `task worklets` rebuilds the bundled
example WASM modules.

## Workspace

All crates are prefixed `awsm-audio-*`.

| Crate | What it is |
| --- | --- |
| `packages/crates/schema` | Pure-data types for every WebAudio node, full-fidelity `AudioParam` automation, composable samples, assets. Round-trips through TOML. The audio "truth". |
| `packages/crates/player` | Instantiates a schema `Graph` onto a live `AudioContext` (or `OfflineAudioContext` for WAV export). Owns transport, the analyser, noise generation, and the WASM-worklet shim. |
| `packages/crates/worklet` | `awsm-audio-worklet` — write a custom DSP processor for the AudioWorklet node (a `Processor` trait + `awsm_worklet!` macro). |
| `packages/frontend/editor` | The `dominator`/`trunk` reactive UI. |
| `packages/worklets/*` | Example worklet processors (bitcrusher, drive, ringmod). |

## Architecture

Every editor mutation flows through a single **`EditorController`** — UI event
handlers translate gestures into a serde-derived `EditorCommand` and call
`dispatch`; nothing mutates state any other way. A serializable snapshot is the
read half. This single command/query surface is the seam a future MCP/websocket
transport drives — designed now, wired later. The JS-callable bridges
(`editor_dispatch_toml`, `editor_snapshot_toml`, `editor_play`,
`editor_export_wav`, `editor_attach_wasm`, …) live in `editor/src/main.rs`.

The player auto-routes any terminal node (one whose output feeds nothing) into a
master gain → analyser → speakers, so a graph plays without an explicit Output
node. An **Output** node is an explicit sink; **Spatial Output** routes through an
HRTF panner positioned in 3D.

## Nodes

Sources (oscillator, buffer, constant, noise, media element/stream), effects
(gain, biquad, IIR, delay, compressor, waveshaper, convolver), spatial (panner,
stereo panner, spatial output), routing (channel splitter/merger), analysis,
and the WASM **AudioWorklet**. Most params are envelope-automatable and
modulation-wire targets.

## Tips & tricks

**Wiring.** Drag from an **output port** (right edge) to an **input port** (left
edge) for audio. Drag an output to one of the smaller **param inlets** (left
edge, e.g. *modulate frequency*) to modulate that parameter. Right-click a wire
to delete it.

**Inputs — a sample's parameters.** Drop an **Input** node, name it (select it →
the inspector), and wire it to a node's parameter. That input becomes a named
port on the parent's `Sample` node, where you can give it a **value** (per
instance) or MIDI-map it. This is the one way to "control a sub-sample from
outside" — there's no separate "macro" concept.

- An input's **value sets** the inner parameter — it's just that param's value
  for this instance (the same as editing the field). So a voice's `pitch` input
  set to 330 plays at 330.
- A **wire** carrying a signal into a parameter — e.g. an LFO → a filter cutoff,
  or a signal fed into a sample's input port — is **additive**: the signal sums
  with the parameter's value (`computed = value + Σ inputs`). This is the native
  WebAudio model — a connection can't *replace* a param, only add to it. To make
  a wired input fully *be* the value, set that parameter's field to `0` so
  `0 + signal = signal`.

**The "float" primitive** is the **Constant Source** node: set its `offset` and
wire the output anywhere (a param, an input, an audio input). Handy as a DC bias
or a base for LFO modulation.

**Composition.** Each project is a set of **samples** (tabs along the top). A
`Sample` node embeds another sample; the player flattens the whole tree at play
time. Select nodes and press **Ctrl/Cmd-G** to *encapsulate* them into a new
sub-sample with auto-generated inputs/outputs. **Play auditions the active tab**,
so you can work on a sub-sample in isolation.

**Editor.** Drag a palette item onto the canvas to drop it at the cursor (or
click to add at center). **Backspace/Delete** removes the selection;
**Ctrl/Cmd-C/V/D** copy/paste/duplicate; **Ctrl/Cmd-A** select-all;
**Shift-drag** box-selects; the wheel zooms toward the cursor; **Fit** frames the
graph. Right-click a node for **Clone**/**Delete**. Each node's **?** opens MDN-
linked help.

**Visualizing nodes.** Selecting a node shows its envelopes plus, where it
helps, a live picture of what it does: a **Wave Shaper**'s transfer curve, an
**IIR Filter**'s magnitude response, and — for a **custom oscillator** (type
`custom`) — a **drawable harmonics editor**: drag across the bars to paint the
partial amplitudes (bar 0 = fundamental). The player builds a `PeriodicWave`
from them, so you're sketching the timbre directly.

**Playing & MIDI.** The computer keyboard is a one-octave piano (`z`-row white
keys, `s`-row black keys) — it transposes the patch, and it's **polyphonic**:
hold several keys for a chord, each note rings until you release it. Click
**MIDI** to enable Web MIDI: note-on/note-off play polyphonically (note 60 =
unison), **velocity scales amplitude**, and the patch is auditioned per voice.
Map a hardware knob to any input with **MIDI-learn**: in the inputs panel (shown
when nothing is selected) click an input's **MIDI** chip, then turn a CC — it
binds (shown as `CC#n`) and that control then drives the input. Click the chip
again to unbind. CC moves (and dragging an input's value) **sweep live** — the
change glides into the sounding param without rebuilding, across every held
note, so filter sweeps and the like are smooth and click-free.

**Songs (the sequencer).** Drop a **MIDI Song** node to play a whole multi-track
song through instruments you've built (see the **Sequenced Song** example). Select
it and either **load a `.mid` file** or **add a track** and author notes in the
**piano roll**: drag empty grid to draw a note (drag right for length), drag a
note's body to **move** it, drag its right edge to **resize**, **scroll** over it
to set **velocity** (brighter = louder), and click it to delete. Tabs along the
top switch between the song's tracks; a **playhead** sweeps the grid during
playback. Each **part** is an output port bound to a track — **wire that port to
an instrument** (a `Sample` node, or any node) and that part plays it; set a
part's **transpose**/**gain**, and the node's **tempo**, **start** (a beat to
seek to), and **loop** (loops seamlessly). Press **play** to perform the song —
every note becomes a polyphonic voice of its instrument, transposed to pitch and
scaled by velocity. Imported `.mid` files honor **mid-song tempo changes**. A
part can be flagged **drums** (auto for General-MIDI channel 10): its piano-roll
rows are labeled with GM percussion names, and a **per-note drum map** lets each
note play its own instrument sample (build a Kick/Snare/Hat sample and assign
them) — unmapped notes fall back to the wired instrument played pitched. The MIDI
Song node makes no sound itself — it triggers the instruments wired to it.

## Writing a WASM worklet

Implement `Processor` and invoke `awsm_worklet!` once; compile as a `cdylib` to
`wasm32-unknown-unknown` and load the `.wasm` into an AudioWorklet node. Its
parameters are auto-discovered and become editable, automatable, modulation-
targetable knobs. Processing is **stereo** (`CHANNELS = 2`); a mono input is
duplicated to both channels.

```rust
use awsm_audio_worklet::*;

struct Gain;
impl Processor for Gain {
    const PARAMS: &'static [ParamDesc] = &[ParamDesc::new("gain", 0.0, 2.0, 1.0)];
    fn new(_sample_rate: f32) -> Self { Gain }
    fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], params: &Params) {
        let g = params.get(0);
        for ch in 0..output.len() {
            for i in 0..output[ch].len() {
                output[ch][i] = input[ch].get(i).copied().unwrap_or(0.0) * g;
            }
        }
    }
}
awsm_worklet!(Gain);
```

### The worklet ABI

A generic shim is registered once per context; it instantiates your module and
drives it a render quantum at a time. The macro generates these exports (the
shim only requires `memory` + `process`):

- `memory` — the module's linear memory.
- `init(sample_rate: f32, max_frames: u32)` — called once.
- `input_ptr() -> u32` / `output_ptr() -> u32` — base of planar f32 scratch,
  `CHANNELS * MAX_FRAMES` long (channel `c` at `c * MAX_FRAMES`).
- `params_ptr() -> u32` — f32 array, one slot per discovered param (k-rate).
- `process(frames: u32)` — read input + params, write output.
- `channels() -> u32` — channel count (2).
- Discovery: `param_count()`, `param_name_ptr(i)/param_name_len(i)`,
  `param_min(i)/param_max(i)/param_default(i)`.

Modules must be import-free so the shim can instantiate them with no imports.

## Persistence

**Save** writes a self-contained project (the portable `SampleLibrary` — graph +
embedded WASM modules + embedded audio clips — plus editor layout + camera).
**Open** restores it exactly; a bare `SampleLibrary` (e.g. an exported example)
also opens and auto-lays-out. **Export** renders the graph offline to a WAV.

## Status

Verification is preview-driven (an internal headless browser). CI runs
fmt + clippy (wasm + host) + schema tests. Browser-integration tests
(`wasm-bindgen-test`) are not yet wired.
