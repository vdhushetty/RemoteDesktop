pub mod frame;
pub mod platform;

use frame::CapturedFrame;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("failed to initialize screen capture: {0}")]
    Init(String),
    #[error("failed to capture frame: {0}")]
    Capture(String),
    #[error("no displays found")]
    NoDisplays,
    #[error("platform not supported")]
    Unsupported,
}

/// Trait for platform-specific screen capture implementations
pub trait Capturer: Send {
    /// Capture a single frame from the screen.
    /// Returns BGRA pixel data.
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError>;

    /// Get the dimensions of the capture target
    fn dimensions(&self) -> (u32, u32);
}

/// Create a screen capturer for the current platform
pub fn create_capturer() -> Result<Box<dyn Capturer>, CaptureError> {
    platform::create_platform_capturer()
}
