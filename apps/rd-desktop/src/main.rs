use anyhow::Result;
use eframe::egui;
use rd_audio::AudioPlayback;
use rd_clipboard::{ClipboardContent, ClipboardSync};
use rd_codec::{CodecConfig, DecodedFrame, Decoder, Encoder};
use rd_input::{ButtonAction, InputInjector, MouseButton};
use rd_net::connection::{MessageReceiver, MessageSender};
use rd_net::identity::generate_pairing_code;
use rd_net::{DeviceIdentity, IrohTransport, LanDiscovery, QuicServer};
use rd_protocol::messages;
use rd_transfer::FileSender;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

const DEFAULT_PORT: u16 = 9876;
const TARGET_FPS: u32 = 30;
const HEARTBEAT_INTERVAL_MS: u64 = 2000;
const CLIPBOARD_POLL_MS: u64 = 500;
const AUDIO_FRAME_MS: u64 = 20;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Remote Desktop")
            .with_inner_size([600.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Remote Desktop",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}

// ── Shared state ──

#[derive(Debug, Clone)]
enum QueuedInput {
    MouseMove { x: f64, y: f64 },
    MouseButton { button: messages::MouseButtonType, action: messages::ButtonAction, x: f64, y: f64 },
    MouseScroll { delta_x: f64, delta_y: f64, x: f64, y: f64 },
    KeyPress { keycode: u32, modifiers: u32 },
    KeyRelease { keycode: u32, modifiers: u32 },
}

struct SharedState {
    // Agent state
    device_id: String,
    connection_ticket: String,
    device_name: String,
    agent_ready: bool,
    incoming_session: Option<String>,

    // Shared iroh endpoint (used by both agent and viewer)
    iroh: Option<Arc<IrohTransport>>,

    // Viewer/session state
    frame: Option<DecodedFrame>,
    status: ConnectionStatus,
    discovered_devices: Vec<rd_net::discovery::DiscoveredDevice>,
    input_queue: Vec<QueuedInput>,
    rtt_us: u64,
    remote_screen: (u32, u32),
    file_to_send: Option<std::path::PathBuf>,
    transfer_progress: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected { remote_name: String },
    Error(String),
}

// ── App ──

struct App {
    state: Arc<Mutex<SharedState>>,
    texture: Option<egui::TextureHandle>,
    connect_target: String, // Device ID or IP
    runtime: tokio::runtime::Runtime,
}

impl App {
    fn new(_cc: &eframe::CreationContext) -> Self {
        let state = Arc::new(Mutex::new(SharedState {
            device_id: "loading...".into(),
            connection_ticket: String::new(),
            device_name: String::new(),
            agent_ready: false,
            incoming_session: None,
            iroh: None,
            frame: None,
            status: ConnectionStatus::Disconnected,
            discovered_devices: vec![],
            input_queue: vec![],
            rtt_us: 0,
            remote_screen: (1920, 1080),
            file_to_send: None,
            transfer_progress: None,
        }));

        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        // Start agent services in the background
        let state_bg = state.clone();
        runtime.spawn(async move {
            if let Err(e) = start_agent_services(state_bg).await {
                tracing::error!(error = %e, "agent services failed");
            }
        });

        Self {
            state,
            texture: None,
            connect_target: String::new(),
            runtime,
        }
    }

    fn connect(&mut self, target: String, ctx: egui::Context) {
        let state = self.state.clone();
        state.lock().unwrap().status = ConnectionStatus::Connecting;

        self.runtime.spawn(async move {
            let result: Result<()> = async {
                // Auto-detect: IP address vs connection ticket
                if target.contains(':') && target.parse::<SocketAddr>().is_ok() {
                    // IP:port — direct LAN
                    let addr: SocketAddr = target.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
                    connect_lan(addr, state.clone(), ctx.clone()).await
                } else if target.parse::<std::net::IpAddr>().is_ok() {
                    // IP without port — LAN with default port
                    let addr = SocketAddr::new(target.parse().map_err(|e| anyhow::anyhow!("{e}"))?, DEFAULT_PORT);
                    connect_lan(addr, state.clone(), ctx.clone()).await
                } else {
                    // Connection ticket (base64) or device ID (hex)
                    connect_internet(target, state.clone(), ctx.clone()).await
                }
            }.await;

            if let Err(e) = result {
                tracing::error!(error = %e, "connection error");
                state.lock().unwrap().status = ConnectionStatus::Error(e.to_string());
                ctx.request_repaint();
            }
        });
    }

    fn handle_input(&self, ui: &egui::Ui, image_rect: egui::Rect) {
        let response = ui.interact(image_rect, ui.id().with("input"), egui::Sense::click_and_drag());
        let normalize = |pos: egui::Pos2| -> (f64, f64) {
            (((pos.x - image_rect.left()) / image_rect.width()).clamp(0.0, 1.0) as f64,
             ((pos.y - image_rect.top()) / image_rect.height()).clamp(0.0, 1.0) as f64)
        };
        let mut inputs = Vec::new();

        if let Some(pos) = ui.ctx().pointer_hover_pos() {
            if image_rect.contains(pos) {
                let (x, y) = normalize(pos);
                inputs.push(QueuedInput::MouseMove { x, y });
            }
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = normalize(pos);
                inputs.push(QueuedInput::MouseButton { button: messages::MouseButtonType::MouseButtonLeft, action: messages::ButtonAction::Press, x, y });
                inputs.push(QueuedInput::MouseButton { button: messages::MouseButtonType::MouseButtonLeft, action: messages::ButtonAction::Release, x, y });
            }
        }
        if response.secondary_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let (x, y) = normalize(pos);
                inputs.push(QueuedInput::MouseButton { button: messages::MouseButtonType::MouseButtonRight, action: messages::ButtonAction::Press, x, y });
                inputs.push(QueuedInput::MouseButton { button: messages::MouseButtonType::MouseButtonRight, action: messages::ButtonAction::Release, x, y });
            }
        }
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if scroll.y.abs() > 0.1 || scroll.x.abs() > 0.1 {
            if let Some(pos) = ui.ctx().pointer_hover_pos() {
                if image_rect.contains(pos) {
                    let (x, y) = normalize(pos);
                    inputs.push(QueuedInput::MouseScroll { delta_x: scroll.x as f64, delta_y: scroll.y as f64, x, y });
                }
            }
        }
        ui.input(|is| {
            for ev in &is.events {
                if let egui::Event::Key { key, pressed, modifiers, .. } = ev {
                    let kc = egui_key_to_keycode(*key);
                    if kc > 0 {
                        let m = egui_modifiers_to_flags(modifiers);
                        inputs.push(if *pressed { QueuedInput::KeyPress { keycode: kc, modifiers: m } } else { QueuedInput::KeyRelease { keycode: kc, modifiers: m } });
                    }
                }
            }
        });
        if !inputs.is_empty() { self.state.lock().unwrap().input_queue.extend(inputs); }
    }
}

// ── UI ──

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (status, connection_ticket, device_name, agent_ready, devices, rtt, transfer_progress) = {
            let s = self.state.lock().unwrap();
            (s.status.clone(), s.connection_ticket.clone(), s.device_name.clone(),
             s.agent_ready, s.discovered_devices.clone(), s.rtt_us, s.transfer_progress)
        };

        match status {
            ConnectionStatus::Disconnected | ConnectionStatus::Error(_) | ConnectionStatus::Connecting => {
                // ── Home screen ──
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(30.0);
                        ui.heading("Remote Desktop");
                        ui.add_space(20.0);

                        // Your connection ticket section
                        ui.group(|ui| {
                            ui.label("Your Connection Code (share this to let others connect):");
                            ui.add_space(5.0);
                            if agent_ready {
                                // Show a truncated ticket with copy button
                                ui.horizontal(|ui| {
                                    let short = if connection_ticket.len() > 40 {
                                        format!("{}...", &connection_ticket[..40])
                                    } else {
                                        connection_ticket.clone()
                                    };
                                    ui.monospace(&short);
                                    if ui.button("Copy").clicked() {
                                        ui.output_mut(|o| o.copied_text = connection_ticket.clone());
                                    }
                                });
                            } else {
                                ui.label("Starting...");
                                ui.spinner();
                            }
                            if !device_name.is_empty() {
                                ui.label(format!("Device: {device_name}"));
                            }
                        });

                        ui.add_space(15.0);

                        // Error display
                        if let ConnectionStatus::Error(ref e) = status {
                            ui.colored_label(egui::Color32::RED, format!("Error: {e}"));
                            ui.add_space(5.0);
                        }

                        // Connecting spinner
                        if status == ConnectionStatus::Connecting {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Connecting...");
                            });
                            ui.add_space(5.0);
                        }

                        // Connect to remote section
                        ui.group(|ui| {
                            ui.label("Connect to remote machine:");
                            ui.add_space(3.0);
                            // Use multiline text edit so long codes wrap instead of pushing button off screen
                            let te = ui.add(
                                egui::TextEdit::multiline(&mut self.connect_target)
                                    .desired_rows(2)
                                    .desired_width(ui.available_width())
                                    .hint_text("Paste a Connection Code or IP address")
                            );
                            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.connect_target.is_empty() {
                                let target = self.connect_target.trim().to_string();
                                self.connect_target = target.clone();
                                self.connect(target, ctx.clone());
                            }
                            ui.add_space(3.0);
                            if ui.add_sized([ui.available_width(), 30.0], egui::Button::new("Connect")).clicked() && !self.connect_target.is_empty() {
                                let target = self.connect_target.trim().to_string();
                                self.connect(target, ctx.clone());
                            }
                        });

                        ui.add_space(15.0);

                        // LAN devices
                        if !devices.is_empty() {
                            ui.group(|ui| {
                                ui.label("Devices on your network:");
                                for dev in &devices {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{} ({})", dev.name, dev.os));
                                        if ui.button("Connect").clicked() {
                                            self.connect(dev.addr.to_string(), ctx.clone());
                                        }
                                    });
                                }
                            });
                        }
                    });
                });

                // Keep repainting while agent is starting
                if !agent_ready {
                    ctx.request_repaint_after(Duration::from_millis(500));
                }
            }

            ConnectionStatus::Connected { ref remote_name } => {
                // ── Remote session screen ──
                // Resize window for remote view
                egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Connected to: {remote_name}"));
                        if rtt > 0 {
                            ui.separator();
                            ui.label(format!("RTT: {:.1}ms", rtt as f64 / 1000.0));
                        }
                        ui.separator();
                        if let Some(progress) = transfer_progress {
                            ui.label(format!("Sending: {:.0}%", progress * 100.0));
                        } else if ui.button("Send File").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                self.state.lock().unwrap().file_to_send = Some(path);
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Disconnect").clicked() {
                                let mut s = self.state.lock().unwrap();
                                s.status = ConnectionStatus::Disconnected;
                                s.frame = None;
                            }
                        });
                    });
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    let s = self.state.lock().unwrap();
                    if let Some(ref frame) = s.frame {
                        let sz = [frame.width as usize, frame.height as usize];
                        let img = egui::ColorImage::from_rgba_unmultiplied(sz, &frame.data);
                        drop(s);
                        let tex = self.texture.get_or_insert_with(|| ctx.load_texture("vf", img.clone(), egui::TextureOptions::LINEAR));
                        tex.set(img, egui::TextureOptions::LINEAR);
                        let avail = ui.available_size();
                        let aspect = sz[0] as f32 / sz[1] as f32;
                        let dsz = if avail.x / avail.y > aspect { egui::vec2(avail.y * aspect, avail.y) } else { egui::vec2(avail.x, avail.x / aspect) };
                        let off = egui::vec2((avail.x - dsz.x) / 2.0, (avail.y - dsz.y) / 2.0);
                        let rect = egui::Rect::from_min_size(ui.min_rect().min + off, dsz);
                        ui.put(rect, egui::Image::new(egui::load::SizedTexture::new(tex.id(), dsz)));
                        self.handle_input(ui, rect);
                    } else {
                        drop(s);
                        ui.centered_and_justified(|ui| { ui.label("Waiting for video..."); });
                    }
                });
                ctx.request_repaint();
            }
        }
    }
}

// ── Agent background services ──

async fn start_agent_services(state: Arc<Mutex<SharedState>>) -> Result<()> {
    let identity = DeviceIdentity::load_or_create(None)?;
    let iroh = Arc::new(IrohTransport::new(identity.clone()).await?);

    // Update UI with device info + store iroh for outgoing connections
    {
        let mut s = state.lock().unwrap();
        s.device_id = iroh.device_id();
        s.connection_ticket = iroh.connection_ticket();
        s.device_name = identity.device_name().to_string();
        s.iroh = Some(iroh.clone());
        s.agent_ready = true;
    }

    tracing::info!(device_id = %iroh.device_id_short(), "agent ready");

    // Start LAN server
    let bind_addr: SocketAddr = format!("0.0.0.0:{DEFAULT_PORT}").parse()?;
    let server = QuicServer::bind(bind_addr).await?;

    // mDNS
    let discovery = LanDiscovery::new()?;
    discovery.register(identity.device_name(), server.local_addr().port(), std::env::consts::OS)?;

    // Start browsing for LAN devices (populate the UI list)
    let discovery_browse = LanDiscovery::new()?;
    let _ = discovery_browse.start_browsing();
    let state_disc = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let devices = discovery_browse.devices();
            state_disc.lock().unwrap().discovered_devices = devices;
        }
    });

    // Iroh accept loop (incoming internet connections)
    let iroh_clone = iroh.clone();
    tokio::spawn(async move {
        loop {
            match iroh_clone.accept().await {
                Ok(conn) => {
                    tracing::info!(remote = %conn.remote_id().fmt_short(), "incoming internet connection");
                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all().build().unwrap();
                        rt.block_on(async {
                            if let Err(e) = handle_incoming_iroh(conn).await {
                                tracing::error!(error = %e, "incoming session error");
                            }
                        });
                    });
                }
                Err(e) => { tracing::error!(error = %e, "iroh accept error"); break; }
            }
        }
    });

    // LAN accept loop (incoming LAN connections)
    loop {
        match server.accept().await {
            Ok(conn) => {
                tracing::info!(remote = %conn.remote_address(), "incoming LAN connection");
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all().build().unwrap();
                    rt.block_on(async {
                        if let Err(e) = handle_incoming_lan(conn).await {
                            tracing::error!(error = %e, "incoming LAN session error");
                        }
                    });
                });
            }
            Err(e) => { tracing::error!(error = %e, "LAN accept error"); }
        }
    }
}

// ── Incoming connections (this machine is being controlled) ──

async fn handle_incoming_iroh(conn: iroh::endpoint::Connection) -> Result<()> {
    let (ctrl_s, ctrl_r) = conn.accept_bi().await?;
    let video_s = conn.open_uni().await?;
    let audio_s = conn.open_uni().await?;
    let input = conn.accept_bi().await.ok();
    serve_session(
        MessageSender::new(ctrl_s), MessageReceiver::new(ctrl_r),
        MessageSender::new(video_s), MessageSender::new(audio_s),
        input.map(|(_, r)| MessageReceiver::new(r)),
    ).await
}

async fn handle_incoming_lan(conn: quinn::Connection) -> Result<()> {
    let (ctrl_s, ctrl_r) = conn.accept_bi().await?;
    let video_s = conn.open_uni().await?;
    let audio_s = conn.open_uni().await?;
    let input = conn.accept_bi().await.ok();
    serve_session(
        MessageSender::new(ctrl_s), MessageReceiver::new(ctrl_r),
        MessageSender::new(video_s), MessageSender::new(audio_s),
        input.map(|(_, r)| MessageReceiver::new(r)),
    ).await
}

/// Serve a remote session: capture screen, encode, stream, handle input
async fn serve_session<W, R>(
    mut ctrl_send: MessageSender<W>,
    mut ctrl_recv: MessageReceiver<R>,
    mut video_send: MessageSender<W>,
    mut audio_send: MessageSender<W>,
    input_recv: Option<MessageReceiver<R>>,
) -> Result<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    // Handshake
    let init_msg = ctrl_recv.recv().await.map_err(|e| anyhow::anyhow!("recv init: {e}"))?;
    if let Some(messages::message::Payload::SessionInit(init)) = &init_msg.payload {
        tracing::info!(device = %init.device_name, os = %init.os, "serving remote session");
    }

    let mut capturer = rd_capture::create_capturer()?;
    let first_frame = capturer.capture_frame()?;
    let width = first_frame.width & !1;
    let height = first_frame.height & !1;

    ctrl_send.send(&messages::Message {
        sequence: 1, timestamp_us: rd_protocol::now_us(),
        payload: Some(messages::message::Payload::SessionAccept(messages::SessionAccept {
            device_name: hostname::get().map(|h| h.to_string_lossy().to_string()).unwrap_or_default(),
            os: std::env::consts::OS.to_string(),
            selected_codec: messages::Codec::Vp9.into(),
        })),
    }).await.map_err(|e| anyhow::anyhow!("send accept: {e}"))?;

    let mut encoder = Encoder::new(CodecConfig { width, height, fps: TARGET_FPS, bitrate_kbps: 4000 })?;

    // Input handler
    if let Some(mut recv) = input_recv {
        let sw = width; let sh = height;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let mut inj = match rd_input::create_injector() { Ok(i) => i, Err(_) => return };
                let clipboard = ClipboardSync::new().ok();
                loop {
                    let msg = match recv.recv().await { Ok(m) => m, Err(_) => break };
                    match msg.payload {
                        Some(messages::message::Payload::InputEvent(input)) => { let _ = process_input(&mut *inj, &input, sw, sh); }
                        Some(messages::message::Payload::ClipboardUpdate(cb)) => {
                            if let Some(ref cb_sync) = clipboard { let _ = cb_sync.write(&ClipboardContent::Text(cb.text)); }
                        }
                        Some(messages::message::Payload::FileChunk(chunk)) => {
                            let path = rd_transfer::download_dir().join(format!("transfer_{}", chunk.transfer_id));
                            if let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await {
                                use tokio::io::AsyncWriteExt;
                                let _ = f.write_all(&chunk.data).await;
                                if chunk.is_last { tracing::info!(path = %path.display(), "file received"); }
                            }
                        }
                        _ => {}
                    }
                }
            });
        });
    }

    // Clipboard polling
    let clipboard_pending: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    { let p = clipboard_pending.clone(); std::thread::spawn(move || {
        if let Ok(cb) = ClipboardSync::new() { loop {
            std::thread::sleep(Duration::from_millis(CLIPBOARD_POLL_MS));
            if let Ok(Some(ClipboardContent::Text(t))) = cb.poll_change() { *p.lock().unwrap() = Some(t); }
        }}
    });}

    // Audio capture
    let audio_pending: Arc<std::sync::Mutex<Vec<Vec<u8>>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    { let p = audio_pending.clone(); std::thread::spawn(move || {
        if let Ok(mut cap) = rd_audio::AudioCapture::new() { loop {
            std::thread::sleep(Duration::from_millis(AUDIO_FRAME_MS));
            if let Ok(Some(data)) = cap.encode_frame() { p.lock().unwrap().push(data); }
        }}
    });}

    // Main capture loop
    let frame_interval = Duration::from_millis(1000 / TARGET_FPS as u64);
    let mut seq: u64 = 0;
    let mut audio_seq: u64 = 50000;

    loop {
        tokio::time::sleep(frame_interval).await;

        // Clipboard
        if let Some(text) = clipboard_pending.lock().unwrap().take() {
            seq += 1;
            let _ = ctrl_send.send(&messages::Message {
                sequence: seq, timestamp_us: rd_protocol::now_us(),
                payload: Some(messages::message::Payload::ClipboardUpdate(messages::ClipboardUpdate { text })),
            }).await;
        }

        // Audio
        for opus_data in std::mem::take(&mut *audio_pending.lock().unwrap()) {
            audio_seq += 1;
            if audio_send.send(&messages::Message {
                sequence: audio_seq, timestamp_us: rd_protocol::now_us(),
                payload: Some(messages::message::Payload::AudioFrame(messages::AudioFrame {
                    opus_data, sample_rate: rd_audio::SAMPLE_RATE, channels: rd_audio::CHANNELS as u32,
                })),
            }).await.is_err() { break; }
        }

        // Video
        let frame = match capturer.capture_frame() { Ok(f) => f, Err(_) => continue };
        let (y, u, v) = frame.to_i420();
        let encoded = match encoder.encode(&y, &u, &v) { Ok(f) => f, Err(_) => continue };
        for ef in &encoded {
            seq += 1;
            let msg = rd_protocol::video_frame_message(seq, seq, messages::Codec::Vp9, ef.is_keyframe, ef.width, ef.height, ef.data.clone());
            if video_send.send(&msg).await.is_err() { return Ok(()); }
        }
    }
}

// ── Outgoing connections (connecting to another machine) ──

async fn connect_lan(addr: SocketAddr, state: Arc<Mutex<SharedState>>, ctx: egui::Context) -> Result<()> {
    let client = rd_net::QuicClient::new()?;
    let conn = client.connect(addr).await?;
    let (cs, cr) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let (is, _) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let vr = conn.accept_uni().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let ar = conn.accept_uni().await.ok();
    view_session(MessageSender::new(cs), MessageReceiver::new(cr), MessageSender::new(is),
        MessageReceiver::new(vr), ar.map(MessageReceiver::new), state, ctx).await
}

async fn connect_internet(target: String, state: Arc<Mutex<SharedState>>, ctx: egui::Context) -> Result<()> {
    // Reuse the agent's iroh endpoint — don't create a second one
    let iroh = {
        let s = state.lock().unwrap();
        s.iroh.clone().ok_or_else(|| anyhow::anyhow!("Agent not ready yet, wait a moment"))?
    };

    // Try as connection ticket first, then as raw device ID
    let conn = if rd_net::iroh_transport::is_connection_ticket(&target) {
        tracing::info!("connecting via connection ticket");
        iroh.connect_by_ticket(&target).await?
    } else {
        tracing::info!("connecting via device ID (discovery)");
        let node_id = rd_net::iroh_transport::parse_device_id(&target)?;
        iroh.connect_by_id(node_id).await?
    };
    let (cs, cr) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let (is, _) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let vr = conn.accept_uni().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let ar = conn.accept_uni().await.ok();
    view_session(MessageSender::new(cs), MessageReceiver::new(cr), MessageSender::new(is),
        MessageReceiver::new(vr), ar.map(MessageReceiver::new), state, ctx).await
}

/// View a remote session: decode video, send input, play audio
async fn view_session<W, R>(
    mut ctrl_send: MessageSender<W>,
    mut ctrl_recv: MessageReceiver<R>,
    input_send: MessageSender<W>,
    mut video_recv: MessageReceiver<R>,
    audio_recv: Option<MessageReceiver<R>>,
    state: Arc<Mutex<SharedState>>,
    ctx: egui::Context,
) -> Result<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    // Send SessionInit
    ctrl_send.send(&messages::Message {
        sequence: 0, timestamp_us: rd_protocol::now_us(),
        payload: Some(messages::message::Payload::SessionInit(messages::SessionInit {
            device_name: hostname::get().map(|h| h.to_string_lossy().to_string()).unwrap_or_default(),
            os: std::env::consts::OS.to_string(), screen_width: 1920, screen_height: 1080,
            codecs: vec![messages::CodecCapability { codec: messages::Codec::Vp9.into(), hardware_accelerated: false }],
        })),
    }).await.map_err(|e| anyhow::anyhow!("send init: {e}"))?;

    let msg = ctrl_recv.recv().await.map_err(|e| anyhow::anyhow!("recv accept: {e}"))?;
    let remote_name = if let Some(messages::message::Payload::SessionAccept(a)) = &msg.payload {
        a.device_name.clone()
    } else { "Unknown".to_string() };

    state.lock().unwrap().status = ConnectionStatus::Connected { remote_name };
    ctx.request_repaint();

    // Heartbeat receiver + clipboard from remote
    let sr = state.clone();
    tokio::spawn(async move {
        let cb = ClipboardSync::new().ok();
        loop {
            match ctrl_recv.recv().await {
                Ok(m) => match m.payload {
                    Some(messages::message::Payload::HeartbeatAck(ack)) => {
                        sr.lock().unwrap().rtt_us = rd_protocol::now_us().saturating_sub(ack.echo_timestamp_us);
                    }
                    Some(messages::message::Payload::ClipboardUpdate(c)) => {
                        if let Some(ref cb) = cb { let _ = cb.write(&ClipboardContent::Text(c.text)); }
                    }
                    _ => {}
                },
                Err(_) => break,
            }
        }
    });

    // Heartbeat sender
    let cs = Arc::new(tokio::sync::Mutex::new(ctrl_send));
    let cs2 = cs.clone();
    tokio::spawn(async move {
        let mut seq = 1000u64;
        loop {
            tokio::time::sleep(Duration::from_millis(HEARTBEAT_INTERVAL_MS)).await;
            seq += 1;
            if cs2.lock().await.send(&rd_protocol::heartbeat_message(seq)).await.is_err() { break; }
        }
    });

    // Audio playback
    if let Some(mut ar) = audio_recv {
        tokio::spawn(async move {
            let mut pb = match AudioPlayback::new() { Ok(p) => p, Err(_) => return };
            loop {
                match ar.recv().await {
                    Ok(m) => {
                        if let Some(messages::message::Payload::AudioFrame(af)) = m.payload {
                            let _ = pb.decode_frame(&af.opus_data);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Clipboard polling on a separate thread (ClipboardSync is not Send)
    let cb_pending: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    { let p = cb_pending.clone(); std::thread::spawn(move || {
        if let Ok(cb) = ClipboardSync::new() { loop {
            std::thread::sleep(Duration::from_millis(CLIPBOARD_POLL_MS));
            if let Ok(Some(ClipboardContent::Text(t))) = cb.poll_change() { *p.lock().unwrap() = Some(t); }
        }}
    });}

    // Input sender + clipboard + file transfer
    let si = state.clone();
    tokio::spawn(async move {
        let mut send = input_send;
        let mut seq = 2000u64;
        let mut input_timer = tokio::time::interval(Duration::from_millis(8));
        loop {
            // File transfer
            let file_path = si.lock().unwrap().file_to_send.take();
            if let Some(path) = file_path {
                let tid = rd_protocol::now_us();
                if let Ok(mut sender) = FileSender::new(tid, &path).await {
                    si.lock().unwrap().transfer_progress = Some(0.0);
                    while let Ok(Some((offset, data, is_last))) = sender.next_chunk().await {
                        seq += 1;
                        let msg = messages::Message { sequence: seq, timestamp_us: rd_protocol::now_us(),
                            payload: Some(messages::message::Payload::FileChunk(messages::FileChunk { transfer_id: tid, offset, data, is_last })) };
                        if send.send(&msg).await.is_err() { return; }
                        si.lock().unwrap().transfer_progress = Some(sender.progress());
                    }
                    si.lock().unwrap().transfer_progress = None;
                }
            }

            input_timer.tick().await;

            // Send clipboard changes
            let cb_text = cb_pending.lock().unwrap().take();
            if let Some(text) = cb_text {
                seq += 1;
                let msg = messages::Message { sequence: seq, timestamp_us: rd_protocol::now_us(),
                    payload: Some(messages::message::Payload::ClipboardUpdate(messages::ClipboardUpdate { text })) };
                if send.send(&msg).await.is_err() { return; }
            }

            // Send input events
            let inputs: Vec<QueuedInput> = std::mem::take(&mut si.lock().unwrap().input_queue);
            for inp in inputs {
                seq += 1;
                let m = match inp {
                    QueuedInput::MouseMove { x, y } => rd_protocol::mouse_move_message(seq, x, y),
                    QueuedInput::MouseButton { button, action, x, y } => rd_protocol::mouse_button_message(seq, button, action, x, y),
                    QueuedInput::MouseScroll { delta_x, delta_y, x, y } => rd_protocol::mouse_scroll_message(seq, delta_x, delta_y, x, y),
                    QueuedInput::KeyPress { keycode, modifiers } => rd_protocol::key_event_message(seq, keycode, messages::ButtonAction::Press, modifiers),
                    QueuedInput::KeyRelease { keycode, modifiers } => rd_protocol::key_event_message(seq, keycode, messages::ButtonAction::Release, modifiers),
                };
                if send.send(&m).await.is_err() { return; }
            }
        }
    });

    // Video decode loop
    let mut decoder = Decoder::new()?;
    loop {
        match video_recv.recv().await {
            Ok(m) => {
                if let Some(messages::message::Payload::VideoFrame(f)) = m.payload {
                    state.lock().unwrap().remote_screen = (f.width, f.height);
                    if let Ok(d) = decoder.decode(&f.data, f.width, f.height) {
                        state.lock().unwrap().frame = Some(d);
                        ctx.request_repaint();
                    }
                }
            }
            Err(e) => { tracing::error!(error = %e, "video recv error"); break; }
        }
    }
    state.lock().unwrap().status = ConnectionStatus::Disconnected;
    ctx.request_repaint();
    Ok(())
}

// ── Input processing ──

fn process_input(inj: &mut dyn InputInjector, ev: &messages::InputEvent, sw: u32, sh: u32) -> Result<(), rd_input::InputError> {
    match &ev.event {
        Some(messages::input_event::Event::MouseMove(m)) => { inj.mouse_move((m.x * sw as f64) as i32, (m.y * sh as f64) as i32)?; }
        Some(messages::input_event::Event::MouseButton(b)) => {
            let btn = match messages::MouseButtonType::try_from(b.button) { Ok(messages::MouseButtonType::MouseButtonLeft) => MouseButton::Left, Ok(messages::MouseButtonType::MouseButtonRight) => MouseButton::Right, Ok(messages::MouseButtonType::MouseButtonMiddle) => MouseButton::Middle, _ => return Ok(()) };
            let act = match messages::ButtonAction::try_from(b.action) { Ok(messages::ButtonAction::Press) => ButtonAction::Press, Ok(messages::ButtonAction::Release) => ButtonAction::Release, _ => return Ok(()) };
            inj.mouse_move((b.x * sw as f64) as i32, (b.y * sh as f64) as i32)?;
            inj.mouse_button(btn, act)?;
        }
        Some(messages::input_event::Event::MouseScroll(s)) => { inj.mouse_scroll(s.delta_x, s.delta_y)?; }
        Some(messages::input_event::Event::KeyEvent(k)) => {
            let act = match messages::ButtonAction::try_from(k.action) { Ok(messages::ButtonAction::Press) => ButtonAction::Press, Ok(messages::ButtonAction::Release) => ButtonAction::Release, _ => return Ok(()) };
            inj.key_event(k.keycode, act)?;
        }
        None => {}
    }
    Ok(())
}

fn egui_key_to_keycode(key: egui::Key) -> u32 {
    match key {
        egui::Key::A => 0x04, egui::Key::B => 0x05, egui::Key::C => 0x06, egui::Key::D => 0x07,
        egui::Key::E => 0x08, egui::Key::F => 0x09, egui::Key::G => 0x0A, egui::Key::H => 0x0B,
        egui::Key::I => 0x0C, egui::Key::J => 0x0D, egui::Key::K => 0x0E, egui::Key::L => 0x0F,
        egui::Key::M => 0x10, egui::Key::N => 0x11, egui::Key::O => 0x12, egui::Key::P => 0x13,
        egui::Key::Q => 0x14, egui::Key::R => 0x15, egui::Key::S => 0x16, egui::Key::T => 0x17,
        egui::Key::U => 0x18, egui::Key::V => 0x19, egui::Key::W => 0x1A, egui::Key::X => 0x1B,
        egui::Key::Y => 0x1C, egui::Key::Z => 0x1D, egui::Key::Num0 => 0x27,
        egui::Key::Num1 => 0x1E, egui::Key::Num2 => 0x1F, egui::Key::Num3 => 0x20,
        egui::Key::Num4 => 0x21, egui::Key::Num5 => 0x22, egui::Key::Num6 => 0x23,
        egui::Key::Num7 => 0x24, egui::Key::Num8 => 0x25, egui::Key::Num9 => 0x26,
        egui::Key::Enter => 0x28, egui::Key::Escape => 0x29, egui::Key::Backspace => 0x2A,
        egui::Key::Tab => 0x2B, egui::Key::Space => 0x2C, egui::Key::Minus => 0x2D,
        egui::Key::ArrowUp => 0x52, egui::Key::ArrowDown => 0x51,
        egui::Key::ArrowLeft => 0x50, egui::Key::ArrowRight => 0x4F,
        egui::Key::Home => 0x4A, egui::Key::End => 0x4D,
        egui::Key::PageUp => 0x4B, egui::Key::PageDown => 0x4E,
        egui::Key::Insert => 0x49, egui::Key::Delete => 0x4C,
        egui::Key::F1 => 0x3A, egui::Key::F2 => 0x3B, egui::Key::F3 => 0x3C,
        egui::Key::F4 => 0x3D, egui::Key::F5 => 0x3E, egui::Key::F6 => 0x3F,
        egui::Key::F7 => 0x40, egui::Key::F8 => 0x41, egui::Key::F9 => 0x42,
        egui::Key::F10 => 0x43, egui::Key::F11 => 0x44, egui::Key::F12 => 0x45,
        _ => 0,
    }
}

fn egui_modifiers_to_flags(m: &egui::Modifiers) -> u32 {
    let mut f = 0u32;
    if m.shift { f |= 0x01; } if m.ctrl { f |= 0x02; }
    if m.alt { f |= 0x04; } if m.mac_cmd || m.command { f |= 0x08; }
    f
}
