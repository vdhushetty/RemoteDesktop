use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("transfer rejected: {0}")]
    Rejected(String),
    #[error("transfer cancelled")]
    Cancelled,
}

/// Chunk size for file transfer: 64KB
pub const CHUNK_SIZE: usize = 64 * 1024;

/// Reads a file and yields chunks for sending
pub struct FileSender {
    transfer_id: u64,
    file: tokio::fs::File,
    file_size: u64,
    offset: u64,
}

impl FileSender {
    pub async fn new(transfer_id: u64, path: &Path) -> Result<Self, TransferError> {
        let metadata = fs::metadata(path).await?;
        let file = fs::File::open(path).await?;
        Ok(Self {
            transfer_id,
            file,
            file_size: metadata.len(),
            offset: 0,
        })
    }

    pub fn transfer_id(&self) -> u64 {
        self.transfer_id
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn progress(&self) -> f64 {
        if self.file_size == 0 { return 1.0; }
        self.offset as f64 / self.file_size as f64
    }

    /// Read the next chunk. Returns None when complete.
    pub async fn next_chunk(&mut self) -> Result<Option<(u64, Vec<u8>, bool)>, TransferError> {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let n = self.file.read(&mut buf).await?;

        if n == 0 {
            return Ok(None);
        }

        buf.truncate(n);
        let offset = self.offset;
        self.offset += n as u64;
        let is_last = self.offset >= self.file_size;

        Ok(Some((offset, buf, is_last)))
    }
}

/// Receives chunks and writes them to a file
pub struct FileReceiver {
    transfer_id: u64,
    file: tokio::fs::File,
    path: PathBuf,
    expected_size: u64,
    received: u64,
}

impl FileReceiver {
    pub async fn new(
        transfer_id: u64,
        path: &Path,
        expected_size: u64,
    ) -> Result<Self, TransferError> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let file = fs::File::create(path).await?;
        Ok(Self {
            transfer_id,
            file,
            path: path.to_path_buf(),
            expected_size,
            received: 0,
        })
    }

    pub fn transfer_id(&self) -> u64 {
        self.transfer_id
    }

    pub fn progress(&self) -> f64 {
        if self.expected_size == 0 { return 1.0; }
        self.received as f64 / self.expected_size as f64
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write a chunk to the file
    pub async fn write_chunk(&mut self, data: &[u8], is_last: bool) -> Result<(), TransferError> {
        self.file.write_all(data).await?;
        self.received += data.len() as u64;

        if is_last {
            self.file.flush().await?;
            tracing::info!(
                path = %self.path.display(),
                size = self.received,
                "file transfer complete"
            );
        }

        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.received >= self.expected_size
    }
}

/// Get the default download directory
pub fn download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}
