//! The generic WASM AudioWorklet shim.
//!
//! One processor (`awsm-wasm`) is registered per context. It is fed a compiled
//! `WebAssembly.Module` (via `processorOptions.module`) that implements the
//! awsm-audio worklet ABI, instantiates it on the audio thread, and drives it
//! one render quantum at a time: copy the mono input + current parameter values
//! into the module's linear memory, call its `process(frames)`, copy the mono
//! output back out (fanned to every output channel).
//!
//! ## Module ABI (all exports optional unless noted)
//! - `memory: WebAssembly.Memory` — **required**, the module's linear memory.
//! - `init(sample_rate: f32, max_frames: u32)` — called once after instantiate.
//! - `input_ptr() -> u32` / `output_ptr() -> u32` — base of planar-f32 mono
//!   scratch regions, each `max_frames` long.
//! - `params_ptr() -> u32` — base of an f32 array, one slot per discovered param.
//! - `process(frames: u32)` — **required**, reads input+params, writes output.
//! - Discovery: `param_count() -> u32`, `param_name_ptr(i)/param_name_len(i)`,
//!   `param_min(i)/param_max(i)/param_default(i) -> f32` (read on the main
//!   thread by the editor; the shim only needs `param_count`).
//!
//! The number of generic `AudioParam`s the shim declares is [`PARAM_BANK`]; a
//! module may expose up to that many discovered params.

/// How many generic `AudioParam`s the shim's processor always declares. A loaded
/// module's discovered params are mapped onto `p0..p{N-1}` so they become real,
/// automatable, modulation-targetable AudioParams.
pub const PARAM_BANK: usize = 32;

/// The shim source, registered once per context via `audioWorklet.addModule`.
/// `__BANK__` is substituted with [`PARAM_BANK`] at runtime.
pub const SHIM_TEMPLATE: &str = r#"
const BANK = __BANK__;
class AwsmWasm extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    const d = [];
    for (let i = 0; i < BANK; i++) {
      d.push({ name: 'p' + i, defaultValue: 0,
               minValue: -3.4e38, maxValue: 3.4e38, automationRate: 'k-rate' });
    }
    return d;
  }
  constructor(opts) {
    super();
    const po = (opts && opts.processorOptions) || {};
    this.ok = false;
    try {
      this.inst = new WebAssembly.Instance(po.module, {});
      const ex = this.inst.exports;
      this.ex = ex;
      this.mem = ex.memory;
      this.maxFrames = 128;
      if (ex.init) ex.init(sampleRate, this.maxFrames);
      this.paramCount = ex.param_count ? (ex.param_count() | 0) : 0;
      this.inPtr  = ex.input_ptr  ? (ex.input_ptr()  | 0) : 0;
      this.outPtr = ex.output_ptr ? (ex.output_ptr() | 0) : 0;
      this.parPtr = ex.params_ptr ? (ex.params_ptr() | 0) : 0;
      this.chans  = ex.channels ? Math.max(1, ex.channels() | 0) : 1;
      this.run = ex.process || null;
      this.ok = !!(this.mem && this.run);
    } catch (e) {
      // Leave ok=false → passes audio through untouched.
      this.err = String(e);
    }
  }
  process(inputs, outputs, parameters) {
    const out = outputs[0];
    if (!out || !out[0]) return true;
    const frames = out[0].length;
    const inp = inputs[0];
    if (!this.ok) {
      // Pass-through: copy input to output if present, else silence.
      for (let c = 0; c < out.length; c++) {
        if (inp && inp[c]) out[c].set(inp[c]); else out[c].fill(0);
      }
      return true;
    }
    const buf = this.mem.buffer;
    // Parameters → wasm memory (k-rate: one value per quantum).
    if (this.parPtr && this.paramCount) {
      const pv = new Float32Array(buf, this.parPtr, this.paramCount);
      for (let i = 0; i < this.paramCount; i++) {
        const a = parameters['p' + i];
        pv[i] = a ? a[0] : 0;
      }
    }
    const CH = this.chans, MF = this.maxFrames;
    // Inputs → wasm planar regions (channel c at byte offset ptr + c*MF*4).
    // A mono input is duplicated to every channel.
    if (this.inPtr) {
      for (let c = 0; c < CH; c++) {
        const region = new Float32Array(buf, this.inPtr + c * MF * 4, frames);
        const src = inp && inp.length ? inp[Math.min(c, inp.length - 1)] : null;
        if (src) region.set(src.subarray(0, frames)); else region.fill(0);
      }
    }
    this.run(frames);
    // Outputs ← wasm planar regions (clamp channel index to what the module has).
    if (this.outPtr) {
      for (let c = 0; c < out.length; c++) {
        const region = new Float32Array(buf, this.outPtr + Math.min(c, CH - 1) * MF * 4, frames);
        out[c].set(region.subarray(0, frames));
      }
    }
    return true;
  }
}
registerProcessor('awsm-wasm', AwsmWasm);
"#;

/// The registered processor name the player instantiates.
pub const PROCESSOR_NAME: &str = "awsm-wasm";

/// The shim source with the param-bank size substituted in.
pub fn shim_source() -> String {
    SHIM_TEMPLATE.replace("__BANK__", &PARAM_BANK.to_string())
}
