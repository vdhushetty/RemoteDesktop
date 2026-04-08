use crate::NetError;
use bytes::Bytes;
use prost::Message as ProstMessage;
use rd_protocol::messages;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Type aliases for quinn-based streams (LAN connections)
pub type QuinnSender = MessageSender<quinn::SendStream>;
pub type QuinnReceiver = MessageReceiver<quinn::RecvStream>;

/// Type aliases for iroh/noq-based streams (internet connections)
pub type IrohSender = MessageSender<iroh::endpoint::SendStream>;
pub type IrohReceiver = MessageReceiver<iroh::endpoint::RecvStream>;

/// Identifies whether this side is the agent (server) or viewer (client)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRole {
    Agent,
    Viewer,
}

/// Wraps a QUIC connection with protocol-level send/receive.
/// Works with both quinn and iroh (noq) connections.
pub struct PeerConnection {
    connection: quinn::Connection,
    role: ConnectionRole,
}

impl PeerConnection {
    pub fn new(connection: quinn::Connection, role: ConnectionRole) -> Self {
        Self { connection, role }
    }

    pub async fn open_control_stream(
        &self,
    ) -> Result<(MessageSender<quinn::SendStream>, MessageReceiver<quinn::RecvStream>), NetError> {
        let (send, recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|e| NetError::Connection(format!("open_bi: {e}")))?;
        Ok((MessageSender::new(send), MessageReceiver::new(recv)))
    }

    pub async fn accept_stream(
        &self,
    ) -> Result<(MessageSender<quinn::SendStream>, MessageReceiver<quinn::RecvStream>), NetError> {
        let (send, recv) = self
            .connection
            .accept_bi()
            .await
            .map_err(|e| NetError::Connection(format!("accept_bi: {e}")))?;
        Ok((MessageSender::new(send), MessageReceiver::new(recv)))
    }

    pub async fn open_video_stream(&self) -> Result<MessageSender<quinn::SendStream>, NetError> {
        let send = self
            .connection
            .open_uni()
            .await
            .map_err(|e| NetError::Connection(format!("open_uni: {e}")))?;
        Ok(MessageSender::new(send))
    }

    pub async fn accept_video_stream(&self) -> Result<MessageReceiver<quinn::RecvStream>, NetError> {
        let recv = self
            .connection
            .accept_uni()
            .await
            .map_err(|e| NetError::Connection(format!("accept_uni: {e}")))?;
        Ok(MessageReceiver::new(recv))
    }

    pub fn remote_address(&self) -> std::net::SocketAddr {
        self.connection.remote_address()
    }

    pub fn role(&self) -> ConnectionRole {
        self.role
    }
}

/// Sends length-prefixed protobuf messages over any async write stream.
/// Works with quinn::SendStream, noq::SendStream, etc.
pub struct MessageSender<W: AsyncWrite + Unpin + Send> {
    stream: W,
}

impl<W: AsyncWrite + Unpin + Send> MessageSender<W> {
    pub fn new(stream: W) -> Self {
        Self { stream }
    }

    pub async fn send(&mut self, msg: &messages::Message) -> Result<(), NetError> {
        let encoded = rd_protocol::encode_message(msg)
            .map_err(|e| NetError::Transport(format!("encode: {e}")))?;

        self.stream
            .write_all(&encoded)
            .await
            .map_err(|e| NetError::Transport(format!("write: {e}")))?;

        Ok(())
    }

    pub async fn send_raw(&mut self, data: &[u8]) -> Result<(), NetError> {
        let len = (data.len() as u32).to_be_bytes();
        self.stream
            .write_all(&len)
            .await
            .map_err(|e| NetError::Transport(format!("write len: {e}")))?;
        self.stream
            .write_all(data)
            .await
            .map_err(|e| NetError::Transport(format!("write data: {e}")))?;
        Ok(())
    }
}

/// Receives length-prefixed protobuf messages from any async read stream.
/// Works with quinn::RecvStream, noq::RecvStream, etc.
pub struct MessageReceiver<R: AsyncRead + Unpin + Send> {
    stream: R,
}

impl<R: AsyncRead + Unpin + Send> MessageReceiver<R> {
    pub fn new(stream: R) -> Self {
        Self { stream }
    }

    pub async fn recv(&mut self) -> Result<messages::Message, NetError> {
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| NetError::Transport(format!("read len: {e}")))?;

        let len = u32::from_be_bytes(len_buf) as usize;
        if len > rd_protocol::MAX_MESSAGE_SIZE {
            return Err(NetError::Transport(format!(
                "message too large: {len} bytes"
            )));
        }

        let mut payload = vec![0u8; len];
        self.stream
            .read_exact(&mut payload)
            .await
            .map_err(|e| NetError::Transport(format!("read payload: {e}")))?;

        let msg = <messages::Message as ProstMessage>::decode(Bytes::from(payload))
            .map_err(|e| NetError::Transport(format!("decode: {e}")))?;

        Ok(msg)
    }

    pub async fn recv_raw(&mut self) -> Result<Vec<u8>, NetError> {
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| NetError::Transport(format!("read len: {e}")))?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut data = vec![0u8; len];
        self.stream
            .read_exact(&mut data)
            .await
            .map_err(|e| NetError::Transport(format!("read data: {e}")))?;

        Ok(data)
    }
}
