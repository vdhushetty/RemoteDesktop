pub mod frame;

use frame::CapturedFrame;
use xcap::Monitor;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("failed to initialize screen capture: {0}")]
    Init(String),
    #[error("failed to capture frame: {0}")]
    Capture(String),
    #[error("no displays found")]
    NoDisplays,
}

/// Cross-platform screen capturer using xcap
pub struct ScreenCapturer {
    monitor: Monitor,
    width: u32,
    height: u32,
}

impl ScreenCapturer {
    fn new() -> Result<Self, CaptureError> {
        let monitors = Monitor::all().map_err(|e| CaptureError::Init(format!("{e}")))?;
        let monitor = monitors
            .into_iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or_else(|| Monitor::all().ok()?.into_iter().next())
            .ok_or(CaptureError::NoDisplays)?;

        let name = monitor.name().unwrap_or_else(|_| "unknown".into());
        let width = monitor.width().unwrap_or(1920);
        let height = monitor.height().unwrap_or(1080);

        tracing::info!(name = %name, width, height, "screen capture initialized");

        Ok(Self { monitor, width, height })
    }

    pub fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let image = self
            .monitor
            .capture_image()
            .map_err(|e| CaptureError::Capture(format!("{e}")))?;

        let width = image.width();
        let height = image.height();
        let rgba_data = image.into_raw();

        // Convert RGBA to BGRA (our frame format)
        let mut bgra_data = rgba_data;
        for chunk in bgra_data.chunks_exact_mut(4) {
            chunk.swap(0, 2); // R <-> B
        }

        self.width = width;
        self.height = height;
        let stride = width * 4;
        Ok(CapturedFrame::new(bgra_data, width, height, stride))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Create a screen capturer for the current platform
pub fn create_capturer() -> Result<ScreenCapturer, CaptureError> {
    ScreenCapturer::new()
}
