//! Push-to-talk audio capture (Plan.txt AC-002).
//!
//! Captures from the default input device at its native rate, then on
//! stop: downmixes to mono, resamples to 16 kHz mono and converts to the
//! i16 PCM that whisper.cpp expects.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::VoiceError;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// A live recording. Call [`RecordingHandle::stop`] to end the capture.
///
/// The cpal stream lives on a dedicated worker thread because
/// `cpal::Stream` is not `Send`; the handle itself (shared sample buffer +
/// stop signal) is Send and can live in shared app state.
pub struct RecordingHandle {
    samples: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    sample_rate: u32,
    stop_signal: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl RecordingHandle {
    /// True while the capture is running.
    pub fn is_active(&self) -> bool {
        !self.stop_signal.load(Ordering::SeqCst)
    }

    /// Stop capturing and return 16 kHz mono i16 PCM ready for STT.
    pub fn stop(mut self) -> Result<Vec<i16>, VoiceError> {
        self.stop_signal.store(true, Ordering::SeqCst);
        // Wait for the worker to drop the stream, then process.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }

        let samples = self.samples.lock().unwrap().clone();
        let mono = downmix_to_mono(&samples, self.channels);
        let resampled = resample_to_16k(&mono, self.sample_rate);
        Ok(f32_to_i16(&resampled))
    }
}

/// Start capturing from the default microphone.
///
/// cpal streams/devices are not `Send`, so the entire device-open and
/// capture lifecycle runs inside a dedicated worker thread. The returned
/// handle holds only `Send` state (shared sample buffer + stop flag) plus
/// the worker join handle.
pub fn start_recording() -> Result<RecordingHandle, VoiceError> {
    // Query metadata on the calling thread (primitive values are Send).
    let host = cpal::default_host();
    let meta_device = host
        .default_input_device()
        .ok_or_else(|| VoiceError::Audio("no input device found".into()))?;
    let config = meta_device
        .default_input_config()
        .map_err(|e| VoiceError::Audio(format!("input config: {e}")))?;
    let cfg = config.config();
    let channels = cfg.channels as usize;
    let sample_rate = cfg.sample_rate.0;

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let stop_signal = Arc::new(AtomicBool::new(false));
    let sink = samples.clone();
    let stop_flag = stop_signal.clone();

    let worker = std::thread::spawn(move || {
        // Everything non-Send lives only in this thread.
        let Some(device) = cpal::default_host().default_input_device() else {
            return;
        };
        let Ok(config) = device.default_input_config() else {
            return;
        };
        let cfg = config.config();
        let err_cb = move |err| {
            eprintln!("veyra-voice: input stream error: {err}")
        };
        let Ok(stream) = (match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg,
                move |data: &[f32], _| sink.lock().unwrap().extend_from_slice(data),
                err_cb,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |data: &[i16], _| {
                    let mut s = sink.lock().unwrap();
                    s.extend(data.iter().map(|&x| x as f32 / 32768.0));
                },
                err_cb,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &cfg,
                move |data: &[u16], _| {
                    let mut s = sink.lock().unwrap();
                    s.extend(data.iter().map(|&x| (x as f32 - 32768.0) / 32768.0));
                },
                err_cb,
                None,
            ),
            other => {
                eprintln!("veyra-voice: unsupported sample format: {other:?}");
                return;
            }
        }) else {
            return;
        };
        stream.play().ok();
        while !stop_flag.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Dropping the stream ends the callbacks.
    });

    Ok(RecordingHandle {
        samples,
        channels,
        sample_rate,
        stop_signal,
        worker: Some(worker),
    })
}

/// Average interleaved channels into a single mono track.
pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Linear-interpolation resample to 16 kHz (whisper.cpp input rate).
pub fn resample_to_16k(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == TARGET_SAMPLE_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(samples.len() - 1);
        let frac = (pos - i0 as f64) as f32;
        out.push(samples[i0] * (1.0 - frac) + samples[i1] * frac);
    }
    out
}

/// Convert f32 samples [-1.0, 1.0] to clamped i16 PCM.
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        let stereo = vec![0.0, 1.0, 0.5, 0.5];
        let mono = downmix_to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.5, 0.5]);
    }

    #[test]
    fn resample_changes_length_proportionally() {
        // 48 kHz → 16 kHz = 1/3 length.
        let input = vec![0.0f32; 4800];
        let out = resample_to_16k(&input, 48_000);
        assert_eq!(out.len(), 1600);
    }

    #[test]
    fn resample_same_rate_is_noop() {
        let input = vec![0.1f32, 0.2, 0.3];
        assert_eq!(resample_to_16k(&input, 16_000), input);
    }

    #[test]
    fn i16_conversion_clamps() {
        let out = f32_to_i16(&[-2.0, 0.0, 2.0]);
        assert_eq!(out, vec![-32767, 0, 32767]);
    }
}
