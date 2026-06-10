use std::sync::LazyLock;

use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;

const BASE_URL: &str = "http://localhost:11434";

/// Shared HTTP client (connection pool reused across all requests)
pub static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

const SYSTEM_PROMPT: &str = "You are a professional simultaneous interpreter. \
Translate the user's English utterance into natural Japanese. \
Output ONLY the Japanese translation. No explanations, no romaji, no quotes. \
The input is live meeting speech and may be a sentence fragment; translate it as-is.";

const SUMMARY_PROMPT: &str = "あなたは優秀な議事録作成者です。\
ユーザーが送るミーティングの文字起こしから、日本語で簡潔な議事録を Markdown で作成してください。\
構成: 「### 要点」「### 決定事項」「### TODO」。該当が無いセクションは省略してください。\
文字起こしの誤認識は文脈から自然に補正して構いません。出力は議事録本文のみ。";

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Deserialize)]
struct ChatChunk {
    message: Option<ChatMessage>,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

pub async fn check() -> bool {
    HTTP.get(format!("{BASE_URL}/api/version"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .is_ok()
}

pub async fn list_models() -> Result<Vec<String>, String> {
    let res = HTTP
        .get(format!("{BASE_URL}/api/tags"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| "Ollama に接続できません。`ollama serve` が起動しているか確認してください。")?;
    let tags: TagsResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(tags.models.into_iter().map(|m| m.name).collect())
}

/// POSTs a system+user chat request to Ollama and returns the raw response
async fn chat_request(
    model: &str,
    system: &str,
    user: &str,
    stream: bool,
    temperature: f64,
    timeout: Option<std::time::Duration>,
) -> Result<reqwest::Response, String> {
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "stream": stream,
        "keep_alive": "30m",
        "options": { "temperature": temperature },
    });

    let mut req = HTTP.post(format!("{BASE_URL}/api/chat")).json(&body);
    if let Some(t) = timeout {
        req = req.timeout(t);
    }
    let res = req
        .send()
        .await
        .map_err(|_| "Ollama に接続できません。`ollama serve` を確認してください。".to_string())?;
    if !res.status().is_success() {
        return Err(format!("Ollama エラー: HTTP {}", res.status()));
    }
    Ok(res)
}

/// Generates a meeting-minutes summary from the full transcript (non-streaming)
pub async fn summarize(model: &str, transcript: &str) -> Result<String, String> {
    let res = chat_request(
        model,
        SUMMARY_PROMPT,
        transcript,
        false,
        0.3,
        Some(std::time::Duration::from_secs(300)),
    )
    .await?;
    let value: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    value["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or("Ollama の応答を解釈できません".to_string())
}

/// Streams a translation of English text into Japanese.
/// Calls on_delta(delta, done) as each token arrives.
pub async fn translate_stream(
    model: &str,
    text: &str,
    mut on_delta: impl FnMut(String, bool),
) -> Result<(), String> {
    let res = chat_request(model, SYSTEM_PROMPT, text, true, 0.0, None).await?;

    let mut stream = res.bytes_stream();
    let mut line_buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Ollama ストリームエラー: {e}"))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // NDJSON: parse each complete line in place, then drop it from the buffer
        while let Some(pos) = line_buf.find('\n') {
            let parsed = serde_json::from_str::<ChatChunk>(line_buf[..pos].trim()).ok();
            line_buf.drain(..=pos);
            let Some(parsed) = parsed else { continue };
            let delta = parsed.message.map(|m| m.content).unwrap_or_default();
            if !delta.is_empty() || parsed.done {
                on_delta(delta, parsed.done);
            }
            if parsed.done {
                return Ok(());
            }
        }
    }
    Ok(())
}
