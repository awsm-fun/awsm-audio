//! The fixed WebAudio enumerations, with serde representations matching the
//! platform's string values so the player can map each variant straight onto
//! the corresponding `web_sys` enum.

use serde::{Deserialize, Serialize};

/// `OscillatorNode.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OscillatorType {
    #[default]
    Sine,
    Square,
    Sawtooth,
    Triangle,
    /// Driven by a [`PeriodicWaveAsset`](crate::PeriodicWaveAsset) referenced on
    /// the node.
    Custom,
}

/// `BiquadFilterNode.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BiquadFilterType {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
    Lowshelf,
    Highshelf,
    Peaking,
    Notch,
    Allpass,
}

/// `WaveShaperNode.oversample`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OverSampleType {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "2x")]
    X2,
    #[serde(rename = "4x")]
    X4,
}

/// The distortion character of a [`WaveShaperNode`](crate::WaveShaperNode). The
/// player generates the shaping curve from this + `amount` (the intensity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveShaperShape {
    /// Smooth `tanh` saturation (warm overdrive).
    #[default]
    Tanh,
    /// Hard clipping (aggressive, square-ish).
    HardClip,
    /// Sine wavefolder (metallic, folds more as `amount` rises).
    Fold,
    /// A user-drawn transfer curve (see [`WaveShaperNode::curve`]); `amount` is
    /// ignored. Falls back to `tanh` if the curve is empty.
    Custom,
}

/// `PannerNode.panningModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PanningModelType {
    #[default]
    #[serde(rename = "equalpower")]
    EqualPower,
    #[serde(rename = "HRTF")]
    Hrtf,
}

/// `PannerNode.distanceModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DistanceModelType {
    Linear,
    #[default]
    Inverse,
    Exponential,
}

/// `AudioNode.channelCountMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelCountMode {
    #[default]
    Max,
    ClampedMax,
    Explicit,
}

/// `AudioNode.channelInterpretation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelInterpretation {
    #[default]
    Speakers,
    Discrete,
}

/// Spectral/character flavor of a [`NoiseNode`](crate::NoiseNode). Not a
/// WebAudio enum — these select how the player synthesizes the noise buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseFlavor {
    /// Flat spectrum — bright hiss.
    #[default]
    White,
    /// −3 dB/oct — balanced/natural (rain, wind, surf).
    Pink,
    /// −6 dB/oct — deep rumble (heavy rain, thunder, ocean).
    Brown,
    /// +3 dB/oct — airy/bright.
    Blue,
    /// +6 dB/oct — very bright/hissy.
    Violet,
    /// Sparse random impulses at `density` events/sec (droplets, crackle).
    Dust,
    /// Sparse signed impulses, one per grid window — smoother than dust.
    Velvet,
}

/// `AudioParam.automationRate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomationRate {
    #[serde(rename = "a-rate")]
    ARate,
    #[serde(rename = "k-rate")]
    KRate,
}
