/// Convert I420 (YUV 4:2:0) to RGBA for display
pub fn i420_to_rgba(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut rgba = vec![0u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let y_idx = row * w + col;
            let uv_idx = (row / 2) * (w / 2) + (col / 2);

            let y = y_plane[y_idx] as f32;
            let u = u_plane[uv_idx] as f32 - 128.0;
            let v = v_plane[uv_idx] as f32 - 128.0;

            // BT.601 YUV to RGB
            let r = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
            let g = (y - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * u).clamp(0.0, 255.0) as u8;

            let rgba_idx = (row * w + col) * 4;
            rgba[rgba_idx] = r;
            rgba[rgba_idx + 1] = g;
            rgba[rgba_idx + 2] = b;
            rgba[rgba_idx + 3] = 255;
        }
    }

    rgba
}

/// Convert RGBA to I420 (YUV 4:2:0) for encoding
pub fn rgba_to_i420(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    let mut y_plane = vec![0u8; w * h];
    let mut u_plane = vec![0u8; (w / 2) * (h / 2)];
    let mut v_plane = vec![0u8; (w / 2) * (h / 2)];

    for row in 0..h {
        for col in 0..w {
            let rgba_idx = (row * w + col) * 4;
            let r = rgba[rgba_idx] as f32;
            let g = rgba[rgba_idx + 1] as f32;
            let b = rgba[rgba_idx + 2] as f32;

            let y = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 255.0);
            y_plane[row * w + col] = y as u8;

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
