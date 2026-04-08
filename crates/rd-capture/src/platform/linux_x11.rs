use crate::frame::CapturedFrame;
use crate::{CaptureError, Capturer};
use x11rb::connection::Connection;
use x11rb::protocol::shm::{self, ConnectionExt as ShmConnectionExt};
use x11rb::protocol::xproto::{ConnectionExt, Screen};

pub struct X11Capturer {
    conn: x11rb::rust_connection::RustConnection,
    screen_num: usize,
    width: u32,
    height: u32,
    shm_seg: Option<shm::Seg>,
    shm_id: i32,
    shm_ptr: *mut u8,
    shm_size: usize,
}

// Safety: the SHM pointer is only accessed from the capture thread
unsafe impl Send for X11Capturer {}

impl X11Capturer {
    pub fn new() -> Result<Self, CaptureError> {
        let (conn, screen_num) = x11rb::connect(None)
            .map_err(|e| CaptureError::Init(format!("X11 connect: {e}")))?;

        let screen = &conn.setup().roots[screen_num];
        let width = screen.width_in_pixels as u32;
        let height = screen.height_in_pixels as u32;

        // Check for SHM extension
        let shm_available = conn
            .shm_query_version()
            .map_err(|e| CaptureError::Init(format!("SHM query: {e}")))?
            .reply()
            .is_ok();

        if !shm_available {
            tracing::warn!("X11 SHM extension not available, falling back to GetImage");
        }

        tracing::info!(width, height, shm_available, "initialized X11 screen capture");

        Ok(Self {
            conn,
            screen_num,
            width,
            height,
            shm_seg: None,
            shm_id: -1,
            shm_ptr: std::ptr::null_mut(),
            shm_size: 0,
        })
    }

    fn screen(&self) -> &Screen {
        &self.conn.setup().roots[self.screen_num]
    }

    /// Fallback capture using GetImage (slower, but always works)
    fn capture_get_image(&mut self) -> Result<CapturedFrame, CaptureError> {
        let root = self.screen().root;

        let reply = self
            .conn
            .get_image(
                x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
                root,
                0,
                0,
                self.width as u16,
                self.height as u16,
                !0,
            )
            .map_err(|e| CaptureError::Capture(format!("GetImage request: {e}")))?
            .reply()
            .map_err(|e| CaptureError::Capture(format!("GetImage reply: {e}")))?;

        let stride = self.width * 4;

        Ok(CapturedFrame::new(
            reply.data,
            self.width,
            self.height,
            stride,
        ))
    }
}

impl Capturer for X11Capturer {
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        // Use GetImage fallback for now. SHM can be added as an optimization later.
        self.capture_get_image()
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Drop for X11Capturer {
    fn drop(&mut self) {
        if !self.shm_ptr.is_null() {
            unsafe {
                libc::shmdt(self.shm_ptr as *const _);
                if self.shm_id >= 0 {
                    libc::shmctl(self.shm_id, libc::IPC_RMID, std::ptr::null_mut());
                }
            }
        }
    }
}
