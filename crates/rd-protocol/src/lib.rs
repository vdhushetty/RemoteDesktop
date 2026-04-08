pub mod messages {
    include!(concat!(env!("OUT_DIR"), "/rd.protocol.rs"));
}

use bytes::{Bytes, BytesMut};
use prost::Message;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("failed to encode message: {0}")]
    Encode(#[from] prost::EncodeError),
    #[error("failed to decode message: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("message too large: {size} bytes (max {max})")]
    MessageTooLarge { size: usize, max: usize },
}

/// Maximum message size: 16 MB (enough for a full keyframe)
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Encode a protocol message into a length-prefixed byte buffer.
/// Format: [4 bytes big-endian length][protobuf payload]
pub fn encode_message(msg: &messages::Message) -> Result<Bytes, ProtocolError> {
    let size = msg.encoded_len();
    if size > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge {
            size,
            max: MAX_MESSAGE_SIZE,
        });
    }

    let mut buf = BytesMut::with_capacity(4 + size);
    buf.extend_from_slice(&(size as u32).to_be_bytes());
    msg.encode(&mut buf)?;
    Ok(buf.freeze())
}

/// Decode a protocol message from a length-prefixed byte buffer.
pub fn decode_message(data: &[u8]) -> Result<messages::Message, ProtocolError> {
    if data.len() < 4 {
        return Err(ProtocolError::Decode(prost::DecodeError::new(
            "buffer too short for length prefix",
        )));
    }

    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge {
            size: len,
            max: MAX_MESSAGE_SIZE,
        });
    }

    let payload = &data[4..4 + len];
    let msg = messages::Message::decode(payload)?;
    Ok(msg)
}

/// Helper to create a timestamp in microseconds
pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Helper to construct a video frame message
pub fn video_frame_message(
    sequence: u64,
    frame_id: u64,
    codec: messages::Codec,
    is_keyframe: bool,
    width: u32,
    height: u32,
    data: Vec<u8>,
) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::VideoFrame(
            messages::VideoFrame {
                frame_id,
                codec: codec.into(),
                is_keyframe,
                width,
                height,
                data,
                dirty_rects: vec![],
            },
        )),
    }
}

/// Helper to construct a mouse move input message
pub fn mouse_move_message(sequence: u64, x: f64, y: f64) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::InputEvent(
            messages::InputEvent {
                event: Some(messages::input_event::Event::MouseMove(
                    messages::MouseMove { x, y },
                )),
            },
        )),
    }
}

/// Helper to construct a mouse button input message
pub fn mouse_button_message(
    sequence: u64,
    button: messages::MouseButtonType,
    action: messages::ButtonAction,
    x: f64,
    y: f64,
) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::InputEvent(
            messages::InputEvent {
                event: Some(messages::input_event::Event::MouseButton(
                    messages::MouseButton {
                        button: button.into(),
                        action: action.into(),
                        x,
                        y,
                    },
                )),
            },
        )),
    }
}

/// Helper to construct a mouse scroll input message
pub fn mouse_scroll_message(
    sequence: u64,
    delta_x: f64,
    delta_y: f64,
    x: f64,
    y: f64,
) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::InputEvent(
            messages::InputEvent {
                event: Some(messages::input_event::Event::MouseScroll(
                    messages::MouseScroll {
                        delta_x,
                        delta_y,
                        x,
                        y,
                    },
                )),
            },
        )),
    }
}

/// Helper to construct a key event input message
pub fn key_event_message(
    sequence: u64,
    keycode: u32,
    action: messages::ButtonAction,
    modifiers: u32,
) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::InputEvent(
            messages::InputEvent {
                event: Some(messages::input_event::Event::KeyEvent(
                    messages::KeyEvent {
                        keycode,
                        action: action.into(),
                        modifiers,
                    },
                )),
            },
        )),
    }
}

/// Helper to construct a cursor info message
pub fn cursor_info_message(
    sequence: u64,
    x: f64,
    y: f64,
    visible: bool,
    shape: messages::CursorShape,
) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::CursorInfo(
            messages::CursorInfo {
                x,
                y,
                visible,
                shape: shape.into(),
            },
        )),
    }
}

/// Helper to construct a heartbeat message
pub fn heartbeat_message(sequence: u64) -> messages::Message {
    messages::Message {
        sequence,
        timestamp_us: now_us(),
        payload: Some(messages::message::Payload::Heartbeat(
            messages::Heartbeat {
                timestamp_us: now_us(),
            },
        )),
    }
}
