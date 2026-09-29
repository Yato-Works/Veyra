//! Local speech-to-text via whisper.cpp (Plan.txt section 7).
//!
//! ggml models are downloaded from the official whisper.cpp release store
//! on first use, with progress callbacks so the UI can show a download
//! bar. Transcription runs fully locally.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::VoiceError;

const WHISPER_MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Officially supported whisper ggml models.
const ALLOWED_MODELS: &[&str] = &[
    "tiny",
    "tiny.en",
    "base",
    "base.en",
    "small",
    "small.en",
    "medium",
    "medium.en",
    "large-v1",
    "large-v2",
    "large-v3",
    "large-v3-turbo",
];

/// Maximum download size allowed for a model file (4 GB).
const MAX_MODEL_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Pinned integrity data for every allowed model: (name, SHA-256, byte size),
/// taken from the official whisper.cpp release store (HuggingFace LFS oids).
/// Fresh downloads are rejected unless both the digest and the size match, so
/// a tampered or corrupted model can never reach the transcription engine.
const MODEL_INTEGRITY: &[(&str, &str, u64)] = &[
    ("tiny", "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21", 77_691_713),
    ("tiny.en", "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f", 77_704_715),
    ("base", "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe", 147_951_465),
    ("base.en", "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002", 147_964_211),
    ("small", "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b", 487_601_967),
    ("small.en", "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d", 487_614_201),
    ("medium", "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208", 1_533_763_059),
    ("medium.en", "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356", 1_533_774_781),
    ("large-v1", "7d99f41a10525d0206bddadd86760181fa920438b6b33237e3118ff6c83bb53d", 3_094_623_691),
    ("large-v2", "9a423fe4d40c82774b6af34115b8b935f34152246eb19e80e376071d3f999487", 3_094_623_691),
    ("large-v3", "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2", 3_095_033_483),
    ("large-v3-turbo", "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69", 1_624_555_275),
];

fn expected_integrity(name: &str) -> Option<(&'static str, u64)> {
    MODEL_INTEGRITY
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, sha, size)| (*sha, *size))
}

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
    // Validate model against official whitelist to prevent path traversal and arbitrary downloads
    if !ALLOWED_MODELS.contains(&name) {
        return Err(VoiceError::Stt(format!("disallowed or unknown whisper model: '{name}'")));
    }
    let (expected_sha, expected_size) = expected_integrity(name).ok_or_else(|| {
        VoiceError::Stt(format!("no pinned integrity data for whisper model: '{name}'"))
    })?;

    let path = dir.join(format!("ggml-{name}.bin"));
    if path.exists() {
        // Cheap cache gate: a wrong-sized file is truncated or corrupted, so
        // delete it and fall through to a verified re-download.
        match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.len() == expected_size => return Ok(path),
            _ => {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
    tokio::fs::create_dir_all(dir).await?;

    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = dir.join(format!("ggml-{name}.{unique_id}.download"));
    let url = model_url(name);
    let resp = reqwest::get(&url).await?.error_for_status()?;
    let total = resp.content_length();
    let mut file = tokio::io::BufWriter::new(tokio::fs::File::create(&tmp_path).await?);

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(VoiceError::Network(e));
            }
        };
        received += chunk.len() as u64;
        if received > MAX_MODEL_DOWNLOAD_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(VoiceError::Stt(format!(
                "model download exceeded maximum allowed size of {} bytes",
                MAX_MODEL_DOWNLOAD_BYTES
            )));
        }
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(VoiceError::Io(e));
        }
        progress(received, total);
    }
    if let Err(e) = file.flush().await {
        drop(file);
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(VoiceError::Io(e));
    }
    drop(file);

    let digest = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if !digest.eq_ignore_ascii_case(expected_sha) || received != expected_size {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(VoiceError::Stt(format!(
            "whisper model '{name}' failed integrity check (expected sha256 {expected_sha}, got {digest})"
        )));
    }

    tokio::fs::rename(&tmp_path, &path).await?;
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

    #[tokio::test]
    async fn rejects_invalid_model_names() {
        let dir = tempfile::tempdir().unwrap();
        let res = ensure_model("../bad", dir.path(), |_, _| {}).await;
        assert!(res.is_err());
    }

    #[test]
    fn integrity_table_covers_allowlist() {
        assert_eq!(ALLOWED_MODELS.len(), MODEL_INTEGRITY.len());
        for name in ALLOWED_MODELS {
            let (sha, size) = expected_integrity(name)
                .unwrap_or_else(|| panic!("missing pinned integrity for model '{name}'"));
            assert_eq!(sha.len(), 64, "sha256 must be 64 hex chars for {name}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(size > 0);
        }
    }
}
