use std::path::PathBuf;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const WHISPER_MODEL: &str = "ggml-large-v3-turbo-q5_0.bin";
pub const VAD_MODEL: &str = "ggml-silero-v5.1.2.bin";

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const VAD_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin";

/// Whisper models downloadable from within the app
const CATALOG: &[(&str, &str, u64)] = &[
    ("large-v3-turbo (推奨)", "ggml-large-v3-turbo-q5_0.bin", 574),
    ("base (軽量・低精度)", "ggml-base.bin", 142),
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelInfo {
    pub label: String,
    pub file: String,
    pub size_mb: u64,
    pub downloaded: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn whisper_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let path = models_dir(app)?.join(WHISPER_MODEL);
    if !path.exists() {
        return Err(format!(
            "Whisper モデルが見つかりません: {}。設定画面からダウンロードしてください。",
            path.display()
        ));
    }
    Ok(path)
}

pub fn vad_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let path = models_dir(app)?.join(VAD_MODEL);
    if !path.exists() {
        return Err(format!(
            "VAD モデルが見つかりません: {}。設定画面からダウンロードしてください。",
            path.display()
        ));
    }
    Ok(path)
}

pub fn is_ready(app: &AppHandle) -> bool {
    whisper_model_path(app).is_ok() && vad_model_path(app).is_ok()
}

pub fn list(app: &AppHandle) -> Result<Vec<WhisperModelInfo>, String> {
    let dir = models_dir(app)?;
    Ok(CATALOG
        .iter()
        .map(|(label, file, size_mb)| WhisperModelInfo {
            label: label.to_string(),
            file: file.to_string(),
            size_mb: *size_mb,
            downloaded: dir.join(file).exists(),
        })
        .collect())
}

/// Downloads a whisper model. Fetches the VAD model first if missing (~1MB).
/// Progress is reported via the `model-download-progress` event.
pub async fn download(app: AppHandle, file: String) -> Result<(), String> {
    if !CATALOG.iter().any(|(_, f, _)| *f == file) {
        return Err(format!("未知のモデル: {file}"));
    }
    let dir = models_dir(&app)?;

    let vad_dest = dir.join(VAD_MODEL);
    if !vad_dest.exists() {
        download_file(&app, VAD_URL, &vad_dest, VAD_MODEL).await?;
    }

    let dest = dir.join(&file);
    if dest.exists() {
        return Ok(());
    }
    let url = format!("{HF_BASE}/{file}");
    download_file(&app, &url, &dest, &file).await
}

async fn download_file(
    app: &AppHandle,
    url: &str,
    dest: &std::path::Path,
    name: &str,
) -> Result<(), String> {
    let res = reqwest::get(url)
        .await
        .map_err(|e| format!("ダウンロードに失敗: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("ダウンロードに失敗: HTTP {}", res.status()));
    }
    let total = res.content_length().unwrap_or(0);

    // Write to .part and rename on completion (avoids corrupt files after interruption)
    let part = dest.with_extension("part");
    let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut stream = res.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("ダウンロード中断: {e}"))?;
        std::io::Write::write_all(&mut out, &chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        // Report progress every 2MB
        if downloaded - last_emit > 2_000_000 || downloaded == total {
            last_emit = downloaded;
            let _ = app.emit(
                "model-download-progress",
                DownloadProgress {
                    file: name.to_string(),
                    downloaded,
                    total,
                },
            );
        }
    }
    drop(out);

    if total > 0 && downloaded != total {
        let _ = std::fs::remove_file(&part);
        return Err("ダウンロードが不完全です。再試行してください。".into());
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
    let _ = app.emit(
        "model-download-progress",
        DownloadProgress {
            file: name.to_string(),
            downloaded,
            total: downloaded.max(total),
        },
    );
    Ok(())
}
