use crate::yuv::i420_to_rgba;
use crate::{CodecError, DecodedFrame};
use std::ptr;

/// VP9 decoder using libvpx (vpx-sys)
pub struct Decoder {
    ctx: vpx_sys::vpx_codec_ctx_t,
}

impl Decoder {
    pub fn new() -> Result<Self, CodecError> {
        unsafe {
            let iface = vpx_sys::vpx_codec_vp9_dx();
            if iface.is_null() {
                return Err(CodecError::DecoderInit("VP9 decoder not available".into()));
            }

            let mut ctx: vpx_sys::vpx_codec_ctx_t = std::mem::zeroed();
            let cfg: vpx_sys::vpx_codec_dec_cfg_t = std::mem::zeroed();

            let result = vpx_sys::vpx_codec_dec_init_ver(
                &mut ctx,
                iface,
                &cfg,
                0,
                vpx_sys::VPX_DECODER_ABI_VERSION as i32,
            );

            if result != vpx_sys::VPX_CODEC_OK {
                return Err(CodecError::DecoderInit(format!(
                    "vpx_codec_dec_init failed: {:?}",
                    result
                )));
            }

            tracing::info!("VP9 decoder initialized");
            Ok(Self { ctx })
        }
    }

    /// Decode an encoded VP9 frame. Returns RGBA pixel data.
    pub fn decode(
        &mut self,
        data: &[u8],
        _width: u32,
        _height: u32,
    ) -> Result<DecodedFrame, CodecError> {
        unsafe {
            let result = vpx_sys::vpx_codec_decode(
                &mut self.ctx,
                data.as_ptr(),
                data.len() as u32,
                ptr::null_mut(),
                0,
            );

            if result != vpx_sys::VPX_CODEC_OK {
                return Err(CodecError::Decode(format!(
                    "vpx_codec_decode failed: {:?}",
                    result
                )));
            }

            // Get decoded frame
            let mut iter: vpx_sys::vpx_codec_iter_t = ptr::null();
            let img = vpx_sys::vpx_codec_get_frame(&mut self.ctx, &mut iter);

            if img.is_null() {
                return Err(CodecError::Decode("no frame decoded".into()));
            }

            let img = &*img;
            let w = img.d_w as u32;
            let h = img.d_h as u32;

            // Extract I420 planes
            let y_stride = img.stride[0] as usize;
            let u_stride = img.stride[1] as usize;
            let v_stride = img.stride[2] as usize;

            let y_ptr = img.planes[0];
            let u_ptr = img.planes[1];
            let v_ptr = img.planes[2];

            // Copy Y plane (handle stride)
            let mut y_plane = vec![0u8; (w * h) as usize];
            for row in 0..h as usize {
                let src = std::slice::from_raw_parts(y_ptr.add(row * y_stride), w as usize);
                y_plane[row * w as usize..(row + 1) * w as usize].copy_from_slice(src);
            }

            // Copy U plane
            let uw = (w / 2) as usize;
            let uh = (h / 2) as usize;
            let mut u_plane = vec![0u8; uw * uh];
            for row in 0..uh {
                let src = std::slice::from_raw_parts(u_ptr.add(row * u_stride), uw);
                u_plane[row * uw..(row + 1) * uw].copy_from_slice(src);
            }

            // Copy V plane
            let mut v_plane = vec![0u8; uw * uh];
            for row in 0..uh {
                let src = std::slice::from_raw_parts(v_ptr.add(row * v_stride), uw);
                v_plane[row * uw..(row + 1) * uw].copy_from_slice(src);
            }

            // Convert to RGBA
            let rgba = i420_to_rgba(&y_plane, &u_plane, &v_plane, w, h);

            Ok(DecodedFrame {
                data: rgba,
                width: w,
                height: h,
            })
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            vpx_sys::vpx_codec_destroy(&mut self.ctx);
        }
    }
}

// Safety: The decoder context is only accessed from one thread
unsafe impl Send for Decoder {}
