use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::sync::mpsc;

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

        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };

        let mut resampler = SincFixedIn::<f32>::new(
            TARGET_SAMPLE_RATE as f64 / source_rate as f64,
            1.1,
            params,
            1024,
            source_channels,
        )
        .context("Failed to create resampler")?;

        let err_tx = tx.clone();

        let stream = match self.sample_format {
            SampleFormat::F32 => self.device.build_input_stream(
                &self.config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(source_channels)
                        .map(|frame| frame.iter().sum::<f32>() / source_channels as f32)
                        .collect();

                    if let Ok(resampled) = resampler.process(&[mono], None) {
                        if let Some(chunk) = resampled.first() {
                            if tx.send(chunk.clone()).is_err() {
                                tracing::warn!("Audio receiver dropped");
                            }
                        }
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
                    let mono: Vec<f32> = data
                        .chunks(source_channels)
                        .map(|frame| {
                            frame.iter().map(|&s| s as f32 / i16::MAX as f32).sum::<f32>()
                                / source_channels as f32
                        })
                        .collect();

                    if let Ok(resampled) = resampler.process(&[mono], None) {
                        if let Some(chunk) = resampled.first() {
                            if tx.send(chunk.clone()).is_err() {
                                tracing::warn!("Audio receiver dropped");
                            }
                        }
                    }
                },
                move |err| {
                    tracing::error!("Audio capture error: {}", err);
                    let _ = err_tx.send(Vec::new());
                },
                None,
            ),
            SampleFormat::U16 => self.device.build_input_stream(
                &self.config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(source_channels)
                        .map(|frame| {
                            frame.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum::<f32>()
                                / source_channels as f32
                        })
                        .collect();

                    if let Ok(resampled) = resampler.process(&[mono], None) {
                        if let Some(chunk) = resampled.first() {
                            if tx.send(chunk.clone()).is_err() {
                                tracing::warn!("Audio receiver dropped");
                            }
                        }
                    }
                },
                move |err| {
                    tracing::error!("Audio capture error: {}", err);
                    let _ = err_tx.send(Vec::new());
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
