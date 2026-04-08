use crate::frame::CapturedFrame;
use crate::{CaptureError, Capturer};
use core_graphics::display::{CGDisplay, CGPoint, CGRect, CGSize};

pub struct MacOsCapturer {
    display_id: u32,
    width: u32,
    height: u32,
}

impl MacOsCapturer {
    pub fn new() -> Result<Self, CaptureError> {
        let main_display = CGDisplay::main();
        let width = main_display.pixels_wide() as u32;
        let height = main_display.pixels_high() as u32;

        if width == 0 || height == 0 {
            return Err(CaptureError::NoDisplays);
        }

        tracing::info!(
            display_id = main_display.id,
            width,
            height,
            "initialized macOS screen capture"
        );

        Ok(Self {
            display_id: main_display.id,
            width,
            height,
        })
    }
}

impl Capturer for MacOsCapturer {
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let display = CGDisplay::new(self.display_id);

        let image = CGDisplay::screenshot(
            CGRect::new(
                &CGPoint::new(0.0, 0.0),
                &CGSize::new(self.width as f64, self.height as f64),
            ),
            core_graphics::display::kCGWindowListOptionOnScreenOnly,
            core_graphics::display::kCGNullWindowID,
            core_graphics::display::kCGWindowImageDefault,
        )
        .ok_or_else(|| CaptureError::Capture("CGDisplay::screenshot returned None".into()))?;

        let width = image.width() as u32;
        let height = image.height() as u32;
        let stride = image.bytes_per_row() as u32;
        let data = image.data();
        let bytes: Vec<u8> = data.bytes().to_vec();

        // Update dimensions in case display resolution changed
        self.width = width;
        self.height = height;

        Ok(CapturedFrame::new(bytes, width, height, stride))
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
