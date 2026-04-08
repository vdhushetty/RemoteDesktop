use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard access failed: {0}")]
    Access(String),
    #[error("clipboard not available")]
    Unavailable,
}

/// Clipboard content that can be synced
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Empty,
}

/// Watches the local clipboard for changes and provides sync capabilities
pub struct ClipboardSync {
    last_content: Arc<Mutex<ClipboardContent>>,
}

impl ClipboardSync {
    pub fn new() -> Result<Self, ClipboardError> {
        Ok(Self {
            last_content: Arc::new(Mutex::new(ClipboardContent::Empty)),
        })
    }

    /// Read the current clipboard content
    pub fn read(&self) -> Result<ClipboardContent, ClipboardError> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| ClipboardError::Access(format!("{e}")))?;

        match clipboard.get_text() {
            Ok(text) if !text.is_empty() => Ok(ClipboardContent::Text(text)),
            _ => Ok(ClipboardContent::Empty),
        }
    }

    /// Write content to the local clipboard
    pub fn write(&self, content: &ClipboardContent) -> Result<(), ClipboardError> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| ClipboardError::Access(format!("{e}")))?;

        match content {
            ClipboardContent::Text(text) => {
                clipboard.set_text(text)
                    .map_err(|e| ClipboardError::Access(format!("{e}")))?;
            }
            ClipboardContent::Empty => {}
        }

        // Update last known content to avoid echo
        *self.last_content.lock().unwrap() = content.clone();
        Ok(())
    }

    /// Check if the clipboard has changed since last check.
    /// Returns Some(content) if changed, None if unchanged.
    pub fn poll_change(&self) -> Result<Option<ClipboardContent>, ClipboardError> {
        let current = self.read()?;
        let mut last = self.last_content.lock().unwrap();

        if current != *last {
            *last = current.clone();
            Ok(Some(current))
        } else {
            Ok(None)
        }
    }
}
