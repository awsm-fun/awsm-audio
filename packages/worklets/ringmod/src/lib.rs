//! Ring modulator: multiplies the input by an internal sine oscillator, giving
//! metallic, clangorous, bell-like timbres. Shows a stateful processor with its
//! own oscillator (uses the context sample rate).
//!
//! ```sh
//! cargo build -p awsm-audio-worklet-ringmod --target wasm32-unknown-unknown --release
//! ```

use awsm_audio_worklet::{awsm_worklet, math, ParamDesc, Params, Processor};

struct RingMod {
    phase: f32,
    sample_rate: f32,
}

impl Processor for RingMod {
    const PARAMS: &'static [ParamDesc] = &[
        // Modulator frequency.
        ParamDesc::new("freq", 20.0, 2000.0, 220.0),
        // Dry/wet blend.
        ParamDesc::new("mix", 0.0, 1.0, 1.0),
    ];

    fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            sample_rate: sample_rate.max(1.0),
        }
    }

    fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]], params: &Params) {
        let freq = params.get(0).max(0.0);
        let mix = params.get(1).clamp(0.0, 1.0);
        let inc = math::TAU * freq / self.sample_rate;
        let frames = output.first().map(|c| c.len()).unwrap_or(0);
        // One shared modulator across channels: advance phase per frame.
        for i in 0..frames {
            let m = math::sin(self.phase);
            self.phase += inc;
            if self.phase > math::TAU {
                self.phase -= math::TAU;
            }
            for ch in 0..output.len() {
                let x = input[ch].get(i).copied().unwrap_or(0.0);
                output[ch][i] = x * (1.0 - mix) + (x * m) * mix;
            }
        }
    }
}

awsm_worklet!(RingMod);
