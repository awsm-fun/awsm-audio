//! Overdrive: `tanh` soft-clipping saturation with drive / mix / output level.
//! A warm distortion that adds harmonics as `drive` rises.
//!
//! ```sh
//! cargo build -p awsm-audio-worklet-drive --target wasm32-unknown-unknown --release
//! ```

use awsm_audio_worklet::{awsm_worklet, math, ParamDesc, Params, Processor};

struct Drive;

impl Processor for Drive {
    const PARAMS: &'static [ParamDesc] = &[
        // Pre-gain into the saturator — more drive → more harmonics.
        ParamDesc::new("drive", 1.0, 30.0, 6.0),
        // Dry/wet blend.
        ParamDesc::new("mix", 0.0, 1.0, 1.0),
        // Output level (saturation makes things louder).
        ParamDesc::new("level", 0.0, 1.0, 0.7),
    ];

    fn new(_sample_rate: f32) -> Self {
        Drive
    }

    fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], params: &Params) {
        let drive = params.get(0).max(1.0);
        let mix = params.get(1).clamp(0.0, 1.0);
        let level = params.get(2);
        // Normalize so the wet signal stays roughly unity-peak across drive.
        let norm = 1.0 / math::tanh(drive);
        for ch in 0..output.len() {
            let inp = input[ch];
            for i in 0..output[ch].len() {
                let x = inp.get(i).copied().unwrap_or(0.0);
                let wet = math::tanh(x * drive) * norm;
                output[ch][i] = (x * (1.0 - mix) + wet * mix) * level;
            }
        }
    }
}

awsm_worklet!(Drive);
