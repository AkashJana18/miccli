use anyhow::{Context, Result};
use ndarray::Array3;
use ort::session::Session;
use ort::value::TensorRef;
use std::path::PathBuf;
use std::sync::LazyLock;

const CHUNK_SIZE: usize = 512;
const CONTEXT_SIZE: usize = 64;
const STATE_SIZE: usize = 128;
const SAMPLE_RATE: i64 = 16000;

static MODEL_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("miccli")
});

pub fn model_path() -> PathBuf {
    MODEL_DIR.join("silero_vad.onnx")
}

pub fn ensure_model() -> Result<PathBuf> {
    let path = model_path();
    if path.exists() {
        return Ok(path);
    }

    let dir = path.parent().context("No model parent dir")?;
    std::fs::create_dir_all(dir)?;

    println!("Downloading Silero VAD model...");
    let url = "https://github.com/snakers4/silero-vad/raw/refs/tags/v5.0/files/silero_vad.onnx";
    let bytes = reqwest::blocking::get(url)
        .context("Failed to download Silero VAD model")?
        .bytes()
        .context("Failed to read model bytes")?;
    std::fs::write(&path, &bytes)?;
    println!("Downloaded Silero VAD to {}", path.display());

    Ok(path)
}

pub struct SileroVad {
    session: Session,
    threshold: f32,
    state: [[f32; STATE_SIZE]; 2],
    context: [f32; CONTEXT_SIZE],
    speech_count: usize,
    silence_count: usize,
    in_speech: bool,
    min_speech_samples: usize,
    min_silence_samples: usize,
}

impl SileroVad {
    pub fn new(threshold: f32, min_speech_ms: u32, min_silence_ms: u32) -> Result<Self> {
        let path = ensure_model()?;
        let session = Session::builder()?
            .commit_from_file(&path)
            .context("Failed to load Silero VAD ONNX model")?;

        let min_speech_samples = (min_speech_ms as usize * 16) / 1000;
        let min_silence_samples = (min_silence_ms as usize * 16) / 1000;

        Ok(SileroVad {
            session,
            threshold,
            state: [[0f32; STATE_SIZE]; 2],
            context: [0f32; CONTEXT_SIZE],
            speech_count: 0,
            silence_count: 0,
            in_speech: false,
            min_speech_samples,
            min_silence_samples,
        })
    }

    pub fn process(&mut self, audio: &[f32]) -> Result<VadEvent> {
        let mut event = VadEvent::None;

        for chunk in audio.chunks(CHUNK_SIZE) {
            if chunk.len() < CHUNK_SIZE {
                break;
            }

            let prob = self.process_chunk(chunk)?;

            if prob >= self.threshold {
                self.speech_count += chunk.len();
                self.silence_count = 0;
                if !self.in_speech && self.speech_count >= self.min_speech_samples {
                    self.in_speech = true;
                    event = VadEvent::SpeechStart;
                }
            } else {
                self.silence_count += chunk.len();
                if self.in_speech && self.silence_count >= self.min_silence_samples {
                    self.in_speech = false;
                    self.speech_count = 0;
                    self.silence_count = 0;
                    event = VadEvent::SpeechEnd;
                }
            }
        }

        Ok(event)
    }

    fn process_chunk(&mut self, chunk: &[f32]) -> Result<f32> {
        debug_assert_eq!(chunk.len(), CHUNK_SIZE);

        // Build input: [1, 576] = context[64] + chunk[512]
        let mut input = [0f32; CHUNK_SIZE + CONTEXT_SIZE];
        input[..CONTEXT_SIZE].copy_from_slice(&self.context);
        input[CONTEXT_SIZE..].copy_from_slice(chunk);

        // Update rolling context: take last 64 samples of this chunk
        self.context.copy_from_slice(&chunk[CHUNK_SIZE - CONTEXT_SIZE..]);

        // Create input tensor [1, 576]
        let input_tensor = ndarray::Array2::from_shape_vec(
            (1, CHUNK_SIZE + CONTEXT_SIZE),
            input.to_vec(),
        )?;

        // Create state tensor [2, 1, 128]
        let state_data: Vec<f32> = self.state.iter().flatten().copied().collect();
        let state_tensor = Array3::from_shape_vec((2, 1, STATE_SIZE), state_data)?;

        // Create sample rate tensor (scalar stored as [1])
        let sr_tensor = ndarray::arr1(&[SAMPLE_RATE]);

        // Run inference — inputs! no longer returns Result
        let outputs = self.session.run(ort::inputs![
            "input" => TensorRef::from_array_view(&input_tensor)?,
            "state" => TensorRef::from_array_view(&state_tensor)?,
            "sr" => TensorRef::from_array_view(&sr_tensor)?,
        ])?;

        // Extract speech probability [1, 1]
        let prob = outputs["output"]
            .try_extract_tensor::<f32>()?
            .1[0];

        // Extract updated state [2, 1, 128]
        if let Some(state_out) = outputs.get("stateN") {
            let (_shape, state_data) = state_out.try_extract_tensor::<f32>()?;
            let flat: Vec<f32> = state_data.to_vec();
            for (i, val) in flat.iter().enumerate() {
                let row = i / STATE_SIZE;
                let col = i % STATE_SIZE;
                if row < 2 && col < STATE_SIZE {
                    self.state[row][col] = *val;
                }
            }
        }

        Ok(prob)
    }

    pub fn reset(&mut self) {
        self.state = [[0f32; STATE_SIZE]; 2];
        self.context = [0f32; CONTEXT_SIZE];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_path() {
        let path = model_path();
        assert!(path.to_string_lossy().contains("silero_vad.onnx"));
        assert!(path.to_string_lossy().contains(".config/miccli"));
    }

    #[test]
    fn test_context_buffer_concatenation() {
        let context = [1.0f32; CONTEXT_SIZE];
        let chunk = [2.0f32; CHUNK_SIZE];
        let mut input = [0f32; CHUNK_SIZE + CONTEXT_SIZE];
        input[..CONTEXT_SIZE].copy_from_slice(&context);
        input[CONTEXT_SIZE..].copy_from_slice(&chunk);

        assert_eq!(&input[..CONTEXT_SIZE], &[1.0f32; CONTEXT_SIZE]);
        assert_eq!(&input[CONTEXT_SIZE..], &[2.0f32; CHUNK_SIZE]);
    }

    #[test]
    fn test_context_update() {
        let mut context = [0f32; CONTEXT_SIZE];
        let mut full_chunk = [0.0f32; CHUNK_SIZE];
        full_chunk[CHUNK_SIZE - 1] = 9.9;

        context.copy_from_slice(&full_chunk[CHUNK_SIZE - CONTEXT_SIZE..]);
        assert!((context[CONTEXT_SIZE - 1] - 9.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_vad_event_debug() {
        assert_eq!(format!("{:?}", VadEvent::SpeechStart), "SpeechStart");
        assert_eq!(format!("{:?}", VadEvent::None), "None");
    }
}
