use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("audio device error: {0}")]
    Device(String),
    #[error("opus error: {0}")]
    Codec(String),
    #[error("no audio device available")]
    NoDevice,
}

/// Audio capture configuration
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 1; // mono for remote desktop
pub const FRAME_SIZE: usize = 960; // 20ms at 48kHz

/// Captures audio from the system output (loopback) and encodes to Opus
pub struct AudioCapture {
    encoder: opus::Encoder,
    buffer: Arc<Mutex<Vec<f32>>>,
    _stream: cpal::Stream,
}

impl AudioCapture {
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();

        // Try to get output device for loopback capture
        let device = host.default_output_device()
            .ok_or(AudioError::NoDevice)?;

        tracing::info!(device = %device.description().map(|d| d.name().to_string()).unwrap_or_default(), "audio capture device");

        let config = cpal::StreamConfig {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = buffer.clone();

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                buf_clone.lock().unwrap().extend_from_slice(data);
            },
            |err| {
                tracing::error!(error = %err, "audio capture error");
            },
            None,
        ).map_err(|e| AudioError::Device(format!("{e}")))?;

        stream.play().map_err(|e| AudioError::Device(format!("{e}")))?;

        let encoder = opus::Encoder::new(
            SAMPLE_RATE,
            opus::Channels::Mono,
            opus::Application::LowDelay,
        ).map_err(|e| AudioError::Codec(format!("{e}")))?;

        Ok(Self {
            encoder,
            buffer,
            _stream: stream,
        })
    }

    /// Encode a frame of audio. Returns None if not enough samples yet.
    pub fn encode_frame(&mut self) -> Result<Option<Vec<u8>>, AudioError> {
        let samples: Vec<f32> = {
            let mut buf = self.buffer.lock().unwrap();
            if buf.len() < FRAME_SIZE {
                return Ok(None);
            }
            buf.drain(..FRAME_SIZE).collect()
        };

        let mut output = vec![0u8; 4000]; // max opus frame size
        let len = self.encoder.encode_float(&samples, &mut output)
            .map_err(|e| AudioError::Codec(format!("{e}")))?;

        output.truncate(len);
        Ok(Some(output))
    }
}

/// Decodes Opus audio and plays it through speakers
pub struct AudioPlayback {
    decoder: opus::Decoder,
    playback_buffer: Arc<Mutex<Vec<f32>>>,
    _stream: cpal::Stream,
}

impl AudioPlayback {
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device()
            .ok_or(AudioError::NoDevice)?;

        tracing::info!(device = %device.description().map(|d| d.name().to_string()).unwrap_or_default(), "audio playback device");

        let config = cpal::StreamConfig {
            channels: CHANNELS,
            sample_rate: SAMPLE_RATE,
            buffer_size: cpal::BufferSize::Default,
        };

        let playback_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let buf_clone = playback_buffer.clone();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut buf = buf_clone.lock().unwrap();
                for sample in data.iter_mut() {
                    *sample = buf.pop().unwrap_or(0.0);
                }
            },
            |err| {
                tracing::error!(error = %err, "audio playback error");
            },
            None,
        ).map_err(|e| AudioError::Device(format!("{e}")))?;

        stream.play().map_err(|e| AudioError::Device(format!("{e}")))?;

        let decoder = opus::Decoder::new(SAMPLE_RATE, opus::Channels::Mono)
            .map_err(|e| AudioError::Codec(format!("{e}")))?;

        Ok(Self {
            decoder,
            playback_buffer,
            _stream: stream,
        })
    }

    /// Decode an Opus frame and queue it for playback
    pub fn decode_frame(&mut self, data: &[u8]) -> Result<(), AudioError> {
        let mut output = vec![0f32; FRAME_SIZE];
        let len = self.decoder.decode_float(data, &mut output, false)
            .map_err(|e| AudioError::Codec(format!("{e}")))?;

        output.truncate(len);

        let mut buf = self.playback_buffer.lock().unwrap();
        // Reverse because we pop from the end during playback
        output.reverse();
        buf.extend_from_slice(&output);

        // Limit buffer to 500ms to prevent growing lag
        let max_samples = (SAMPLE_RATE as usize) / 2;
        if buf.len() > max_samples {
            let excess = buf.len() - max_samples;
            buf.drain(..excess);
        }

        Ok(())
    }
}
