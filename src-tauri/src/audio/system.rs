use std::sync::mpsc;
use std::sync::Mutex;

use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferExt};
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::configuration::SCStreamConfiguration;
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::output_type::SCStreamOutputType;
use screencapturekit::stream::sc_stream::SCStream;

const SYSTEM_SAMPLE_RATE: u32 = 48_000;
const SYSTEM_CHANNELS: usize = 2;

struct AudioHandler {
    producer: Mutex<rtrb::Producer<f32>>,
}

impl screencapturekit::stream::output_trait::SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if !matches!(of_type, SCStreamOutputType::Audio) {
            return;
        }
        let Some(list) = sample.audio_buffer_list() else {
            return;
        };

        let mut producer = self.producer.lock().unwrap();
        let n_buffers = list.num_buffers();
        if n_buffers >= 2 {
            // Non-interleaved: separate buffer per channel → average into mono
            // and push as already-interleaved mono
            let (Some(left), Some(right)) = (list.buffer(0), list.buffer(1)) else {
                return;
            };
            let (l, r) = unsafe {
                (
                    crate::audio::bytes_as_f32(left.data()),
                    crate::audio::bytes_as_f32(right.data()),
                )
            };
            for i in 0..l.len().min(r.len()) {
                let _ = producer.push((l[i] + r[i]) * 0.5);
            }
        } else if n_buffers == 1 {
            // Interleaved (or mono): push as-is and downmix downstream
            if let Some(buf) = list.buffer(0) {
                let samples = unsafe { crate::audio::bytes_as_f32(buf.data()) };
                crate::audio::push_samples(&mut producer, samples);
            }
        }
    }
}

pub struct SystemCaptureInfo {
    pub sample_rate: u32,
    /// Always 1, since AudioHandler already downmixes to mono
    pub channels: usize,
}

/// Starts system audio capture via ScreenCaptureKit.
/// Requires the Screen Recording permission.
pub fn start(
    producer: rtrb::Producer<f32>,
    stop_rx: mpsc::Receiver<()>,
) -> Result<SystemCaptureInfo, String> {
    let (info_tx, info_rx) = mpsc::channel::<Result<(), String>>();

    std::thread::Builder::new()
        .name("system-capture".into())
        .spawn(move || {
            let result = (|| -> Result<SCStream, String> {
                let content = SCShareableContent::get().map_err(|e| {
                    format!(
                        "画面収録の権限がないかコンテンツを取得できません: {e}。\
                         システム設定 → プライバシーとセキュリティ → 画面収録 で許可してください。"
                    )
                })?;
                let displays = content.displays();
                let display = displays.first().ok_or("ディスプレイが見つかりません")?;
                let filter = SCContentFilter::create()
                    .with_display(display)
                    .with_excluding_windows(&[])
                    .build();

                // We only want audio, but SCK requires video too, so keep it minimal in size and frequency
                let config = SCStreamConfiguration::new()
                    .with_captures_audio(true)
                    .with_sample_rate(SYSTEM_SAMPLE_RATE as i32)
                    .with_channel_count(SYSTEM_CHANNELS as i32)
                    .with_excludes_current_process_audio(true);

                let mut stream = SCStream::new(&filter, &config);
                stream.add_output_handler(
                    AudioHandler {
                        producer: Mutex::new(producer),
                    },
                    SCStreamOutputType::Audio,
                );
                stream
                    .start_capture()
                    .map_err(|e| format!("システム音声キャプチャの開始に失敗: {e}"))?;
                Ok(stream)
            })();

            match result {
                Ok(stream) => {
                    let _ = info_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    let _ = stream.stop_capture();
                }
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;

    info_rx
        .recv()
        .map_err(|_| "システム音声キャプチャスレッドが応答しません".to_string())??;

    Ok(SystemCaptureInfo {
        sample_rate: SYSTEM_SAMPLE_RATE,
        channels: 1,
    })
}
