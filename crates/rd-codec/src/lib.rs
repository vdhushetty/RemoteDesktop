pub mod encoder;
pub mod decoder;
mod yuv;

pub use encoder::Encoder;
pub use decoder::Decoder;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("encoder initialization failed: {0}")]
    EncoderInit(String),
    #[error("decoder initialization failed: {0}")]
    DecoderInit(String),
    #[error("encoding failed: {0}")]
    Encode(String),
    #[error("decoding failed: {0}")]
    Decode(String),
}

/// Video codec configuration
#[derive(Debug, Clone)]
pub struct CodecConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl Default for CodecConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_kbps: 4000,
        }
    }
}

/// Encoded frame output
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub width: u32,
    pub height: u32,
}

/// Decoded frame output in RGBA format
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
