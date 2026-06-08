//! A bitcrusher: reduces bit depth (`bits`) and sample rate (`reduction`) for a
//! lo-fi, crunchy texture. Build to `wasm32-unknown-unknown` and load into an
//! AudioWorklet node.
//!
//! ```sh
//! cargo build -p awsm-audio-worklet-bitcrusher --target wasm32-unknown-unknown --release
//! # → target/wasm32-unknown-unknown/release/awsm_audio_worklet_bitcrusher.wasm
//! ```

use awsm_audio_worklet::*;

struct Bitcrusher {
    /// Per-channel last quantized sample, held across `reduction` input samples.
    hold: [f32; 2],
    /// Per-channel sample counter for the sample-rate (downsampling) reducer.
    counter: [u32; 2],
}

impl Processor for Bitcrusher {
    const PARAMS: &'static [ParamDesc] = &[
        // Bit depth: fewer bits → coarser quantization.
        ParamDesc::new("bits", 1.0, 16.0, 6.0),
        // Sample-rate reduction factor: hold each value for N samples.
        ParamDesc::new("reduction", 1.0, 32.0, 4.0),
    ];

    fn new(_sample_rate: f32) -> Self {
        Self {
            hold: [0.0; 2],
            counter: [0; 2],
        }
    }

    fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], params: &Params) {
        let bits = params.get(0).clamp(1.0, 16.0);
        // 2^bits quantization levels across the [-1, 1] range.
        let levels = exp2(bits);
        let reduction = params.get(1).max(1.0) as u32;

        for ch in 0..output.len() {
            let inp = input[ch];
            for i in 0..output[ch].len() {
                let x = inp.get(i).copied().unwrap_or(0.0);
                if self.counter[ch] % reduction == 0 {
                    // Quantize to `levels` steps (-1..1 → 0..1, round, back).
                    let q = ((x * 0.5 + 0.5) * levels).round() / levels;
                    self.hold[ch] = q * 2.0 - 1.0;
                }
                self.counter[ch] = self.counter[ch].wrapping_add(1);
                output[ch][i] = self.hold[ch];
            }
        }
    }
}

/// `2^x` without `f32::powf` (which pulls extra symbols on wasm): integer part by
/// repeated doubling, fractional part by a small polynomial. Plenty for `bits`.
#[allow(clippy::approx_constant)] // 0.6931 ≈ ln2; a deliberate inline polynomial
fn exp2(x: f32) -> f32 {
    let n = x.floor();
    let frac = x - n;
    let mut int_pow = 1.0f32;
    let mut k = n as i32;
    while k > 0 {
        int_pow *= 2.0;
        k -= 1;
    }
    let frac_pow = 1.0 + frac * (0.693_147_2 + frac * 0.240_226_5);
    int_pow * frac_pow
}

awsm_worklet!(Bitcrusher);
