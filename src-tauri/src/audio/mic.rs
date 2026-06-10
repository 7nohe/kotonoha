use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;

pub struct MicCaptureInfo {
    pub sample_rate: u32,
    pub channels: usize,
}

/// Starts microphone capture.
///
/// cpal::Stream is !Send, so it is created and held inside a dedicated thread;
/// a signal on stop_rx (or disconnection) drops the stream and ends the thread.
/// The audio callback runs on the RT thread, so it avoids locks and allocations
/// and only writes into the rtrb ring buffer.
pub fn start(
    mut producer: rtrb::Producer<f32>,
    stop_rx: mpsc::Receiver<()>,
) -> Result<MicCaptureInfo, String> {
    let (info_tx, info_rx) = mpsc::channel::<Result<MicCaptureInfo, String>>();

    std::thread::Builder::new()
        .name("mic-capture".into())
        .spawn(move || {
            let result = (|| -> Result<(cpal::Stream, MicCaptureInfo), String> {
                let host = cpal::default_host();
                let device = host
                    .default_input_device()
                    .ok_or("入力デバイスが見つかりません")?;
                let supported = device
                    .default_input_config()
                    .map_err(|e| format!("入力デバイス設定の取得に失敗: {e}"))?;
                let sample_rate = supported.sample_rate();
                let channels = supported.channels() as usize;
                let config: cpal::StreamConfig = supported.into();

                let stream = device
                    .build_input_stream(
                        config,
                        move |data: &[f32], _info| {
                            // If the buffer is full, drop old audio instead of blocking
                            let n = producer.slots().min(data.len());
                            for &sample in &data[..n] {
                                let _ = producer.push(sample);
                            }
                        },
                        |err| eprintln!("mic stream error: {err}"),
                        None,
                    )
                    .map_err(|e| format!("マイクストリームの作成に失敗: {e}"))?;
                stream.play().map_err(|e| format!("マイクストリームの開始に失敗: {e}"))?;

                Ok((stream, MicCaptureInfo { sample_rate, channels }))
            })();

            match result {
                Ok((stream, info)) => {
                    let _ = info_tx.send(Ok(info));
                    // Keep the stream alive until a stop signal or the sender is dropped
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;

    info_rx
        .recv()
        .map_err(|_| "マイクキャプチャスレッドが応答しません".to_string())?
}
