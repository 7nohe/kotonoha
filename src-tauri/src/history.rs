use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::events::SourceKind;

#[derive(Clone)]
pub struct HistoryEntry {
    pub utterance_id: String,
    pub source: SourceKind,
    pub text: String,
    pub translation: Option<String>,
    pub time: SystemTime,
}

/// One line in a session .jsonl file. Append-only so a crash mid-meeting
/// loses at most the in-flight utterance; translations arrive after their
/// utterance and are replayed onto it on load.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SessionRecord {
    Utterance {
        utterance_id: String,
        source: SourceKind,
        text: String,
        time_ms: u64,
    },
    Translation {
        utterance_id: String,
        text: String,
    },
}

fn time_to_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn ms_to_time(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}

/// Attaches a translation to the newest matching utterance. Shared by the
/// live path and session replay so the two can never diverge.
fn apply_translation(entries: &mut [HistoryEntry], utterance_id: &str, translation: String) -> bool {
    match entries.iter_mut().rev().find(|e| e.utterance_id == utterance_id) {
        Some(entry) => {
            entry.translation = Some(translation);
            true
        }
        None => false,
    }
}

#[derive(Default)]
struct SessionState {
    entries: Vec<HistoryEntry>,
    /// Opened lazily on the first write so empty sessions never hit disk
    writer: Option<File>,
    path: Option<PathBuf>,
}

impl SessionState {
    fn write_record(&mut self, record: &SessionRecord) {
        if self.writer.is_none() {
            let Some(path) = self.path.as_ref() else { return };
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => self.writer = Some(file),
                Err(e) => {
                    // Persistence failing must not break live captioning;
                    // clear the path so we don't retry on every utterance
                    eprintln!("[history] cannot open session file {}: {e}", path.display());
                    self.path = None;
                    return;
                }
            }
        }
        if let Some(file) = self.writer.as_mut() {
            if let Ok(mut line) = serde_json::to_string(record) {
                line.push('\n');
                let _ = file.write_all(line.as_bytes());
            }
        }
    }
}

/// Accumulates finalized utterances during a session. Used for export and
/// summary generation. Each session is also written through to a JSONL file
/// so past meetings can be exported later (see `list_sessions`).
#[derive(Default)]
pub struct History {
    session: Mutex<SessionState>,
}

impl History {
    /// Starts a new session: clears in-memory entries and points the writer
    /// at a fresh JSONL path (created on first write).
    pub fn rotate_session(&self, app: &AppHandle) -> Result<(), String> {
        let path = new_session_path(app)?;
        let mut session = self.session.lock().unwrap();
        *session = SessionState {
            path: Some(path),
            ..SessionState::default()
        };
        Ok(())
    }

    pub fn push_final(&self, utterance_id: String, source: SourceKind, text: String) {
        let time = SystemTime::now();
        let mut session = self.session.lock().unwrap();
        session.entries.push(HistoryEntry {
            utterance_id: utterance_id.clone(),
            source,
            text: text.clone(),
            translation: None,
            time,
        });
        session.write_record(&SessionRecord::Utterance {
            utterance_id,
            source,
            text,
            time_ms: time_to_ms(time),
        });
    }

    pub fn set_translation(&self, utterance_id: &str, translation: String) {
        let mut session = self.session.lock().unwrap();
        // A translation can outlive its session (in-flight during rotation);
        // only persist it into the file that holds the matching utterance
        if apply_translation(&mut session.entries, utterance_id, translation.clone()) {
            session.write_record(&SessionRecord::Translation {
                utterance_id: utterance_id.to_string(),
                text: translation,
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.session.lock().unwrap().entries.is_empty()
    }

    pub fn snapshot(&self) -> Vec<HistoryEntry> {
        self.session.lock().unwrap().entries.clone()
    }
}

/// Builds the transcript body as Markdown
pub fn to_markdown(entries: &[HistoryEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        let time: chrono::DateTime<chrono::Local> = e.time.into();
        let speaker = e.source.speaker_ja();
        out.push_str(&format!(
            "**[{}] {}**: {}\n",
            time.format("%H:%M:%S"),
            speaker,
            e.text
        ));
        if let Some(t) = &e.translation {
            out.push_str(&format!("> {}\n", t));
        }
        out.push('\n');
    }
    out
}

/// Plain text used for summary generation
pub fn to_plain_text(entries: &[HistoryEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let speaker = e.source.speaker_ja();
            match &e.translation {
                Some(t) => format!("{speaker}: {} ({t})", e.text),
                None => format!("{speaker}: {}", e.text),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- Stored sessions ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// File stem, e.g. "20260719-140302"
    pub id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub utterances: usize,
}

fn sessions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("sessions");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn new_session_path(app: &AppHandle) -> Result<PathBuf, String> {
    let name = format!("{}.jsonl", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    Ok(sessions_dir(app)?.join(name))
}

fn session_file(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    // Ids come back from the frontend; never let them escape the sessions dir
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("不正なセッションIDです".into());
    }
    Ok(sessions_dir(app)?.join(format!("{id}.jsonl")))
}

fn read_session(path: &Path) -> Result<Vec<HistoryEntry>, String> {
    let file = File::open(path).map_err(|e| format!("セッションを読み込めません: {e}"))?;
    let mut entries: Vec<HistoryEntry> = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        // Tolerate a torn last line from a crashed session
        let Ok(record) = serde_json::from_str::<SessionRecord>(line.trim()) else {
            continue;
        };
        match record {
            SessionRecord::Utterance {
                utterance_id,
                source,
                text,
                time_ms,
            } => entries.push(HistoryEntry {
                utterance_id,
                source,
                text,
                translation: None,
                time: ms_to_time(time_ms),
            }),
            SessionRecord::Translation { utterance_id, text } => {
                apply_translation(&mut entries, &utterance_id, text);
            }
        }
    }
    Ok(entries)
}

/// Prefix every utterance line starts with (we are the only writer, and serde
/// puts the `kind` tag first) — lets the listing count without a full parse
const UTTERANCE_PREFIX: &str = r#"{"kind":"utterance""#;

/// Lists stored sessions, newest first, without deserializing their contents:
/// the id encodes the start time, the mtime approximates the end, and
/// utterances are counted by line prefix. Files a torn write left without any
/// utterance are hidden.
pub fn list_sessions(app: &AppHandle) -> Result<Vec<SessionInfo>, String> {
    let dir = sessions_dir(app)?;
    let mut sessions = Vec::new();
    for dent in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let Ok(dent) = dent else { continue };
        let path = dent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(started) = chrono::NaiveDateTime::parse_from_str(id, "%Y%m%d-%H%M%S")
            .ok()
            .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let utterances = content.lines().filter(|l| l.starts_with(UTTERANCE_PREFIX)).count();
        if utterances == 0 {
            continue;
        }
        let started_at_ms = started.timestamp_millis() as u64;
        let ended_at_ms = dent
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(time_to_ms)
            .unwrap_or(started_at_ms);
        sessions.push(SessionInfo {
            id: id.to_string(),
            started_at_ms,
            ended_at_ms,
            utterances,
        });
    }
    sessions.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(sessions)
}

pub fn load_session(app: &AppHandle, id: &str) -> Result<Vec<HistoryEntry>, String> {
    read_session(&session_file(app, id)?)
}

pub fn delete_session(app: &AppHandle, id: &str) -> Result<(), String> {
    std::fs::remove_file(session_file(app, id)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with_path(path: &Path) -> History {
        History {
            session: Mutex::new(SessionState {
                path: Some(path.to_path_buf()),
                ..SessionState::default()
            }),
        }
    }

    #[test]
    fn markdown_includes_speaker_text_and_translation() {
        let h = History::default();
        h.push_final("sys-1".into(), SourceKind::System, "Let's ship on July 10th.".into());
        h.push_final("mic-1".into(), SourceKind::Mic, "了解です。".into());
        h.set_translation("sys-1", "7月10日にリリースしましょう。".to_string());

        let entries = h.snapshot();
        let md = to_markdown(&entries);
        assert!(md.contains("**[") && md.contains("] 相手**: Let's ship on July 10th."));
        assert!(md.contains("> 7月10日にリリースしましょう。"));
        assert!(md.contains("] 自分**: 了解です。"));

        let plain = to_plain_text(&entries);
        assert!(plain.contains("相手: Let's ship on July 10th. (7月10日にリリースしましょう。)"));
        assert!(plain.contains("自分: 了解です。"));
    }

    #[test]
    fn empty_state() {
        let h = History::default();
        assert!(h.is_empty());
        h.push_final("a".into(), SourceKind::Mic, "x".into());
        assert!(!h.is_empty());
    }

    #[test]
    fn session_records_roundtrip_via_jsonl() {
        let dir = std::env::temp_dir().join(format!("kotonoha-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");

        let h = history_with_path(&path);
        h.push_final("sys-1".into(), SourceKind::System, "hello".into());
        h.push_final("mic-1".into(), SourceKind::Mic, "こんにちは".into());
        h.set_translation("sys-1", "やあ".into());
        // Translation for an unknown utterance must not be persisted
        h.set_translation("sys-999", "捨てられる".into());
        drop(h);

        let entries = read_session(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "hello");
        assert_eq!(entries[0].translation.as_deref(), Some("やあ"));
        assert_eq!(entries[1].text, "こんにちは");
        assert_eq!(entries[1].translation, None);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_session_never_touches_disk() {
        let dir = std::env::temp_dir().join(format!("kotonoha-lazy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");

        let h = history_with_path(&path);
        // Only a stray translation arrives (no matching utterance): no file
        h.set_translation("sys-1", "訳".into());
        assert!(!path.exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_session_tolerates_torn_last_line() {
        let dir = std::env::temp_dir().join(format!("kotonoha-torn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        std::fs::write(
            &path,
            "{\"kind\":\"utterance\",\"utterance_id\":\"mic-1\",\"source\":\"mic\",\"text\":\"x\",\"time_ms\":1}\n{\"kind\":\"utt",
        )
        .unwrap();
        let entries = read_session(&path).unwrap();
        assert_eq!(entries.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn utterance_prefix_matches_serialization() {
        let line = serde_json::to_string(&SessionRecord::Utterance {
            utterance_id: "mic-1".into(),
            source: SourceKind::Mic,
            text: "x".into(),
            time_ms: 1,
        })
        .unwrap();
        assert!(line.starts_with(UTTERANCE_PREFIX));
    }
}
