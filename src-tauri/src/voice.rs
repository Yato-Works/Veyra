//! Voice subsystem wiring: push-to-talk commands, global hotkey handling,
//! TTS, and whisper model management (Plan.txt sections 7-9).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use veyra_runtime::cleanup::cleanup_transcript;
use veyra_voice::{ensure_model, start_recording, RecordingHandle, WhisperStt};

/// Live voice state: the current push-to-talk recording, if any.
pub struct VoiceRuntime {
    pub recording: Option<RecordingHandle>,
}

/// Transcript delivered to the UI after STT + cleanup.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptPayload {
    pub raw: String,
    pub cleaned: String,
}

/// Start push-to-talk capture. Called by the UI mic button (press) and by
/// the global hotkey (Ctrl+Space press).
pub fn start_pushtotalk(state: &AppState, app: &AppHandle) -> Result<(), String> {
    let mut voice = state.voice.lock().unwrap_or_else(|e| e.into_inner());
    if voice.recording.is_some() {
        return Err("already recording".into());
    }
    let handle = start_recording().map_err(|e| e.to_string())?;
    voice.recording = Some(handle);
    let _ = app.emit("voice-recording", true);
    Ok(())
}

/// Shared STT + cleanup pipeline (used by command and hotkey paths).
pub(crate) async fn transcribe_public(
    state: &AppState,
    app: &AppHandle,
    pcm: Vec<i16>,
    model: Option<String>,
) -> Result<TranscriptPayload, String> {
    transcribe(state, app, pcm, model).await
}

/// Shared STT + cleanup pipeline (used by command and hotkey paths).
async fn transcribe(
    state: &AppState,
    app: &AppHandle,
    pcm: Vec<i16>,
    model: Option<String>,
) -> Result<TranscriptPayload, String> {
    if pcm.is_empty() {
        return Err("no audio captured".into());
    }

    let model_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let model_name = model.unwrap_or_else(|| "tiny".into());

    // Download on first use, streaming progress to the UI.
    let app_for_progress = app.clone();
    let model_path = ensure_model(&model_name, &model_dir, |received, total| {
        let _ = app_for_progress.emit(
            "voice-model-progress",
            serde_json::json!({ "received": received, "total": total }),
        );
    })
    .await
    .map_err(|e| e.to_string())?;

    // whisper.cpp is CPU-bound: run it on a blocking thread.
    let stt = WhisperStt::new(model_path, "ja");
    let raw = tokio::task::spawn_blocking(move || stt.transcribe(&pcm))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let cleaned = cleanup_transcript(&state.router, &raw).await;
    Ok(TranscriptPayload { raw, cleaned })
}

/// Global hotkey (Ctrl+Space) pressed → start capture.
pub fn on_hotkey_press(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Err(e) = start_pushtotalk(&state, app) {
        let _ = app.emit("voice-error", e);
    }
}

/// Global hotkey released → stop + transcribe in the background.
pub fn on_hotkey_release(app: &AppHandle) {
    let pcm = {
        let state = app.state::<AppState>();
        let mut voice = state.voice.lock().unwrap_or_else(|e| e.into_inner());
        match voice.recording.take() {
            Some(handle) => handle.stop().ok(),
            None => return,
        }
    };
    let Some(pcm) = pcm else { return };
    let _ = app.emit("voice-recording", false);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match transcribe(&state, &app, pcm, None).await {
            Ok(payload) => {
                let _ = app.emit("voice-transcript", &payload);
            }
            Err(e) => {
                let _ = app.emit("voice-error", e);
            }
        }
    });
}
