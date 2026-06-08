//! Deterministic noise synthesis. Given a [`NoiseFlavor`] + seed, produce a
//! `Vec<f32>` of samples in roughly `[-1, 1]`. All randomness comes from a
//! seeded `fastrand::Rng`, so the same recipe always yields the same buffer.
//!
//! The continuous colors differ by spectral tilt (white = flat, pink = −3
//! dB/oct, brown = −6, blue = +3, violet = +6); dust/velvet are sparse impulse
//! trains for droplet/crackle textures.

use awsm_audio_schema::NoiseFlavor;
use fastrand::Rng;

/// Generate `len` samples of `flavor` noise.
pub fn generate(
    flavor: NoiseFlavor,
    seed: u64,
    len: usize,
    sample_rate: f32,
    density: f32,
    gaussian: bool,
) -> Vec<f32> {
    let mut rng = Rng::with_seed(seed);
    let mut out = match flavor {
        NoiseFlavor::White => white(&mut rng, len, gaussian),
        NoiseFlavor::Pink => pink(&mut rng, len, gaussian),
        NoiseFlavor::Brown => brown(&mut rng, len, gaussian),
        NoiseFlavor::Violet => violet(&mut rng, len, gaussian),
        NoiseFlavor::Blue => blue(&mut rng, len, gaussian),
        NoiseFlavor::Dust => dust(&mut rng, len, sample_rate, density),
        NoiseFlavor::Velvet => velvet(&mut rng, len, sample_rate, density),
    };
    normalize(&mut out, 0.9);
    out
}

/// One white sample in `[-1, 1]`. Gaussian-ish via summed uniforms (central
/// limit) when requested.
fn white_sample(rng: &mut Rng, gaussian: bool) -> f32 {
    if gaussian {
        // Sum of 4 uniforms → approx normal; scale so ±~1 covers the bulk.
        let s: f32 = (0..4).map(|_| rng.f32() * 2.0 - 1.0).sum();
        s * 0.5
    } else {
        rng.f32() * 2.0 - 1.0
    }
}

fn white(rng: &mut Rng, len: usize, gaussian: bool) -> Vec<f32> {
    (0..len).map(|_| white_sample(rng, gaussian)).collect()
}

/// Paul Kellet's economy pink-noise filter (−3 dB/oct).
fn pink(rng: &mut Rng, len: usize, gaussian: bool) -> Vec<f32> {
    let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
    (0..len)
        .map(|_| {
            let w = white_sample(rng, gaussian);
            b0 = 0.997_76 * b0 + w * 0.099_046;
            b1 = 0.963_00 * b1 + w * 0.296_516_4;
            b2 = 0.570_00 * b2 + w * 1.052_691_3;
            (b0 + b1 + b2 + w * 0.184_8) * 0.2
        })
        .collect()
}

/// Leaky-integrated white (−6 dB/oct): the deep rumble.
fn brown(rng: &mut Rng, len: usize, gaussian: bool) -> Vec<f32> {
    let mut last = 0.0f32;
    (0..len)
        .map(|_| {
            let w = white_sample(rng, gaussian);
            last = (last + 0.02 * w) / 1.02;
            last * 3.5
        })
        .collect()
}

/// First difference of white (+6 dB/oct).
fn violet(rng: &mut Rng, len: usize, gaussian: bool) -> Vec<f32> {
    let mut prev = 0.0f32;
    (0..len)
        .map(|_| {
            let w = white_sample(rng, gaussian);
            let v = w - prev;
            prev = w;
            v
        })
        .collect()
}

/// First difference of pink (≈ +3 dB/oct).
fn blue(rng: &mut Rng, len: usize, gaussian: bool) -> Vec<f32> {
    let p = pink(rng, len, gaussian);
    let mut prev = 0.0f32;
    p.into_iter()
        .map(|x| {
            let v = x - prev;
            prev = x;
            v
        })
        .collect()
}

/// Sparse random impulses: with probability `density/sr` each sample fires a
/// random-amplitude spike (droplets, crackle, Geiger-style ticks).
fn dust(rng: &mut Rng, len: usize, sample_rate: f32, density: f32) -> Vec<f32> {
    let prob = (density / sample_rate).clamp(0.0, 1.0);
    (0..len)
        .map(|_| {
            if (rng.f32()) < prob {
                rng.f32() * 2.0 - 1.0
            } else {
                0.0
            }
        })
        .collect()
}

/// One signed (±1) impulse per grid window of `sr/density` samples, placed at a
/// random position — smoother/regular vs dust.
fn velvet(rng: &mut Rng, len: usize, sample_rate: f32, density: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    let window = ((sample_rate / density.max(1.0)) as usize).max(1);
    let mut i = 0;
    while i < len {
        let pos = i + rng.usize(0..window);
        if pos < len {
            out[pos] = if rng.bool() { 1.0 } else { -1.0 };
        }
        i += window;
    }
    out
}

/// Scale so the peak magnitude is `peak` (no-op for silence).
fn normalize(samples: &mut [f32], peak: f32) {
    let max = samples.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    if max > 1e-9 {
        let g = peak / max;
        for x in samples.iter_mut() {
            *x *= g;
        }
    }
}
