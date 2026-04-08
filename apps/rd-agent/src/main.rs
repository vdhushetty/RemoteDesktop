use anyhow::Result;
use rd_audio::AudioCapture;
use rd_clipboard::{ClipboardContent, ClipboardSync};
use rd_codec::{CodecConfig, Encoder};
use rd_input::{ButtonAction, InputInjector, MouseButton};
use rd_net::connection::{IrohReceiver, IrohSender, MessageReceiver, MessageSender, QuinnReceiver, QuinnSender};
use rd_net::identity::generate_pairing_code;
use rd_net::{DeviceIdentity, IrohTransport, LanDiscovery, QuicServer};
use rd_protocol::messages;
use rd_transfer::FileReceiver;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

const DEFAULT_PORT: u16 = 9876;
const TARGET_FPS: u32 = 30;
const CLIPBOARD_POLL_MS: u64 = 500;
const AUDIO_FRAME_MS: u64 = 20;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    tracing::info!("rd-agent starting...");

    let identity = DeviceIdentity::load_or_create(None)?;
    let iroh = Arc::new(IrohTransport::new(identity.clone()).await?);
    let pairing_code = generate_pairing_code();

    println!("\n========================================");
    println!("  Device ID:    {}", iroh.device_id());
    println!("  Name:         {}", identity.device_name());
    println!("  Pairing Code: {}", pairing_code);
    println!("========================================\n");

    let bind_addr: SocketAddr = format!("0.0.0.0:{DEFAULT_PORT}").parse()?;
    let server = QuicServer::bind(bind_addr).await?;
    tracing::info!(addr = %server.local_addr(), "LAN server listening");

    let discovery = LanDiscovery::new()?;
    discovery.register(identity.device_name(), server.local_addr().port(), std::env::consts::OS)?;

    tracing::info!("waiting for connections (LAN + Internet)...");

    let identity_clone = identity.clone();
    let pairing_clone = pairing_code.clone();

    // Iroh accept loop
    let iroh_clone = iroh.clone();
    let id2 = identity.clone();
    let pc2 = pairing_code.clone();
    tokio::spawn(async move {
        loop {
            match iroh_clone.accept().await {
                Ok(conn) => {
                    tracing::info!(remote = %conn.remote_id().fmt_short(), "new internet connection");
                    let id = id2.clone();
                    let pc = pc2.clone();
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all().build().unwrap();
                        rt.block_on(async {
                            if let Err(e) = handle_iroh_session(conn, id, pc).await {
                                tracing::error!(error = %e, "iroh session error");
                            }
                        });
                    });
                }
                Err(e) => { tracing::error!(error = %e, "iroh accept error"); break; }
            }
        }
    });

    // LAN accept loop
    loop {
        match server.accept().await {
            Ok(conn) => {
                let remote = conn.remote_address();
                tracing::info!(%remote, "new LAN connection");
                let id = identity_clone.clone();
                let pc = pairing_clone.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all().build().unwrap();
                    rt.block_on(async {
                        if let Err(e) = handle_lan_session(conn, id, pc).await {
                            tracing::error!(%remote, error = %e, "LAN session error");
                        }
                    });
                });
            }
            Err(e) => { tracing::error!(error = %e, "LAN accept error"); }
        }
    }
}

async fn handle_iroh_session(conn: iroh::endpoint::Connection, identity: DeviceIdentity, pairing_code: String) -> Result<()> {
    let (ctrl_s, ctrl_r) = conn.accept_bi().await?;
    let ctrl_send: IrohSender = MessageSender::new(ctrl_s);
    let ctrl_recv: IrohReceiver = MessageReceiver::new(ctrl_r);

    let video_s = conn.open_uni().await?;
    let video_send: IrohSender = MessageSender::new(video_s);

    // Audio uni stream (agent -> viewer)
    let audio_s = conn.open_uni().await?;
    let audio_send: IrohSender = MessageSender::new(audio_s);

    let input_streams = conn.accept_bi().await.ok();
    let input_recv = input_streams.map(|(_, r)| -> IrohReceiver { MessageReceiver::new(r) });

    run_session(ctrl_send, ctrl_recv, video_send, audio_send, input_recv, &identity, &pairing_code, "internet").await
}

async fn handle_lan_session(conn: quinn::Connection, identity: DeviceIdentity, pairing_code: String) -> Result<()> {
    let (ctrl_s, ctrl_r) = conn.accept_bi().await?;
    let ctrl_send: QuinnSender = MessageSender::new(ctrl_s);
    let ctrl_recv: QuinnReceiver = MessageReceiver::new(ctrl_r);

    let video_s = conn.open_uni().await?;
    let video_send: QuinnSender = MessageSender::new(video_s);

    let audio_s = conn.open_uni().await?;
    let audio_send: QuinnSender = MessageSender::new(audio_s);

    let input_streams = conn.accept_bi().await.ok();
    let input_recv = input_streams.map(|(_, r)| -> QuinnReceiver { MessageReceiver::new(r) });

    run_session(ctrl_send, ctrl_recv, video_send, audio_send, input_recv, &identity, &pairing_code, "LAN").await
}

async fn run_session<W, R>(
    mut ctrl_send: MessageSender<W>,
    mut ctrl_recv: MessageReceiver<R>,
    mut video_send: MessageSender<W>,
    mut audio_send: MessageSender<W>,
    input_recv: Option<MessageReceiver<R>>,
    _identity: &DeviceIdentity,
    _pairing_code: &str,
    transport: &str,
) -> Result<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    // Session handshake
    let init_msg = ctrl_recv.recv().await
        .map_err(|e| anyhow::anyhow!("recv init: {e}"))?;
    if let Some(messages::message::Payload::SessionInit(init)) = &init_msg.payload {
        tracing::info!(device = %init.device_name, os = %init.os, transport, "session init");
    }

    let mut capturer = rd_capture::create_capturer()?;
    let first_frame = capturer.capture_frame()?;
    let width = first_frame.width & !1;
    let height = first_frame.height & !1;
    tracing::info!(width, height, "screen capture initialized");

    ctrl_send.send(&messages::Message {
        sequence: 1,
        timestamp_us: rd_protocol::now_us(),
        payload: Some(messages::message::Payload::SessionAccept(
            messages::SessionAccept {
                device_name: hostname::get()
                    .map(|h| h.to_string_lossy().to_string())
                    .unwrap_or_default(),
                os: std::env::consts::OS.to_string(),
                selected_codec: messages::Codec::Vp9.into(),
            },
        )),
    }).await.map_err(|e| anyhow::anyhow!("send accept: {e}"))?;

    let mut encoder = Encoder::new(CodecConfig {
        width, height, fps: TARGET_FPS, bitrate_kbps: 4000,
    })?;

    // Start input handler
    if let Some(mut recv) = input_recv {
        let sw = width;
        let sh = height;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap();
            rt.block_on(handle_input(&mut recv, sw, sh))
        });
    }

    // Clipboard sync (agent -> viewer)
    let clipboard_pending: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    {
        let pending = clipboard_pending.clone();
        std::thread::spawn(move || {
            if let Ok(cb) = ClipboardSync::new() {
                loop {
                    std::thread::sleep(Duration::from_millis(CLIPBOARD_POLL_MS));
                    if let Ok(Some(ClipboardContent::Text(text))) = cb.poll_change() {
                        *pending.lock().unwrap() = Some(text);
                    }
                }
            }
        });
    }

    // Audio capture + send (agent -> viewer)
    let audio_pending: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let pending = audio_pending.clone();
        std::thread::spawn(move || {
            match AudioCapture::new() {
                Ok(mut capture) => {
                    tracing::info!("audio capture started");
                    loop {
                        std::thread::sleep(Duration::from_millis(AUDIO_FRAME_MS));
                        match capture.encode_frame() {
                            Ok(Some(data)) => {
                                pending.lock().unwrap().push(data);
                            }
                            Ok(None) => {} // not enough samples yet
                            Err(e) => {
                                tracing::debug!(error = %e, "audio encode error");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "audio capture not available");
                }
            }
        });
    }

    // Main loop: capture + encode + send video, clipboard, audio
    let frame_interval = Duration::from_millis(1000 / TARGET_FPS as u64);
    let mut sequence: u64 = 0;
    let mut audio_seq: u64 = 50000;
    tracing::info!(fps = TARGET_FPS, transport, "streaming started");

    loop {
        tokio::time::sleep(frame_interval).await;

        // Send clipboard changes
        if let Some(text) = clipboard_pending.lock().unwrap().take() {
            sequence += 1;
            let msg = messages::Message {
                sequence,
                timestamp_us: rd_protocol::now_us(),
                payload: Some(messages::message::Payload::ClipboardUpdate(
                    messages::ClipboardUpdate { text },
                )),
            };
            let _ = ctrl_send.send(&msg).await;
        }

        // Send audio frames
        let audio_frames: Vec<Vec<u8>> = std::mem::take(&mut *audio_pending.lock().unwrap());
        for opus_data in audio_frames {
            audio_seq += 1;
            let msg = messages::Message {
                sequence: audio_seq,
                timestamp_us: rd_protocol::now_us(),
                payload: Some(messages::message::Payload::AudioFrame(
                    messages::AudioFrame {
                        opus_data,
                        sample_rate: rd_audio::SAMPLE_RATE,
                        channels: rd_audio::CHANNELS as u32,
                    },
                )),
            };
            if let Err(e) = audio_send.send(&msg).await {
                tracing::debug!(error = %e, "audio send failed");
                break;
            }
        }

        // Capture + encode + send video
        let frame = match capturer.capture_frame() {
            Ok(f) => f,
            Err(e) => { tracing::warn!(error = %e, "capture"); continue; }
        };

        let (y, u, v) = frame.to_i420();
        let encoded = match encoder.encode(&y, &u, &v) {
            Ok(f) => f,
            Err(e) => { tracing::warn!(error = %e, "encode"); continue; }
        };

        for ef in &encoded {
            sequence += 1;
            let msg = rd_protocol::video_frame_message(
                sequence, sequence, messages::Codec::Vp9,
                ef.is_keyframe, ef.width, ef.height, ef.data.clone(),
            );
            if let Err(e) = video_send.send(&msg).await {
                tracing::error!(error = %e, "send frame");
                return Ok(());
            }
        }
    }
}

async fn handle_input<R: AsyncRead + Unpin + Send>(
    recv: &mut MessageReceiver<R>, sw: u32, sh: u32,
) -> Result<()> {
    let mut inj = rd_input::create_injector()
        .map_err(|e| anyhow::anyhow!("create injector: {e}"))?;
    let clipboard = ClipboardSync::new().ok();
    let download_dir = rd_transfer::download_dir();

    loop {
        let msg = match recv.recv().await {
            Ok(m) => m,
            Err(_) => break,
        };

        match msg.payload {
            Some(messages::message::Payload::InputEvent(input)) => {
                let _ = process_input(&mut *inj, &input, sw, sh);
            }
            Some(messages::message::Payload::ClipboardUpdate(cb)) => {
                if let Some(ref clipboard) = clipboard {
                    let _ = clipboard.write(&ClipboardContent::Text(cb.text));
                }
            }
            Some(messages::message::Payload::FileChunk(chunk)) => {
                // Simple file receive: append chunks to file
                let path = download_dir.join(format!("transfer_{}", chunk.transfer_id));
                if let Ok(mut file) = tokio::fs::OpenOptions::new()
                    .create(true).append(true).open(&path).await
                {
                    use tokio::io::AsyncWriteExt;
                    let _ = file.write_all(&chunk.data).await;
                    if chunk.is_last {
                        tracing::info!(path = %path.display(), "file transfer complete");
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn process_input(
    inj: &mut dyn InputInjector, ev: &messages::InputEvent, sw: u32, sh: u32,
) -> Result<(), rd_input::InputError> {
    match &ev.event {
        Some(messages::input_event::Event::MouseMove(m)) => {
            inj.mouse_move((m.x * sw as f64) as i32, (m.y * sh as f64) as i32)?;
        }
        Some(messages::input_event::Event::MouseButton(b)) => {
            let btn = match messages::MouseButtonType::try_from(b.button) {
                Ok(messages::MouseButtonType::MouseButtonLeft) => MouseButton::Left,
                Ok(messages::MouseButtonType::MouseButtonRight) => MouseButton::Right,
                Ok(messages::MouseButtonType::MouseButtonMiddle) => MouseButton::Middle,
                _ => return Ok(()),
            };
            let act = match messages::ButtonAction::try_from(b.action) {
                Ok(messages::ButtonAction::Press) => ButtonAction::Press,
                Ok(messages::ButtonAction::Release) => ButtonAction::Release,
                _ => return Ok(()),
            };
            inj.mouse_move((b.x * sw as f64) as i32, (b.y * sh as f64) as i32)?;
            inj.mouse_button(btn, act)?;
        }
        Some(messages::input_event::Event::MouseScroll(s)) => {
            inj.mouse_scroll(s.delta_x, s.delta_y)?;
        }
        Some(messages::input_event::Event::KeyEvent(k)) => {
            let act = match messages::ButtonAction::try_from(k.action) {
                Ok(messages::ButtonAction::Press) => ButtonAction::Press,
                Ok(messages::ButtonAction::Release) => ButtonAction::Release,
                _ => return Ok(()),
            };
            inj.key_event(k.keycode, act)?;
        }
        None => {}
    }
    Ok(())
}
