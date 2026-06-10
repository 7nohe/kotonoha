use std::sync::Mutex;
use std::time::SystemTime;

use crate::events::SourceKind;

pub struct HistoryEntry {
    pub utterance_id: String,
    pub source: SourceKind,
    pub text: String,
    pub translation: Option<String>,
    pub time: SystemTime,
}

/// Accumulates finalized utterances during a session. Used for export and summary generation.
#[derive(Default)]
pub struct History {
    entries: Mutex<Vec<HistoryEntry>>,
}

impl History {
    pub fn push_final(&self, utterance_id: String, source: SourceKind, text: String) {
        self.entries.lock().unwrap().push(HistoryEntry {
            utterance_id,
            source,
            text,
            translation: None,
            time: SystemTime::now(),
        });
    }

    pub fn set_translation(&self, utterance_id: &str, translation: String) {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.iter_mut().rev().find(|e| e.utterance_id == utterance_id) {
            entry.translation = Some(translation);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// Builds the transcript body as Markdown
    pub fn to_markdown(&self) -> String {
        let entries = self.entries.lock().unwrap();
        let mut out = String::new();
        for e in entries.iter() {
            let time: chrono::DateTime<chrono::Local> = e.time.into();
            let speaker = match e.source {
                SourceKind::Mic => "自分",
                SourceKind::System => "相手",
            };
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

    #[cfg(test)]
    fn push_for_test(&self, id: &str, source: SourceKind, text: &str) {
        self.push_final(id.to_string(), source, text.to_string());
    }

    /// Plain text used for summary generation
    pub fn to_plain_text(&self) -> String {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .map(|e| {
                let speaker = match e.source {
                    SourceKind::Mic => "自分",
                    SourceKind::System => "相手",
                };
                match &e.translation {
                    Some(t) => format!("{speaker}: {} ({t})", e.text),
                    None => format!("{speaker}: {}", e.text),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_includes_speaker_text_and_translation() {
        let h = History::default();
        h.push_for_test("sys-1", SourceKind::System, "Let's ship on July 10th.");
        h.push_for_test("mic-1", SourceKind::Mic, "了解です。");
        h.set_translation("sys-1", "7月10日にリリースしましょう。".to_string());

        let md = h.to_markdown();
        assert!(md.contains("**[") && md.contains("] 相手**: Let's ship on July 10th."));
        assert!(md.contains("> 7月10日にリリースしましょう。"));
        assert!(md.contains("] 自分**: 了解です。"));

        let plain = h.to_plain_text();
        assert!(plain.contains("相手: Let's ship on July 10th. (7月10日にリリースしましょう。)"));
        assert!(plain.contains("自分: 了解です。"));
    }

    #[test]
    fn clear_and_empty() {
        let h = History::default();
        assert!(h.is_empty());
        h.push_for_test("a", SourceKind::Mic, "x");
        assert!(!h.is_empty());
        h.clear();
        assert!(h.is_empty());
    }
}
