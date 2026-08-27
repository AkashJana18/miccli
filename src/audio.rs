use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::sync::{mpsc, Arc, Mutex};

pub const TARGET_SAMPLE_RATE: u32 = 16000;

pub struct AudioCapture {
    device: Device,
    config: StreamConfig,
    sample_format: SampleFormat,
}

pub struct AudioStream {
    pub rx: mpsc::Receiver<Vec<f32>>,
    _stream: Stream,
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No input device available")?;

        tracing::info!("Using audio device: {:?}", device.name());

        let supported = device
            .default_input_config()
            .context("No default input config")?;

        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        tracing::info!(
            "Audio config: {}Hz, {} ch, {:?}",
            config.sample_rate.0,
            config.channels,
            sample_format
        );

        Ok(AudioCapture {
            device,
            config,
            sample_format,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn channels(&self) -> u16 {
        self.config.channels
    }

    /// Start capturing audio. Returns an AudioStream with a receiver of 16kHz mono f32 PCM chunks.
    /// The stream keeps running as long as the AudioStream is alive.
    pub fn start_capture(&self) -> Result<AudioStream> {
        let (tx, rx) = mpsc::channel();

        let source_rate = self.sample_rate() as usize;
        let source_channels = self.channels() as usize;

        // cpal delivers input in driver-sized buffers (e.g. 512 frames), but the
        // `SincFixedIn` resampler requires a fixed input block size. We accumulate
        // mono frames into a pending buffer and feed the resampler in fixed blocks.
        const RESAMPLE_BLOCK: usize = 1024;
        let sink = Arc::new(Mutex::new(PendingResampler::new(
            source_rate,
            source_channels,
            RESAMPLE_BLOCK,
        )?));

        let err_tx = tx.clone();

        let (sink2, sink3) = (sink.clone(), sink.clone());
        let (err_tx2, err_tx3) = (err_tx.clone(), err_tx.clone());
        let (tx2, tx3) = (tx.clone(), tx.clone());

        let stream = match self.sample_format {
            SampleFormat::F32 => self.device.build_input_stream(
                &self.config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    for frame in data.chunks(source_channels) {
                        let mono = frame.iter().sum::<f32>() / source_channels as f32;
                        push_sample(mono, &sink, &tx);
                    }
                },
                move |err| {
                    tracing::error!("Audio capture error: {}", err);
                    let _ = err_tx.send(Vec::new());
                },
                None,
            ),
            SampleFormat::I16 => self.device.build_input_stream(
                &self.config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    for frame in data.chunks(source_channels) {
                        let mono =
                            frame.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>()
                                / source_channels as f32;
                        push_sample(mono, &sink2, &tx2);
                    }
                },
                move |err| {
                    tracing::error!("Audio capture error: {}", err);
                    let _ = err_tx2.send(Vec::new());
                },
                None,
            ),
            SampleFormat::U16 => self.device.build_input_stream(
                &self.config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    for frame in data.chunks(source_channels) {
                        let mono = frame
                            .iter()
                            .map(|&s| (s as f32 - 32768.0) / 32768.0)
                            .sum::<f32>()
                            / source_channels as f32;
                        push_sample(mono, &sink3, &tx3);
                    }
                },
                move |err| {
                    tracing::error!("Audio capture error: {}", err);
                    let _ = err_tx3.send(Vec::new());
                },
                None,
            ),
            _ => anyhow::bail!("Unsupported sample format: {:?}", self.sample_format),
        }
        .context("Failed to build input stream")?;

        stream.play().context("Failed to start audio stream")?;

        Ok(AudioStream { rx, _stream: stream })
    }
}

/// Push one mono frame of raw audio into the shared resampler sink; any
/// resampled output produced is forwarded to the receiver.
fn push_sample(
    sample: f32,
    sink: &Arc<Mutex<PendingResampler>>,
    tx: &mpsc::Sender<Vec<f32>>,
) {
    let mut sink = sink.lock().unwrap();
    if let Some(chunk) = sink.push(sample) {
        if tx.send(chunk).is_err() {
            tracing::warn!("Audio receiver dropped");
        }
    }
}

/// Buffers incoming mono samples and feeds them to a fixed-input resampler in
/// blocks, returning any resampled output for forwarding to the receiver.
struct PendingResampler {
    resampler: SincFixedIn<f32>,
    pending: Vec<f32>,
    block: usize,
}

impl PendingResampler {
    fn new(source_rate: usize, channels: usize, block: usize) -> Result<Self> {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = SincFixedIn::<f32>::new(
            TARGET_SAMPLE_RATE as f64 / source_rate as f64,
            1.1,
            params,
            block,
            channels,
        )
        .context("Failed to create resampler")?;
        Ok(PendingResampler {
            resampler,
            pending: Vec::with_capacity(block),
            block,
        })
    }

    /// Push one mono sample; returns a resampled chunk if a full block was filled.
    fn push(&mut self, sample: f32) -> Option<Vec<f32>> {
        self.pending.push(sample);
        if self.pending.len() >= self.block {
            let input = std::mem::replace(&mut self.pending, Vec::with_capacity(self.block));
            let resampled = self
                .resampler
                .process(&[input], None)
                .ok()?;
            resampled.into_iter().next().filter(|c| !c.is_empty())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use std::time::Duration;

    /// Captures a short burst of real audio from the default input device and
    /// asserts that frames actually arrive. Run with `--ignored` since it needs
    /// a working microphone:
    ///     cargo test --release capture_frames -- --ignored --nocapture
    #[test]
    #[ignore]
    fn capture_frames() {
        let capture = AudioCapture::new().expect("open audio");
        let stream = capture.start_capture().expect("start audio");
        let rx = stream.rx;

        let mut total_frames = 0usize;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            while let Ok(chunk) = rx.try_recv() {
                total_frames += chunk.len();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("captured {} frames in 3s", total_frames);
        assert!(total_frames > 0, "no audio frames captured");
    }
}
