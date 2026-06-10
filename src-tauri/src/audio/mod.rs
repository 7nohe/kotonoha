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

/// Pushes samples into the ring buffer from an RT audio callback.
/// Never blocks: when the buffer is full, the oldest audio is simply lost.
pub fn push_samples(producer: &mut rtrb::Producer<f32>, samples: &[f32]) {
    let n = producer.slots().min(samples.len());
    for &s in &samples[..n] {
        let _ = producer.push(s);
    }
}

/// Reinterprets a CoreAudio byte buffer as f32 PCM samples.
///
/// # Safety
/// The buffer must contain f32 PCM data; alignment is guaranteed by CoreAudio.
pub unsafe fn bytes_as_f32(bytes: &[u8]) -> &[f32] {
    std::slice::from_raw_parts(bytes.as_ptr().cast::<f32>(), bytes.len() / 4)
}
