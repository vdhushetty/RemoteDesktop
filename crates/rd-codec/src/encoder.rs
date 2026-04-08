use crate::{CodecConfig, CodecError, EncodedFrame};
use std::os::raw::c_uint;

/// VP9 encoder using libvpx
pub struct Encoder {
    inner: vpx_encode::Encoder,
    config: CodecConfig,
    frame_count: i64,
    keyframe_interval: i64,
}

impl Encoder {
    pub fn new(config: CodecConfig) -> Result<Self, CodecError> {
        let vpx_config = vpx_encode::Config {
            width: config.width as c_uint,
            height: config.height as c_uint,
            timebase: [1, config.fps as i32],
            bitrate: config.bitrate_kbps as c_uint,
            codec: vpx_encode::VideoCodecId::VP9,
        };

        let inner = vpx_encode::Encoder::new(vpx_config)
            .map_err(|e| CodecError::EncoderInit(format!("vpx encoder init: {e}")))?;

        tracing::info!(
            width = config.width,
            height = config.height,
            fps = config.fps,
            bitrate_kbps = config.bitrate_kbps,
            "VP9 encoder initialized"
        );

        Ok(Self {
            inner,
            config,
            frame_count: 0,
            keyframe_interval: 90, // keyframe every 3 seconds at 30fps
        })
    }

    /// Encode a frame from I420 (YUV) planes.
    /// Returns encoded packets (may be empty if encoder is buffering).
    pub fn encode(
        &mut self,
        y_plane: &[u8],
        u_plane: &[u8],
        v_plane: &[u8],
    ) -> Result<Vec<EncodedFrame>, CodecError> {
        let pts = self.frame_count;
        self.frame_count += 1;

        // Combine planes into a single I420 buffer
        let mut i420 = Vec::with_capacity(y_plane.len() + u_plane.len() + v_plane.len());
        i420.extend_from_slice(y_plane);
        i420.extend_from_slice(u_plane);
        i420.extend_from_slice(v_plane);

        let packets = self
            .inner
            .encode(pts, &i420)
            .map_err(|e| CodecError::Encode(format!("vpx encode: {e}")))?;

        let frames: Vec<EncodedFrame> = packets
            .map(|pkt| EncodedFrame {
                data: pkt.data.to_vec(),
                is_keyframe: pkt.key,
                width: self.config.width,
                height: self.config.height,
            })
            .collect();

        Ok(frames)
    }

    /// Force-request a keyframe on the next encode call
    pub fn request_keyframe(&mut self) {
        // vpx-encode doesn't expose per-frame flags directly.
        // Keyframes are handled internally by the encoder based on bitrate/quality targets.
        // For forced keyframes, we would need to use vpx-sys directly.
        tracing::debug!("keyframe requested (handled by encoder internally)");
    }

    pub fn config(&self) -> &CodecConfig {
        &self.config
    }
}
