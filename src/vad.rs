#![allow(dead_code)]

use std::path::Path;

/// Simple energy-based VAD for v1. Silero VAD via ort will be added later.
pub struct SileroVad {
    threshold: f32,
    min_speech_samples: usize,
    min_silence_samples: usize,
    speech_count: usize,
    silence_count: usize,
    in_speech: bool,
}

impl SileroVad {
    pub fn new(threshold: f32, min_speech_ms: u32, min_silence_ms: u32) -> anyhow::Result<Self> {
        let min_speech_samples = (min_speech_ms as usize * 16) / 1000;
        let min_silence_samples = (min_silence_ms as usize * 16) / 1000;

        Ok(SileroVad {
            threshold,
            min_speech_samples,
            min_silence_samples,
            speech_count: 0,
            silence_count: 0,
            in_speech: false,
        })
    }

    /// Process a chunk of 16kHz mono f32 audio. Returns a VadEvent.
    pub fn process(&mut self, audio: &[f32]) -> VadEvent {
        let rms: f32 = (audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32).sqrt();

        if rms >= self.threshold {
            self.speech_count += audio.len();
            self.silence_count = 0;
            if !self.in_speech && self.speech_count >= self.min_speech_samples {
                self.in_speech = true;
                return VadEvent::SpeechStart;
            }
        } else {
            self.silence_count += audio.len();
            if self.in_speech && self.silence_count >= self.min_silence_samples {
                self.in_speech = false;
                self.speech_count = 0;
                self.silence_count = 0;
                return VadEvent::SpeechEnd;
            }
        }

        VadEvent::None
    }

    pub fn reset(&mut self) {
        self.speech_count = 0;
        self.silence_count = 0;
        self.in_speech = false;
    }
}

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    None,
    SpeechStart,
    SpeechEnd,
}

pub fn ensure_model() -> anyhow::Result<()> {
    // No model needed for energy-based VAD
    Ok(())
}

pub fn model_path(_name: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(Path::new("unused").to_path_buf())
}
