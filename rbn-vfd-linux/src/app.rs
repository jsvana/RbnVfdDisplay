use crate::config::Config;
use crate::services::{RbnClient, RbnMessage, SpotStore, VfdDisplay};
use eframe::egui;
use std::time::{Duration, Instant};

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
}

impl RbnVfdApp {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let spot_store = SpotStore::new(config.min_snr, config.max_age_minutes);
        let mut vfd_display = VfdDisplay::new();
        vfd_display.set_scroll_interval(config.scroll_interval_seconds);

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
        let spots = self.spot_store.get_spots_by_frequency();
        self.vfd_display.update(&spots);
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
            ui.heading("RBN VFD Display");
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
                        self.spot_store.set_min_snr(snr);
                    }
                });

                ui.add_space(4.0);

                // Max age radio buttons
                ui.horizontal(|ui| {
                    ui.label("Max Age:");
                    let age_options = [5u32, 10, 15, 30];
                    for age in age_options {
                        if ui
                            .radio(self.config.max_age_minutes == age, format!("{} min", age))
                            .clicked()
                        {
                            self.config.max_age_minutes = age;
                            self.spot_store.set_max_age_minutes(age);
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
                    if ui.checkbox(&mut force_random, "Force random mode").clicked() {
                        self.vfd_display.set_force_random_mode(force_random);
                    }
                });

                ui.add_space(4.0);

                // Restore defaults button
                if ui.button("Restore Defaults").clicked() {
                    self.config.reset_to_defaults();
                    self.spot_store.set_min_snr(self.config.min_snr);
                    self.spot_store.set_max_age_minutes(self.config.max_age_minutes);
                    self.vfd_display
                        .set_scroll_interval(self.config.scroll_interval_seconds);
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

                        ui.label(
                            egui::RichText::new(&line1)
                                .monospace()
                                .size(16.0),
                        );
                        ui.label(
                            egui::RichText::new(&line2)
                                .monospace()
                                .size(16.0),
                        );
                    });
            });

            ui.separator();

            // Active spots list
            ui.heading(format!("Active Spots ({})", self.spot_store.count()));

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let spots = self.spot_store.get_spots_by_frequency();
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
