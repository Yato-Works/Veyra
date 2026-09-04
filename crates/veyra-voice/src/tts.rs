//! TTS client for Style-Bert-VITS2 (Plan.txt sections 4, 7).
//!
//! The model itself runs in a separate process (the Style-Bert-VITS2
//! FastAPI server, default `http://127.0.0.1:5000`). This client only
//! speaks HTTP, following the parameter recipe proven in production by
//! AIStoryActingEngine (`tts.py`):
//!
//! - `style_weight = 0.4 + 0.8 × intensity`
//! - `length = 1.0 / pace`, clamped to `[0.5, 2.0]` (SBV2: larger = slower)
//! - fixed synthesis knobs: `sdp_ratio=0.2, noise=0.6, noise_w=0.8`
//! - `auto_split=true, split_interval=0.5, language=JP`

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::VoiceError;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:5000";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Speech synthesis parameters (the "Performance" of a TTS request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sbv2Params {
    /// Model directory name under `model_assets/` (e.g. `jvnv-M1-jp`).
    pub model_name: String,
    /// Emotion style (e.g. `Neutral`, `Joyful`, `Sad`).
    pub style: String,
    /// How strongly the style is applied (0.0–1.0).
    pub intensity: f32,
    /// Speaking pace multiplier (1.0 = normal).
    pub pace: f32,
    pub speaker_id: i32,
}

impl Default for Sbv2Params {
    fn default() -> Self {
        Self {
            model_name: "jvnv-M1-jp".into(),
            style: "Neutral".into(),
            intensity: 0.5,
            pace: 1.0,
            speaker_id: 0,
        }
    }
}

/// Build the `POST /voice` query parameters (pure, unit-testable).
pub fn build_voice_query(text: &str, p: &Sbv2Params) -> Vec<(String, String)> {
    let length = (1.0 / p.pace.max(0.1)).clamp(0.5, 2.0);
    let style_weight = (0.4 + 0.8 * p.intensity).min(1.0);
    vec![
        ("text".into(), text.into()),
        ("model_name".into(), p.model_name.clone()),
        ("speaker_id".into(), p.speaker_id.to_string()),
        ("style".into(), p.style.clone()),
        ("style_weight".into(), format!("{style_weight:.2}")),
        ("sdp_ratio".into(), "0.2".into()),
        ("noise".into(), "0.6".into()),
        ("noise_w".into(), "0.8".into()),
        ("length".into(), format!("{length:.3}")),
        ("auto_split".into(), "true".into()),
        ("split_interval".into(), "0.5".into()),
        ("language".into(), "JP".into()),
    ]
}

/// HTTP client for a running Style-Bert-VITS2 server.
pub struct Sbv2Client {
    base_url: String,
    http: reqwest::Client,
}

impl Default for Sbv2Client {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl Sbv2Client {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Synthesize `text` and return raw WAV bytes.
    pub async fn synthesize(
        &self,
        text: &str,
        params: &Sbv2Params,
    ) -> Result<Vec<u8>, VoiceError> {
        let url = format!("{}/voice", self.base_url);
        let resp = self
            .http
            .post(&url)
            .query(&build_voice_query(text, params))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?
            .error_for_status()?;
        let bytes = resp.bytes().await?;
        if bytes.starts_with(b"RIFF") {
            Ok(bytes.to_vec())
        } else {
            // Server errors are returned as text with 200 in some builds.
            let preview: String = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into();
            Err(VoiceError::Tts(format!(
                "unexpected response (not WAV): {preview}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_query_follows_ai_story_engine_recipe() {
        let params = Sbv2Params {
            model_name: "jvnv-F1-jp".into(),
            style: "Joyful".into(),
            intensity: 0.5,
            pace: 1.0,
            speaker_id: 0,
        };
        let q = build_voice_query("こんにちは", &params);
        let get = |k: &str| q.iter().find(|(key, _)| key == k).unwrap().1.clone();

        assert_eq!(get("text"), "こんにちは");
        assert_eq!(get("model_name"), "jvnv-F1-jp");
        assert_eq!(get("style"), "Joyful");
        assert_eq!(get("style_weight"), "0.80"); // 0.4 + 0.8 × 0.5
        assert_eq!(get("length"), "1.000"); // 1.0 / 1.0
        assert_eq!(get("sdp_ratio"), "0.2");
        assert_eq!(get("noise"), "0.6");
        assert_eq!(get("noise_w"), "0.8");
        assert_eq!(get("auto_split"), "true");
        assert_eq!(get("language"), "JP");
    }

    #[test]
    fn length_clamped_and_style_weight_capped() {
        let fast = Sbv2Params { pace: 5.0, intensity: 2.0, ..Default::default() };
        let q = build_voice_query("x", &fast);
        let get = |k: &str| q.iter().find(|(key, _)| key == k).unwrap().1.clone();
        // pace 5.0 → length 0.2 → clamped to 0.5
        assert_eq!(get("length"), "0.500");
        // intensity 2.0 → style_weight 2.0 → capped at 1.0
        assert_eq!(get("style_weight"), "1.00");
    }
}
