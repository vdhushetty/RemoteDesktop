use anyhow::Result;
use eframe::egui;
use rd_audio::AudioPlayback;
use rd_clipboard::{ClipboardContent, ClipboardSync};
use rd_codec::{DecodedFrame, Decoder};
use rd_net::connection::{MessageReceiver, MessageSender};
use rd_net::{DeviceIdentity, IrohTransport, LanDiscovery, QuicClient};
use rd_protocol::messages;
use rd_transfer::FileSender;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

const DEFAULT_AGENT_PORT: u16 = 9876;
const HEARTBEAT_INTERVAL_MS: u64 = 2000;
const CLIPBOARD_POLL_MS: u64 = 500;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Remote Desktop Viewer")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Remote Desktop Viewer",
        options,
        Box::new(|cc| Ok(Box::new(ViewerApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}

#[derive(Debug, Clone)]
enum QueuedInput {
    MouseMove { x: f64, y: f64 },
    MouseButton { button: messages::MouseButtonType, action: messages::ButtonAction, x: f64, y: f64 },
    MouseScroll { delta_x: f64, delta_y: f64, x: f64, y: f64 },
    KeyPress { keycode: u32, modifiers: u32 },
    KeyRelease { keycode: u32, modifiers: u32 },
}

struct SharedState {
    frame: Option<DecodedFrame>,
    status: ConnectionStatus,
    discovered_devices: Vec<rd_net::discovery::DiscoveredDevice>,
    input_queue: Vec<QueuedInput>,
    rtt_us: u64,
    remote_screen: (u32, u32),
    /// File path to send (set by UI, consumed by network task)
    file_to_send: Option<std::path::PathBuf>,
    /// Transfer progress 0.0-1.0
    transfer_progress: Option<f64>,
    audio_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Disconnected, Connecting, Connected { remote_name: String }, Error(String),
}

struct ViewerApp {
    state: Arc<Mutex<SharedState>>,
    texture: Option<egui::TextureHandle>,
    connect_addr: String,
    connect_device_id: String,
    discovery: Option<LanDiscovery>,
    runtime: tokio::runtime::Runtime,
    identity: Option<DeviceIdentity>,
}

impl ViewerApp {
    fn new(_cc: &eframe::CreationContext) -> Self {
        let state = Arc::new(Mutex::new(SharedState {
            frame: None, status: ConnectionStatus::Disconnected,
            discovered_devices: vec![], input_queue: vec![],
            rtt_us: 0, remote_screen: (1920, 1080),
            file_to_send: None, transfer_progress: None,
            audio_enabled: true,
        }));
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let discovery = LanDiscovery::new().ok();
        if let Some(ref d) = discovery { let _ = d.start_browsing(); }
        let identity = DeviceIdentity::load_or_create(None).ok();

        Self {
            state, texture: None,
            connect_addr: format!("127.0.0.1:{DEFAULT_AGENT_PORT}"),
            connect_device_id: String::new(),
            discovery, runtime, identity,
        }
    }

    fn connect_lan(&mut self, addr: SocketAddr, ctx: egui::Context) {
        let state = self.state.clone();
        state.lock().unwrap().status = ConnectionStatus::Connecting;
        self.runtime.spawn(async move {
            let result: Result<()> = async {
                let client = QuicClient::new()?;
                let conn = client.connect(addr).await?;
                let (cs, cr) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let (is, _) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let vr = conn.accept_uni().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                // Accept audio uni stream from agent
                let ar = conn.accept_uni().await.ok();
                run_session(
                    MessageSender::new(cs), MessageReceiver::new(cr),
                    MessageSender::new(is), MessageReceiver::new(vr),
                    ar.map(MessageReceiver::new),
                    state.clone(), ctx.clone(),
                ).await
            }.await;
            if let Err(e) = result {
                tracing::error!(error = %e, "LAN session error");
                state.lock().unwrap().status = ConnectionStatus::Error(e.to_string());
                ctx.request_repaint();
            }
        });
    }

    fn connect_internet(&mut self, device_id: String, ctx: egui::Context) {
        let state = self.state.clone();
        state.lock().unwrap().status = ConnectionStatus::Connecting;
        let identity = self.identity.clone();
        self.runtime.spawn(async move {
            let result: Result<()> = async {
                let identity = identity.ok_or_else(|| anyhow::anyhow!("no identity"))?;
                let iroh = IrohTransport::new(identity).await?;
                let node_id = rd_net::iroh_transport::parse_device_id(&device_id)?;
                let conn = iroh.connect_by_id(node_id).await?;
                let (cs, cr) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let (is, _) = conn.open_bi().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let vr = conn.accept_uni().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                let ar = conn.accept_uni().await.ok();
                run_session(
                    MessageSender::new(cs), MessageReceiver::new(cr),
                    MessageSender::new(is), MessageReceiver::new(vr),
                    ar.map(MessageReceiver::new),
                    state.clone(), ctx.clone(),
                ).await
            }.await;
            if let Err(e) = result {
                tracing::error!(error = %e, "internet session error");
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

impl eframe::App for ViewerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(ref d) = self.discovery {
            self.state.lock().unwrap().discovered_devices = d.devices();
        }
        let (status, devices, rtt, transfer_progress) = {
            let s = self.state.lock().unwrap();
            (s.status.clone(), s.discovered_devices.clone(), s.rtt_us, s.transfer_progress)
        };

        match status {
            ConnectionStatus::Disconnected | ConnectionStatus::Error(_) => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("Remote Desktop Viewer");
                        ui.add_space(20.0);
                        if let ConnectionStatus::Error(ref e) = status {
                            ui.colored_label(egui::Color32::RED, format!("Error: {e}"));
                            ui.add_space(10.0);
                        }
                        ui.group(|ui| {
                            ui.label("Connect over Internet (Device ID):");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.connect_device_id);
                                if ui.button("Connect").clicked() && !self.connect_device_id.is_empty() {
                                    self.connect_internet(self.connect_device_id.trim().to_string(), ctx.clone());
                                }
                            });
                        });
                        ui.add_space(10.0);
                        ui.group(|ui| {
                            ui.label("Connect by IP (LAN):");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.connect_addr);
                                if ui.button("Connect").clicked() {
                                    if let Ok(addr) = self.connect_addr.parse::<SocketAddr>() {
                                        self.connect_lan(addr, ctx.clone());
                                    }
                                }
                            });
                        });
                        ui.add_space(10.0);
                        if !devices.is_empty() {
                            ui.group(|ui| {
                                ui.label("LAN Devices:");
                                for dev in &devices {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{} ({}) - {}", dev.name, dev.os, dev.addr));
                                        if ui.button("Connect").clicked() { self.connect_lan(dev.addr, ctx.clone()); }
                                    });
                                }
                            });
                        } else { ui.label("Searching for LAN devices..."); }
                    });
                });
            }
            ConnectionStatus::Connecting => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| { ui.add_space(200.0); ui.heading("Connecting..."); ui.spinner(); });
                });
            }
            ConnectionStatus::Connected { ref remote_name } => {
                // Toolbar with controls
                egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Connected to: {remote_name}"));
                        if rtt > 0 {
                            ui.separator();
                            ui.label(format!("RTT: {:.1}ms", rtt as f64 / 1000.0));
                        }
                        ui.separator();

                        // Send File button
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

                // Video display
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

async fn run_session<W, R>(
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

    // Wait for SessionAccept
    let msg = ctrl_recv.recv().await.map_err(|e| anyhow::anyhow!("recv accept: {e}"))?;
    let remote_name = if let Some(messages::message::Payload::SessionAccept(a)) = &msg.payload {
        tracing::info!(remote = %a.device_name, "session accepted");
        a.device_name.clone()
    } else { "Unknown".to_string() };

    state.lock().unwrap().status = ConnectionStatus::Connected { remote_name };
    ctx.request_repaint();

    // Heartbeat receiver + clipboard from agent
    let state_r = state.clone();
    tokio::spawn(async move {
        let clipboard = ClipboardSync::new().ok();
        loop {
            match ctrl_recv.recv().await {
                Ok(m) => match m.payload {
                    Some(messages::message::Payload::HeartbeatAck(ack)) => {
                        state_r.lock().unwrap().rtt_us = rd_protocol::now_us().saturating_sub(ack.echo_timestamp_us);
                    }
                    Some(messages::message::Payload::ClipboardUpdate(cb)) => {
                        if let Some(ref clipboard) = clipboard {
                            let _ = clipboard.write(&ClipboardContent::Text(cb.text));
                        }
                    }
                    _ => {}
                },
                Err(_) => break,
            }
        }
    });

    // Heartbeat sender
    let ctrl_send = Arc::new(tokio::sync::Mutex::new(ctrl_send));
    let cs = ctrl_send.clone();
    tokio::spawn(async move {
        let mut seq = 1000u64;
        loop {
            tokio::time::sleep(Duration::from_millis(HEARTBEAT_INTERVAL_MS)).await;
            seq += 1;
            if cs.lock().await.send(&rd_protocol::heartbeat_message(seq)).await.is_err() { break; }
        }
    });

    // Audio playback
    if let Some(mut audio_recv) = audio_recv {
        tokio::spawn(async move {
            let mut playback = match AudioPlayback::new() {
                Ok(p) => { tracing::info!("audio playback started"); p }
                Err(e) => { tracing::warn!(error = %e, "audio playback not available"); return; }
            };
            loop {
                match audio_recv.recv().await {
                    Ok(m) => {
                        if let Some(messages::message::Payload::AudioFrame(af)) = m.payload {
                            if let Err(e) = playback.decode_frame(&af.opus_data) {
                                tracing::debug!(error = %e, "audio decode error");
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Input sender + clipboard sync + file transfer
    let si = state.clone();
    tokio::spawn(async move {
        let mut send = input_send;
        let mut seq = 2000u64;
        let clipboard = ClipboardSync::new().ok();
        let mut clipboard_timer = tokio::time::interval(Duration::from_millis(CLIPBOARD_POLL_MS));
        let mut input_timer = tokio::time::interval(Duration::from_millis(8));

        loop {
            // Check for file transfer request
            let file_path = si.lock().unwrap().file_to_send.take();
            if let Some(path) = file_path {
                tracing::info!(path = %path.display(), "starting file transfer");
                let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let transfer_id = rd_protocol::now_us();

                match FileSender::new(transfer_id, &path).await {
                    Ok(mut sender) => {
                        si.lock().unwrap().transfer_progress = Some(0.0);
                        // Send file chunks over the input stream
                        while let Ok(Some((offset, data, is_last))) = sender.next_chunk().await {
                            seq += 1;
                            let msg = messages::Message {
                                sequence: seq,
                                timestamp_us: rd_protocol::now_us(),
                                payload: Some(messages::message::Payload::FileChunk(
                                    messages::FileChunk {
                                        transfer_id,
                                        offset,
                                        data,
                                        is_last,
                                    },
                                )),
                            };
                            if send.send(&msg).await.is_err() { return; }
                            si.lock().unwrap().transfer_progress = Some(sender.progress());
                        }
                        si.lock().unwrap().transfer_progress = None;
                        tracing::info!(filename, "file transfer complete");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "file transfer failed");
                        si.lock().unwrap().transfer_progress = None;
                    }
                }
            }

            tokio::select! {
                _ = input_timer.tick() => {
                    let q: Vec<QueuedInput> = std::mem::take(&mut si.lock().unwrap().input_queue);
                    for inp in q {
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
                _ = clipboard_timer.tick() => {
                    if let Some(ref clipboard) = clipboard {
                        if let Ok(Some(ClipboardContent::Text(text))) = clipboard.poll_change() {
                            seq += 1;
                            let msg = messages::Message {
                                sequence: seq,
                                timestamp_us: rd_protocol::now_us(),
                                payload: Some(messages::message::Payload::ClipboardUpdate(
                                    messages::ClipboardUpdate { text },
                                )),
                            };
                            if send.send(&msg).await.is_err() { return; }
                        }
                    }
                }
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
