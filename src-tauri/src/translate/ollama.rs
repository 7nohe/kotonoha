use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;

const BASE_URL: &str = "http://localhost:11434";

const SYSTEM_PROMPT: &str = "You are a professional simultaneous interpreter. \
Translate the user's English utterance into natural Japanese. \
Output ONLY the Japanese translation. No explanations, no romaji, no quotes. \
The input is live meeting speech and may be a sentence fragment; translate it as-is.";

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
    reqwest::Client::new()
        .get(format!("{BASE_URL}/api/version"))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .is_ok()
}

pub async fn list_models() -> Result<Vec<String>, String> {
    let res = reqwest::Client::new()
        .get(format!("{BASE_URL}/api/tags"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| "Ollama に接続できません。`ollama serve` が起動しているか確認してください。")?;
    let tags: TagsResponse = res.json().await.map_err(|e| e.to_string())?;
    Ok(tags.models.into_iter().map(|m| m.name).collect())
}

const SUMMARY_PROMPT: &str = "あなたは優秀な議事録作成者です。\
ユーザーが送るミーティングの文字起こしから、日本語で簡潔な議事録を Markdown で作成してください。\
構成: 「### 要点」「### 決定事項」「### TODO」。該当が無いセクションは省略してください。\
文字起こしの誤認識は文脈から自然に補正して構いません。出力は議事録本文のみ。";

/// Generates a meeting-minutes summary from the full transcript (non-streaming)
pub async fn summarize(model: &str, transcript: &str) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SUMMARY_PROMPT },
            { "role": "user", "content": transcript },
        ],
        "stream": false,
        "keep_alive": "30m",
        "options": { "temperature": 0.3 },
    });

    let res = reqwest::Client::new()
        .post(format!("{BASE_URL}/api/chat"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|_| "Ollama に接続できません。`ollama serve` を確認してください。".to_string())?;
    if !res.status().is_success() {
        return Err(format!("Ollama エラー: HTTP {}", res.status()));
    }
    let value: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    value["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or("Ollama の応答を解釈できません".to_string())
}

/// Streams a translation of English text into Japanese.
/// Calls on_delta(delta, done) as each token arrives.
pub async fn translate_stream(
    client: &reqwest::Client,
    model: &str,
    text: &str,
    mut on_delta: impl FnMut(String, bool),
) -> Result<(), String> {
    let body = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": text },
        ],
        "stream": true,
        "keep_alive": "30m",
        "options": { "temperature": 0.0 },
    });

    let res = client
        .post(format!("{BASE_URL}/api/chat"))
        .json(&body)
        .send()
        .await
        .map_err(|_| "Ollama に接続できません".to_string())?;

    if !res.status().is_success() {
        return Err(format!("Ollama エラー: HTTP {}", res.status()));
    }

    let mut stream = res.bytes_stream();
    let mut line_buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Ollama ストリームエラー: {e}"))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // NDJSON: process only complete lines, carry the rest over in the buffer
        while let Some(pos) = line_buf.find('\n') {
            let line: String = line_buf.drain(..=pos).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(parsed) = serde_json::from_str::<ChatChunk>(line) else {
                continue;
            };
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
