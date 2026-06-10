pub mod mic;
pub mod resample;
pub mod system;
pub mod system_catap;

/// Sample rate required by whisper
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Downmixes interleaved multi-channel f32 samples to mono
pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}
