use crate::config::Config;
use crate::services::{RbnClient, RbnMessage, SpotStore, VfdDisplay};
use eframe::egui;
use std::time::{Duration, Instant};

/// Max lines to keep in raw data log
const RAW_DATA_LOG_MAX_LINES: usize = 500;

/// Main application state
pub struct RbnVfdApp {
    config: Config,
    spot_store: SpotStore,
    vfd_display: VfdDisplay,
    rbn_client: Option<RbnClient>,
    callsign_input: String,
    selected_port: String,
    available_ports: Vec<String>,
    status_message: String,
    is_connected: bool,
    last_purge: Instant,
    last_port_refresh: Instant,
    /// Raw telnet data log for debugging
    raw_data_log: Vec<String>,
}

impl RbnVfdApp {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let spot_store = SpotStore::new();
        let mut vfd_display = VfdDisplay::new();
        vfd_display.set_scroll_interval(config.scroll_interval_seconds);
        vfd_display.set_random_char_percent(config.random_char_percent);

        let available_ports = VfdDisplay::available_ports();
        let selected_port = if available_ports.contains(&config.serial_port) {
            config.serial_port.clone()
        } else {
            available_ports.first().cloned().unwrap_or_default()
        };

        Self {
            callsign_input: config.callsign.clone(),
            config,
            spot_store,
            vfd_display,
            rbn_client: None,
            selected_port,
            available_ports,
            status_message: "Ready".to_string(),
            is_connected: false,
            last_purge: Instant::now(),
            last_port_refresh: Instant::now(),
            raw_data_log: Vec::new(),
        }
    }

    /// Connect to RBN server
    fn connect_rbn(&mut self) {
        if self.callsign_input.trim().is_empty() {
            self.status_message = "Please enter a callsign".to_string();
            return;
        }

        let callsign = self.callsign_input.trim().to_uppercase();
        self.config.callsign = callsign.clone();

        let client = RbnClient::new();
        client.connect(callsign);

        self.rbn_client = Some(client);
        self.is_connected = true;
        self.status_message = "Connecting...".to_string();
    }

    /// Disconnect from RBN server
    fn disconnect_rbn(&mut self) {
        if let Some(ref client) = self.rbn_client {
            client.disconnect();
        }
        self.rbn_client = None;
        self.is_connected = false;
        self.status_message = "Disconnected".to_string();
    }

    /// Open VFD on selected port
    fn open_vfd(&mut self) {
        if self.selected_port.is_empty() {
            self.status_message = "No serial port selected".to_string();
            return;
        }

        match self.vfd_display.open(&self.selected_port) {
            Ok(()) => {
                self.config.serial_port = self.selected_port.clone();
                self.status_message = format!("VFD opened on {}", self.selected_port);
            }
            Err(e) => {
                self.status_message = format!("Failed to open VFD: {}", e);
            }
        }
    }

    /// Close VFD
    fn close_vfd(&mut self) {
        self.vfd_display.close();
        self.status_message = "VFD closed".to_string();
    }

    /// Process incoming RBN messages
    fn process_rbn_messages(&mut self) {
        // Collect messages first to avoid borrow conflicts
        let messages: Vec<RbnMessage> = if let Some(ref mut client) = self.rbn_client {
            let mut msgs = Vec::new();
            while let Some(msg) = client.try_recv() {
                msgs.push(msg);
            }
            msgs
        } else {
            Vec::new()
        };

        // Process collected messages
        let mut should_disconnect = false;
        for msg in messages {
            match msg {
                RbnMessage::Status(s) => {
                    self.status_message = s;
                }
                RbnMessage::Spot(raw) => {
                    self.spot_store.add_spot(raw);
                }
                RbnMessage::Disconnected => {
                    self.is_connected = false;
                    should_disconnect = true;
                }
                RbnMessage::RawData { data, received } => {
                    let prefix = if received { "<<" } else { ">>" };
                    let line = format!("{} {}", prefix, data.trim_end());
                    self.raw_data_log.push(line);
                    // Keep log from growing too large
                    if self.raw_data_log.len() > RAW_DATA_LOG_MAX_LINES {
                        self.raw_data_log.remove(0);
                    }
                }
            }
        }

        if should_disconnect {
            self.rbn_client = None;
        }
    }

    /// Perform periodic updates
    fn update_periodic(&mut self) {
        let now = Instant::now();

        // Purge old spots every 5 seconds
        if now.duration_since(self.last_purge) >= Duration::from_secs(5) {
            self.spot_store.purge_old_spots();
            self.last_purge = now;
        }

        // Refresh available ports every 5 seconds
        if now.duration_since(self.last_port_refresh) >= Duration::from_secs(5) {
            self.available_ports = VfdDisplay::available_ports();
            self.last_port_refresh = now;
        }

        // Update VFD display
        let max_age = Duration::from_secs(self.config.max_age_minutes as u64 * 60);
        let spots = self
            .spot_store
            .get_filtered_spots(self.config.min_snr, max_age);
        self.vfd_display.update(&spots);
    }
}

/// Draw an age ring indicator
fn draw_age_ring(ui: &mut egui::Ui, fraction: f32) {
    let size = 16.0;
    let (response, painter) = ui.allocate_painter(egui::Vec2::splat(size), egui::Sense::hover());
    let center = response.rect.center();
    let radius = size / 2.0 - 2.0;

    // Ring color - static green
    let color = egui::Color32::from_rgb(0, 200, 0);

    // Draw background circle (dim)
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(2.0, egui::Color32::from_rgb(40, 40, 40)),
    );

    // Draw arc for remaining time (1.0 - fraction = remaining)
    let remaining = 1.0 - fraction;
    if remaining > 0.001 {
        // Arc from 12 o'clock (-PI/2), sweeping counter-clockwise
        let start_angle = -std::f32::consts::FRAC_PI_2;
        let sweep = remaining * std::f32::consts::TAU;

        // Draw arc as series of line segments (no allocation)
        let segments = 32;
        for i in 0..segments {
            let t0 = i as f32 / segments as f32;
            let t1 = (i + 1) as f32 / segments as f32;
            let angle0 = start_angle - t0 * sweep;
            let angle1 = start_angle - t1 * sweep;

            let p0 = egui::Pos2::new(
                center.x + radius * angle0.cos(),
                center.y + radius * angle0.sin(),
            );
            let p1 = egui::Pos2::new(
                center.x + radius * angle1.cos(),
                center.y + radius * angle1.sin(),
            );

            painter.line_segment([p0, p1], egui::Stroke::new(2.0, color));
        }
    }
}

impl eframe::App for RbnVfdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process messages and periodic updates
        self.process_rbn_messages();
        self.update_periodic();

        // Request repaint for continuous updates
        ctx.request_repaint_after(Duration::from_millis(100));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("RBN VFD Display");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            ui.separator();

            // Connection section
            ui.horizontal(|ui| {
                ui.label("Callsign:");
                let response = ui.text_edit_singleline(&mut self.callsign_input);
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.is_connected
                {
                    self.connect_rbn();
                }

                if self.is_connected {
                    if ui.button("Disconnect").clicked() {
                        self.disconnect_rbn();
                    }
                } else if ui.button("Connect").clicked() {
                    self.connect_rbn();
                }
            });

            ui.add_space(4.0);

            // Serial port section
            ui.horizontal(|ui| {
                ui.label("VFD Port:");

                egui::ComboBox::from_id_salt("port_selector")
                    .selected_text(&self.selected_port)
                    .show_ui(ui, |ui| {
                        for port in &self.available_ports {
                            ui.selectable_value(&mut self.selected_port, port.clone(), port);
                        }
                    });

                if self.vfd_display.is_open() {
                    if ui.button("Close").clicked() {
                        self.close_vfd();
                    }
                    if ui.button("Blank").clicked() {
                        self.vfd_display.clear();
                        self.status_message = "Display blanked".to_string();
                    }
                } else if ui.button("Open").clicked() {
                    self.open_vfd();
                }
            });

            ui.add_space(4.0);

            // Status line
            ui.horizontal(|ui| {
                ui.label("Status:");
                ui.label(&self.status_message);
            });

            if self.vfd_display.is_open() {
                ui.horizontal(|ui| {
                    ui.label("VFD:");
                    ui.label(format!("Open on {}", self.vfd_display.port_name()));
                });
            }

            ui.separator();

            // Filter controls
            ui.collapsing("Filters", |ui| {
                // Min SNR slider
                ui.horizontal(|ui| {
                    ui.label("Min SNR:");
                    let mut snr = self.config.min_snr;
                    if ui
                        .add(egui::Slider::new(&mut snr, 0..=50).suffix(" dB"))
                        .changed()
                    {
                        self.config.min_snr = snr;
                    }
                });

                ui.add_space(4.0);

                // Max age radio buttons
                ui.horizontal(|ui| {
                    ui.label("Max Age:");
                    let age_options = [1u32, 5, 10, 15, 30];
                    for age in age_options {
                        if ui
                            .radio(self.config.max_age_minutes == age, format!("{} min", age))
                            .clicked()
                        {
                            self.config.max_age_minutes = age;
                        }
                    }
                });

                ui.add_space(4.0);

                // Scroll interval radio buttons
                ui.horizontal(|ui| {
                    ui.label("Scroll:");
                    let scroll_options = [1u32, 3, 5, 10, 30];
                    for secs in scroll_options {
                        if ui
                            .radio(
                                self.config.scroll_interval_seconds == secs,
                                format!("{} sec", secs),
                            )
                            .clicked()
                        {
                            self.config.scroll_interval_seconds = secs;
                            self.vfd_display.set_scroll_interval(secs);
                        }
                    }
                });

                ui.add_space(4.0);

                // Force random mode checkbox
                ui.horizontal(|ui| {
                    let mut force_random = self.vfd_display.is_in_random_mode();
                    if ui
                        .checkbox(&mut force_random, "Force random mode")
                        .clicked()
                    {
                        self.vfd_display.set_force_random_mode(force_random);
                    }
                });

                ui.add_space(4.0);

                // Random char duty cycle slider
                ui.horizontal(|ui| {
                    ui.label("Random Duty Cycle:");
                    let mut percent = self.config.random_char_percent;
                    if ui
                        .add(egui::Slider::new(&mut percent, 0..=100).suffix("%"))
                        .changed()
                    {
                        self.config.random_char_percent = percent;
                        self.vfd_display.set_random_char_percent(percent);
                    }
                });

                ui.add_space(4.0);

                // Restore defaults button
                if ui.button("Restore Defaults").clicked() {
                    self.config.reset_to_defaults();
                    self.vfd_display
                        .set_scroll_interval(self.config.scroll_interval_seconds);
                    self.vfd_display
                        .set_random_char_percent(self.config.random_char_percent);
                }
            });

            ui.separator();

            // VFD Preview
            ui.collapsing("VFD Preview", |ui| {
                let preview = self.vfd_display.get_preview();

                // Create a frame with green-on-black styling
                egui::Frame::new()
                    .fill(egui::Color32::BLACK)
                    .inner_margin(egui::Margin::same(8))
                    .corner_radius(egui::CornerRadius::same(4))
                    .show(ui, |ui| {
                        ui.style_mut().visuals.override_text_color =
                            Some(egui::Color32::from_rgb(0, 255, 0));

                        // Use monospace font
                        let line1 = if preview[0].is_empty() {
                            " ".repeat(20)
                        } else {
                            format!("{:20}", preview[0])
                        };
                        let line2 = if preview[1].is_empty() {
                            " ".repeat(20)
                        } else {
                            format!("{:20}", preview[1])
                        };

                        ui.label(egui::RichText::new(&line1).monospace().size(16.0));
                        ui.label(egui::RichText::new(&line2).monospace().size(16.0));
                    });
            });

            ui.separator();

            // Raw telnet data log
            ui.collapsing("Raw Telnet Data", |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} lines", self.raw_data_log.len()));
                    if ui.button("Clear").clicked() {
                        self.raw_data_log.clear();
                    }
                });

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(20, 20, 20))
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                for line in &self.raw_data_log {
                                    let color = if line.starts_with("<<") {
                                        egui::Color32::from_rgb(100, 255, 100) // received = green
                                    } else {
                                        egui::Color32::from_rgb(100, 100, 255) // sent = blue
                                    };
                                    ui.label(
                                        egui::RichText::new(line)
                                            .monospace()
                                            .size(11.0)
                                            .color(color),
                                    );
                                }
                            });
                    });
            });

            ui.separator();

            // Active spots list
            ui.horizontal(|ui| {
                ui.heading(format!("Active Spots ({})", self.spot_store.count()));
                if ui.button("Clear").clicked() {
                    self.spot_store.clear();
                }
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let max_age = Duration::from_secs(self.config.max_age_minutes as u64 * 60);
                    let spots = self
                        .spot_store
                        .get_filtered_spots(self.config.min_snr, max_age);
                    if spots.is_empty() {
                        ui.label("No spots yet. Connect to RBN to receive spots.");
                    } else {
                        // Header
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:>10}", "Freq"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:<10}", "Callsign"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:>4}", "SNR"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:>5}", "WPM"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:>5}", "#"))
                                    .monospace()
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("{:>6}", "Age"))
                                    .monospace()
                                    .strong(),
                            );
                        });

                        ui.separator();

                        for spot in spots {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{:>10.1}", spot.frequency_khz))
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{:<10}", spot.callsign))
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{:>4}", spot.highest_snr))
                                        .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:>5}",
                                        spot.average_speed.round() as i32
                                    ))
                                    .monospace(),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{:>5}", spot.spot_count))
                                        .monospace(),
                                );

                                // Age display
                                let age_secs = spot.age_seconds();
                                let age_text = if age_secs < 60 {
                                    format!("{:>3}s", age_secs)
                                } else {
                                    format!("{:>3}m", age_secs / 60)
                                };
                                ui.label(egui::RichText::new(age_text).monospace());

                                // Ring indicator
                                let max_age =
                                    Duration::from_secs(self.config.max_age_minutes as u64 * 60);
                                let fraction = spot.age_fraction(max_age);
                                draw_age_ring(ui, fraction);
                            });
                        }
                    }
                });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Disconnect from RBN
        if self.rbn_client.is_some() {
            self.disconnect_rbn();
        }

        // Close VFD
        self.vfd_display.close();

        // Save config
        if let Err(e) = self.config.save() {
            eprintln!("Failed to save config: {}", e);
        }
    }
}
