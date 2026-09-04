//! Local speech-to-text via whisper.cpp (Plan.txt section 7).
//!
//! ggml models are downloaded from the official whisper.cpp release store
//! on first use, with progress callbacks so the UI can show a download
//! bar. Transcription runs fully locally.

use std::path::{Path, PathBuf};

use crate::VoiceError;

const WHISPER_MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Download URL for a ggml whisper model (e.g. `small` → `ggml-small.bin`).
pub fn model_url(name: &str) -> String {
    format!("{WHISPER_MODEL_BASE_URL}/ggml-{name}.bin")
}

/// Ensure the model file exists locally, downloading it if necessary.
///
/// `progress(received_bytes, total_bytes)` is called for each downloaded
/// chunk (total is `None` when the server does not report a length).
pub async fn ensure_model(
    name: &str,
    dir: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, VoiceError> {
    let path = dir.join(format!("ggml-{name}.bin"));
    if path.exists() {
        return Ok(path);
    }
    tokio::fs::create_dir_all(dir).await?;

    let url = model_url(name);
    let resp = reqwest::get(&url).await?.error_for_status()?;
    let total = resp.content_length();
    let mut file = tokio::io::BufWriter::new(tokio::fs::File::create(&path).await?);

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        progress(received, total);
    }
    file.flush().await?;
    Ok(path)
}

/// whisper.cpp based transcription.
pub struct WhisperStt {
    model_path: PathBuf,
    language: String,
}

impl WhisperStt {
    pub fn new(model_path: PathBuf, language: impl Into<String>) -> Self {
        Self { model_path, language: language.into() }
    }

    /// Transcribe 16 kHz mono i16 PCM. Runs synchronously and CPU-bound —
    /// call from a blocking task.
    pub fn transcribe(&self, pcm: &[i16]) -> Result<String, VoiceError> {
        if pcm.is_empty() {
            return Ok(String::new());
        }

        // whisper-rs 0.13 expects f32 samples.
        let mut samples = vec![0f32; pcm.len()];
        let _ = whisper_rs::convert_integer_to_float_audio(pcm, &mut samples);

        let ctx = whisper_rs::WhisperContext::new_with_params(
            &self.model_path.to_string_lossy(),
            whisper_rs::WhisperContextParameters::default(),
        )
        .map_err(|e| VoiceError::Stt(format!("load model: {e}")))?;

        let mut state = ctx
            .create_state()
            .map_err(|e| VoiceError::Stt(format!("create state: {e}")))?;

        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
            best_of: 1,
        });
        params.set_language(Some(&self.language));
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(false);

        state
            .full(params, &samples)
            .map_err(|e| VoiceError::Stt(format!("transcribe: {e}")))?;

        let segments = state
            .full_n_segments()
            .map_err(|e| VoiceError::Stt(format!("segment count: {e}")))?;

        let mut text = String::new();
        for i in 0..segments {
            if let Ok(seg) = state.full_get_segment_text(i) {
                text.push_str(seg.trim());
            }
        }
        Ok(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_url_format() {
        assert_eq!(
            model_url("small"),
            format!("{WHISPER_MODEL_BASE_URL}/ggml-small.bin")
        );
    }

    #[test]
    fn empty_pcm_returns_empty_text() {
        let stt = WhisperStt::new(PathBuf::from("nonexistent.bin"), "ja");
        assert_eq!(stt.transcribe(&[]).unwrap(), "");
    }
}
