//! Input normalization (Plan.txt section 6).
//!
//! Veyra accepts Voice, Text, Hotkey, Clipboard, UI selection, application
//! state and file state. All input is normalized into a common
//! [`InputEvent`] structure before entering the Runtime.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Where an input came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    Voice,
    Text,
    Hotkey,
    Clipboard,
    UiSelection,
    ApplicationState,
    FileState,
}

/// Normalized content of an input event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputContent {
    Text(String),
    VoiceTranscript(String),
    Hotkey(String),
    Clipboard(String),
}

/// The common structure every input is normalized into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputEvent {
    pub id: String,
    pub source: InputSource,
    pub content: InputContent,
    pub timestamp: DateTime<Utc>,
    /// Optional hint about the active application / file context.
    pub context_hint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_event_round_trips() {
        let e = InputEvent {
            id: "in-1".into(),
            source: InputSource::Voice,
            content: InputContent::VoiceTranscript("えーっと今日のミーティング".into()),
            timestamp: Utc::now(),
            context_hint: Some("notepad.exe".into()),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["source"], serde_json::json!("voice"));
        let back: InputEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }
}
