#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux_x11;

use crate::{CaptureError, Capturer};

pub fn create_platform_capturer() -> Result<Box<dyn Capturer>, CaptureError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsCapturer::new()?))
    }

    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::DxgiCapturer::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux_x11::X11Capturer::new()?))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(CaptureError::Unsupported)
    }
}
