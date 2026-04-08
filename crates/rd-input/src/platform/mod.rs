#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

use crate::{InputError, InputInjector};

pub fn create_platform_injector() -> Result<Box<dyn InputInjector>, InputError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsInjector::new()?))
    }

    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsInjector::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxInjector::new()?))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(InputError::Unsupported)
    }
}
