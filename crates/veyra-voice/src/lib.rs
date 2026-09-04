//! Veyra voice: TTS (Style-Bert-VITS2) and STT (whisper.cpp) clients.
//!
//! Design mirrors the proven pattern from AIStoryActingEngine:
//! - The TTS model runs in a **separate process** (the Style-Bert-VITS2
//!   FastAPI server); this crate only speaks HTTP to it.
//! - STT runs whisper.cpp in-process with a ggml model that is downloaded
//!   on first use.
//! - Audio capture is push-to-talk based (Plan.txt AC-002).

pub mod recorder;
pub mod stt;
pub mod tts;

pub use recorder::{start_recording, RecordingHandle};
pub use stt::{ensure_model, model_url, WhisperStt};
pub use tts::{build_voice_query, Sbv2Client, Sbv2Params};

/// Errors raised by the voice subsystem.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("audio error: {0}")]
    Audio(String),
    #[error("stt error: {0}")]
    Stt(String),
    #[error("tts error: {0}")]
    Tts(String),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
