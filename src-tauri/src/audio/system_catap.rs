//! System audio capture via a Core Audio process tap (macOS 14.4+).
//! Unlike ScreenCaptureKit, it only needs the lighter "System Audio Recording"
//! permission instead of "Screen Recording".
//! On failure, the caller (pipeline) falls back to SCK.

use std::sync::mpsc;

use cidre::core_audio::aggregate_device_keys as agg_keys;
use cidre::core_audio::{
    device_start, AggregateDevice, Device, DeviceIoProcId, System, TapDesc,
};
use cidre::{cat, cf, ns, os};

pub struct CatapCaptureInfo {
    pub sample_rate: u32,
    pub channels: usize,
}

struct ProcCtx {
    producer: rtrb::Producer<f32>,
}

/// IOProc (RT thread): streams tapped audio into the ring buffer as raw f32.
extern "C" fn io_proc(
    _device: Device,
    _now: &cat::AudioTimeStamp,
    input_data: &cat::AudioBufList<1>,
    _input_time: &cat::AudioTimeStamp,
    _output_data: &mut cat::AudioBufList<1>,
    _output_time: &cat::AudioTimeStamp,
    client_data: Option<&mut ProcCtx>,
) -> os::Status {
    let Some(ctx) = client_data else {
        return os::Status::NO_ERR;
    };
    let n_buffers = input_data.number_buffers as usize;
    for i in 0..n_buffers.min(1) {
        let buf = &input_data.buffers[i];
        if buf.data.is_null() {
            continue;
        }
        let samples = unsafe {
            std::slice::from_raw_parts(buf.data as *const f32, buf.data_bytes_size as usize / 4)
        };
        crate::audio::push_samples(&mut ctx.producer, samples);
    }
    os::Status::NO_ERR
}

pub fn start(
    producer: rtrb::Producer<f32>,
    stop_rx: mpsc::Receiver<()>,
) -> Result<CatapCaptureInfo, String> {
    let (info_tx, info_rx) = mpsc::channel::<Result<CatapCaptureInfo, String>>();

    std::thread::Builder::new()
        .name("catap-capture".into())
        .spawn(move || {
            // The tap, aggregate device, and IOProc are created on this thread and held until stop
            let result = (|| -> Result<_, String> {
                let desc = TapDesc::with_mono_global_tap_excluding_processes(&ns::Array::new());
                let tap = desc
                    .create_process_tap()
                    .map_err(|e| format!("process tap の作成に失敗 (macOS 14.4+ が必要): {e:?}"))?;
                let tap_uid = tap.uid().map_err(|e| format!("tap uid 取得に失敗: {e:?}"))?;
                let asbd = tap.asbd().map_err(|e| format!("tap format 取得に失敗: {e:?}"))?;
                let sample_rate = asbd.sample_rate as u32;
                let channels = asbd.channels_per_frame as usize;

                let output_uid = System::default_output_device()
                    .and_then(|d| d.uid())
                    .map_err(|e| format!("出力デバイス取得に失敗: {e:?}"))?;
                let uuid = cf::Uuid::new().to_cf_string();
                let dict = cf::DictionaryOf::with_keys_values(
                    &[
                        agg_keys::is_private(),
                        agg_keys::is_stacked(),
                        agg_keys::tap_auto_start(),
                        agg_keys::name(),
                        agg_keys::main_sub_device(),
                        agg_keys::uid(),
                    ],
                    &[
                        cf::Boolean::value_true().as_type_ref(),
                        cf::Boolean::value_false(),
                        cf::Boolean::value_true(),
                        cf::str!(c"kotonoha-tap"),
                        &output_uid,
                        &uuid,
                    ],
                );
                let mut agg = AggregateDevice::with_desc(&dict)
                    .map_err(|e| format!("集約デバイスの作成に失敗: {e:?}"))?;

                let taps = cf::ArrayOf::<cf::String>::from_slice(&[tap_uid.as_ref()]);
                agg.set_tap_list(taps)
                    .map_err(|e| format!("tap list の設定に失敗: {e:?}"))?;

                let mut ctx = Box::new(ProcCtx { producer });
                let proc_id: DeviceIoProcId = agg
                    .create_io_proc_id(io_proc, Some(ctx.as_mut()))
                    .map_err(|e| format!("IOProc の作成に失敗: {e:?}"))?;
                let started = device_start(agg, Some(proc_id))
                    .map_err(|e| format!("キャプチャ開始に失敗: {e:?}"))?;

                Ok((tap, started, ctx, sample_rate, channels))
            })();

            match result {
                Ok((tap, started, ctx, sample_rate, channels)) => {
                    let _ = info_tx.send(Ok(CatapCaptureInfo {
                        sample_rate,
                        channels,
                    }));
                    let _ = stop_rx.recv();
                    drop(started); // AudioDeviceStop
                    drop(tap); // AudioHardwareDestroyProcessTap
                    drop(ctx);
                }
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;

    info_rx
        .recv()
        .map_err(|_| "CATap キャプチャスレッドが応答しません".to_string())?
}
