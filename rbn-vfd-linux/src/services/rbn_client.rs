use crate::models::RawSpot;
use regex::Regex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const RBN_HOST: &str = "rbn.telegraphy.de";
const RBN_PORT: u16 = 7000;

/// Messages sent from the RBN client to the main app
#[derive(Debug, Clone)]
pub enum RbnMessage {
    Status(String),
    Spot(RawSpot),
    Disconnected,
    /// Raw data for debugging (direction: true = received, false = sent)
    RawData { data: String, received: bool },
}

/// Commands sent to the RBN client
#[derive(Debug)]
pub enum RbnCommand {
    Connect(String),
    Disconnect,
}

/// Handle to communicate with the RBN client task
pub struct RbnClient {
    cmd_tx: mpsc::Sender<RbnCommand>,
    msg_rx: mpsc::Receiver<RbnMessage>,
}

impl RbnClient {
    /// Create a new RBN client and spawn the background task
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (msg_tx, msg_rx) = mpsc::channel(256);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");
            rt.block_on(rbn_task(cmd_rx, msg_tx));
        });

        Self { cmd_tx, msg_rx }
    }

    /// Send a connect command (non-blocking from UI)
    pub fn connect(&self, callsign: String) {
        let tx = self.cmd_tx.clone();
        let _ = tx.blocking_send(RbnCommand::Connect(callsign));
    }

    /// Send a disconnect command (non-blocking from UI)
    pub fn disconnect(&self) {
        let tx = self.cmd_tx.clone();
        let _ = tx.blocking_send(RbnCommand::Disconnect);
    }

    /// Try to receive a message (non-blocking)
    pub fn try_recv(&mut self) -> Option<RbnMessage> {
        self.msg_rx.try_recv().ok()
    }
}

async fn rbn_task(mut cmd_rx: mpsc::Receiver<RbnCommand>, msg_tx: mpsc::Sender<RbnMessage>) {
    let spot_regex = Regex::new(
        r"DX de (\S+):\s+(\d+\.?\d*)\s+(\S+)\s+(\w+)\s+(\d+)\s+dB\s+(\d+)\s+WPM",
    )
    .expect("Invalid regex");

    loop {
        // Wait for a connect command
        let callsign = loop {
            match cmd_rx.recv().await {
                Some(RbnCommand::Connect(cs)) => break cs,
                Some(RbnCommand::Disconnect) => continue,
                None => return, // Channel closed
            }
        };

        let _ = msg_tx
            .send(RbnMessage::Status(format!(
                "Connecting to {}:{}...",
                RBN_HOST, RBN_PORT
            )))
            .await;

        // Try to connect
        let stream = match TcpStream::connect((RBN_HOST, RBN_PORT)).await {
            Ok(s) => s,
            Err(e) => {
                let _ = msg_tx
                    .send(RbnMessage::Status(format!("Connection failed: {}", e)))
                    .await;
                let _ = msg_tx.send(RbnMessage::Disconnected).await;
                continue;
            }
        };

        let _ = msg_tx
            .send(RbnMessage::Status(
                "Connected, waiting for login prompt...".to_string(),
            ))
            .await;

        // Handle the connection
        handle_connection(stream, &callsign, &mut cmd_rx, &msg_tx, &spot_regex).await;

        let _ = msg_tx.send(RbnMessage::Disconnected).await;
    }
}

async fn handle_connection(
    stream: TcpStream,
    callsign: &str,
    cmd_rx: &mut mpsc::Receiver<RbnCommand>,
    msg_tx: &mpsc::Sender<RbnMessage>,
    spot_regex: &Regex,
) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut logged_in = false;

    loop {
        tokio::select! {
            // Check for commands
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(RbnCommand::Disconnect) | None => {
                        let _ = msg_tx.send(RbnMessage::Status("Disconnected".to_string())).await;
                        return;
                    }
                    Some(RbnCommand::Connect(_)) => {
                        // Already connected, ignore
                    }
                }
            }

            // Read from stream
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => {
                        let _ = msg_tx.send(RbnMessage::Status("Connection closed by server".to_string())).await;
                        return;
                    }
                    Ok(_) => {
                        // Send raw received data for debugging
                        let _ = msg_tx
                            .send(RbnMessage::RawData {
                                data: line.clone(),
                                received: true,
                            })
                            .await;

                        // Handle login
                        if !logged_in
                            && line.to_lowercase().contains("please enter your callsign")
                        {
                            let send_data = format!("{}\r\n", callsign);
                            if writer.write_all(send_data.as_bytes()).await.is_ok() {
                                // Send raw sent data for debugging
                                let _ = msg_tx
                                    .send(RbnMessage::RawData {
                                        data: send_data,
                                        received: false,
                                    })
                                    .await;
                                let _ = msg_tx
                                    .send(RbnMessage::Status(format!("Logged in as {}", callsign)))
                                    .await;
                                logged_in = true;
                            }
                        }

                        // Parse spots
                        if line.starts_with("DX de") {
                            if let Some(spot) = parse_spot_line(&line, spot_regex) {
                                let _ = msg_tx.send(RbnMessage::Spot(spot)).await;
                            }
                        }

                        line.clear();
                    }
                    Err(e) => {
                        let _ = msg_tx.send(RbnMessage::Status(format!("Read error: {}", e))).await;
                        return;
                    }
                }
            }
        }
    }
}

fn parse_spot_line(line: &str, regex: &Regex) -> Option<RawSpot> {
    let caps = regex.captures(line)?;

    Some(RawSpot::new(
        caps.get(1)?
            .as_str()
            .trim_end_matches(['-', '#', ':'])
            .to_string(),
        caps.get(3)?.as_str().to_string(),
        caps.get(2)?.as_str().parse().ok()?,
        caps.get(5)?.as_str().parse().ok()?,
        caps.get(6)?.as_str().parse().ok()?,
        caps.get(4)?.as_str().to_string(),
    ))
}
