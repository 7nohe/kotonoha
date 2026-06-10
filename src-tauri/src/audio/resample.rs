use rubato::{FastFixedIn, PolynomialDegree, Resampler};

use super::TARGET_SAMPLE_RATE;

const CHUNK_SIZE: usize = 1024;

/// Converts mono f32 samples of any input rate to 16kHz.
/// FastFixedIn requires fixed-size input chunks, so samples are staged internally.
pub struct MonoResampler {
    inner: Option<FastFixedIn<f32>>,
    staging: Vec<f32>,
}

impl MonoResampler {
    pub fn new(input_rate: u32) -> Result<Self, String> {
        let inner = if input_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
            Some(
                FastFixedIn::new(ratio, 1.0, PolynomialDegree::Cubic, CHUNK_SIZE, 1)
                    .map_err(|e| format!("リサンプラーの作成に失敗: {e}"))?,
            )
        };
        Ok(Self {
            inner,
            staging: Vec::with_capacity(CHUNK_SIZE * 2),
        })
    }

    pub fn process(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
        let Some(resampler) = self.inner.as_mut() else {
            return Ok(samples.to_vec());
        };

        self.staging.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.staging.len() >= CHUNK_SIZE {
            let chunk: Vec<f32> = self.staging.drain(..CHUNK_SIZE).collect();
            let mut result = resampler
                .process(&[chunk], None)
                .map_err(|e| format!("リサンプルに失敗: {e}"))?;
            out.append(&mut result[0]);
        }
        Ok(out)
    }
}
