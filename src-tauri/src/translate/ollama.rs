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

const PARTIAL_SUMMARY_PROMPT: &str = "あなたは優秀な議事録作成者です。\
これは長い会議の文字起こしの一部です。あとで他のパートと統合するための中間メモを日本語の Markdown で作成してください。\
構成: 「### 要点」「### 決定事項」「### TODO」。該当が無いセクションは省略してください。\
文字起こしの誤認識は文脈から自然に補正して構いません。出力はメモ本文のみ。";

const MERGE_SUMMARY_PROMPT: &str = "あなたは優秀な議事録作成者です。\
ユーザーが送るのは、一つの会議の文字起こしを分割して要約した中間メモ群です。\
これらを統合し、日本語で簡潔な最終議事録を Markdown で作成してください。\
構成: 「### 要点」「### 決定事項」「### TODO」。該当が無いセクションは省略し、重複する項目はまとめてください。\
出力は議事録本文のみ。";

/// Context window requested for summarization calls. Ollama's default (2k-4k)
/// silently truncates long transcripts; 8k fits a chunk + prompt + output.
const SUMMARY_NUM_CTX: u32 = 8192;

/// Character budget per summarization request. Japanese averages roughly one
/// token per character, so this stays well inside SUMMARY_NUM_CTX.
const SUMMARY_CHUNK_CHARS: usize = 6000;

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
    options: serde_json::Value,
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
        "options": options,
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

/// Splits text into chunks of at most `max_chars` characters, breaking on
/// line boundaries (one line = one utterance). A single oversized line
/// becomes its own chunk rather than being cut mid-sentence.
fn split_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;
    for line in text.lines() {
        let line_chars = line.chars().count();
        if current_chars > 0 && current_chars + line_chars > max_chars {
            chunks.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        if current_chars > 0 {
            current.push('\n');
            current_chars += 1;
        }
        current.push_str(line);
        current_chars += line_chars;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

async fn summarize_once(model: &str, system: &str, user: &str) -> Result<String, String> {
    let res = chat_request(
        model,
        system,
        user,
        false,
        json!({ "temperature": 0.3, "num_ctx": SUMMARY_NUM_CTX }),
        Some(std::time::Duration::from_secs(300)),
    )
    .await?;
    let value: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    value["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or("Ollama の応答を解釈できません".to_string())
}

const NOTE_SEPARATOR: &str = "\n\n---\n\n";

/// Groups whole notes into batches whose joined length stays within budget.
/// Notes are never split internally, so a "### 決定事項" header can't be
/// severed from its bullets.
fn group_by_budget(notes: Vec<String>, max_chars: usize) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_chars = 0usize;
    for note in notes {
        let len = note.chars().count() + NOTE_SEPARATOR.len();
        if !current.is_empty() && current_chars + len > max_chars {
            groups.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        current_chars += len;
        current.push(note);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Generates a meeting-minutes summary from the full transcript.
/// Long transcripts (1-2h meetings) exceed a small local model's context, so
/// they are map-reduced: interim notes per transcript chunk, then merge
/// passes. When the notes themselves exceed one request's budget (3h+
/// meetings) they are tree-reduced — note groups merged with the merge
/// prompt, never routed back through the transcript prompt.
pub async fn summarize(model: &str, transcript: &str) -> Result<String, String> {
    let chunks = split_chunks(transcript, SUMMARY_CHUNK_CHARS);
    if chunks.len() <= 1 {
        return summarize_once(model, SUMMARY_PROMPT, transcript).await;
    }

    let total = chunks.len();
    let mut notes = Vec::with_capacity(total);
    for (i, chunk) in chunks.iter().enumerate() {
        let user = format!("(パート {}/{})\n{}", i + 1, total, chunk);
        notes.push(summarize_once(model, PARTIAL_SUMMARY_PROMPT, &user).await?);
    }

    loop {
        let merged_input = notes.join(NOTE_SEPARATOR);
        if notes.len() == 1 || merged_input.chars().count() <= SUMMARY_CHUNK_CHARS {
            return summarize_once(model, MERGE_SUMMARY_PROMPT, &merged_input).await;
        }
        let mut reduced = Vec::new();
        for group in group_by_budget(notes, SUMMARY_CHUNK_CHARS) {
            reduced.push(summarize_once(model, MERGE_SUMMARY_PROMPT, &group.join(NOTE_SEPARATOR)).await?);
        }
        notes = reduced;
    }
}

/// Streams a translation of English text into Japanese.
/// Calls on_delta(delta, done) as each token arrives.
pub async fn translate_stream(
    model: &str,
    text: &str,
    mut on_delta: impl FnMut(String, bool),
) -> Result<(), String> {
    let res = chat_request(model, SYSTEM_PROMPT, text, true, json!({ "temperature": 0.0 }), None).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_chunks_respects_line_boundaries() {
        let text = "あ".repeat(10) + "\n" + &"い".repeat(10) + "\n" + &"う".repeat(10);
        let chunks = split_chunks(&text, 25);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "あ".repeat(10) + "\n" + &"い".repeat(10));
        assert_eq!(chunks[1], "う".repeat(10));
    }

    #[test]
    fn split_chunks_short_text_is_single_chunk() {
        let chunks = split_chunks("a\nb\nc", 100);
        assert_eq!(chunks, vec!["a\nb\nc".to_string()]);
    }

    #[test]
    fn split_chunks_oversized_line_stays_whole() {
        let long = "x".repeat(50);
        let chunks = split_chunks(&format!("short\n{long}\nshort"), 20);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1], long);
    }

    #[test]
    fn group_by_budget_keeps_notes_whole() {
        let notes: Vec<String> = vec!["a".repeat(10), "b".repeat(10), "c".repeat(10)];
        // 10 chars + 7-char separator each (17): two fit in 40, not three
        let groups = group_by_budget(notes, 40);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 1);
        // An oversized note still forms its own group instead of being split
        let big = vec!["x".repeat(100)];
        assert_eq!(group_by_budget(big, 30).len(), 1);
    }

    /// Live test against a local Ollama (`cargo test summarize_long -- --ignored`).
    /// Exercises the map-reduce path with a transcript long enough for 3 chunks.
    #[tokio::test]
    #[ignore = "requires a running Ollama with qwen2.5:3b-instruct"]
    async fn summarize_long_transcript_via_map_reduce() {
        let topics = [
            "自分: リリース日は金曜日に確定しましょう。ビルドは木曜の夜に作成します。",
            "相手: QAの結果、ログイン画面に軽微な表示崩れがあります。修正は明日中に可能です。",
            "自分: ドキュメントの更新は田中さんが担当します。締め切りは水曜日です。",
            "相手: 価格改定の告知はリリースと同時に出します。広報チームに共有済みです。",
        ];
        let transcript = (0..500)
            .map(|i| topics[i % topics.len()])
            .collect::<Vec<_>>()
            .join("\n");
        assert!(split_chunks(&transcript, SUMMARY_CHUNK_CHARS).len() >= 3);

        let summary = summarize("qwen2.5:3b-instruct", &transcript)
            .await
            .expect("summarize failed");
        assert!(!summary.is_empty());
        println!("--- merged summary ---\n{summary}");
    }
}
