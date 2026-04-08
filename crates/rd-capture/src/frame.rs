/// A captured screen frame in BGRA format
#[derive(Clone)]
pub struct CapturedFrame {
    /// Raw BGRA pixel data
    pub data: Vec<u8>,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Bytes per row (may include padding)
    pub stride: u32,
}

impl CapturedFrame {
    pub fn new(data: Vec<u8>, width: u32, height: u32, stride: u32) -> Self {
        Self {
            data,
            width,
            height,
            stride,
        }
    }

    /// Convert BGRA to RGBA in-place for display
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let row_start = (y * self.stride) as usize;
            for x in 0..self.width {
                let offset = row_start + (x * 4) as usize;
                rgba.push(self.data[offset + 2]); // R (from B position)
                rgba.push(self.data[offset + 1]); // G
                rgba.push(self.data[offset]);     // B (from R position)
                rgba.push(self.data[offset + 3]); // A
            }
        }
        rgba
    }

    /// Convert BGRA to I420 (YUV 4:2:0) for video encoding
    pub fn to_i420(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut y_plane = vec![0u8; w * h];
        let mut u_plane = vec![0u8; (w / 2) * (h / 2)];
        let mut v_plane = vec![0u8; (w / 2) * (h / 2)];

        for row in 0..h {
            let src_row_start = row * self.stride as usize;
            for col in 0..w {
                let src_offset = src_row_start + col * 4;
                let b = self.data[src_offset] as f32;
                let g = self.data[src_offset + 1] as f32;
                let r = self.data[src_offset + 2] as f32;

                // BT.601 conversion
                let y = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 255.0);
                y_plane[row * w + col] = y as u8;

                // Subsample chroma for every 2x2 block
                if row % 2 == 0 && col % 2 == 0 {
                    let u = (-0.169 * r - 0.331 * g + 0.500 * b + 128.0).clamp(0.0, 255.0);
                    let v = (0.500 * r - 0.419 * g - 0.081 * b + 128.0).clamp(0.0, 255.0);
                    let chroma_idx = (row / 2) * (w / 2) + (col / 2);
                    u_plane[chroma_idx] = u as u8;
                    v_plane[chroma_idx] = v as u8;
                }
            }
        }

        (y_plane, u_plane, v_plane)
    }
}
