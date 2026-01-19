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
}

/// Commands sent to the RBN client
#[derive(Debug)]
pub enum RbnCommand {
    Connect(String), // callsign
    Disconnect,
}

/// RBN client that runs in a tokio task
pub struct RbnClient {
    cmd_tx: mpsc::Sender<RbnCommand>,
    msg_rx: mpsc::Receiver<RbnMessage>,
}

impl RbnClient {
    /// Create a new RBN client and spawn the background task
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (msg_tx, msg_rx) = mpsc::channel(256);

        tokio::spawn(rbn_task(cmd_rx, msg_tx));

        Self { cmd_tx, msg_rx }
    }

    /// Send a connect command
    pub async fn connect(&self, callsign: String) -> Result<(), String> {
        self.cmd_tx
            .send(RbnCommand::Connect(callsign))
            .await
            .map_err(|e| format!("Failed to send connect command: {}", e))
    }

    /// Send a disconnect command
    pub async fn disconnect(&self) -> Result<(), String> {
        self.cmd_tx
            .send(RbnCommand::Disconnect)
            .await
            .map_err(|e| format!("Failed to send disconnect command: {}", e))
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
    .unwrap();

    let mut stream: Option<TcpStream> = None;

    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    RbnCommand::Connect(callsign) => {
                        // Disconnect existing connection first
                        stream = None;

                        let _ = msg_tx.send(RbnMessage::Status(
                            format!("Connecting to {}:{}...", RBN_HOST, RBN_PORT)
                        )).await;

                        match TcpStream::connect((RBN_HOST, RBN_PORT)).await {
                            Ok(s) => {
                                let _ = msg_tx.send(RbnMessage::Status(
                                    "Connected, waiting for login prompt...".to_string()
                                )).await;
                                stream = Some(s);

                                // Handle login in a separate block
                                if let Some(ref mut s) = stream {
                                    if let Err(e) = handle_login(s, &callsign, &msg_tx).await {
                                        let _ = msg_tx.send(RbnMessage::Status(
                                            format!("Login failed: {}", e)
                                        )).await;
                                        stream = None;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = msg_tx.send(RbnMessage::Status(
                                    format!("Connection failed: {}", e)
                                )).await;
                            }
                        }
                    }
                    RbnCommand::Disconnect => {
                        stream = None;
                        let _ = msg_tx.send(RbnMessage::Status("Disconnected".to_string())).await;
                        let _ = msg_tx.send(RbnMessage::Disconnected).await;
                    }
                }
            }
            _ = async {
                if let Some(ref mut s) = stream {
                    let mut reader = BufReader::new(s);
                    let mut line = String::new();
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            // Connection closed
                            let _ = msg_tx.send(RbnMessage::Status("Connection closed".to_string())).await;
                            let _ = msg_tx.send(RbnMessage::Disconnected).await;
                            return true; // Signal to clear stream
                        }
                        Ok(_) => {
                            if let Some(spot) = parse_spot_line(&line, &spot_regex) {
                                let _ = msg_tx.send(RbnMessage::Spot(spot)).await;
                            }
                        }
                        Err(e) => {
                            let _ = msg_tx.send(RbnMessage::Status(format!("Read error: {}", e))).await;
                            return true; // Signal to clear stream
                        }
                    }
                }
                false
            }, if stream.is_some() => {
                // Handle result - stream needs clearing handled above
            }
            else => {
                // No stream, just wait for commands
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_login(
    stream: &mut TcpStream,
    callsign: &str,
    msg_tx: &mpsc::Sender<RbnMessage>,
) -> Result<(), String> {
    let mut reader = BufReader::new(&mut *stream);
    let mut line = String::new();

    // Read until we get the login prompt
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => return Err("Connection closed".to_string()),
            Ok(_) => {
                if line.to_lowercase().contains("please enter your call") {
                    // Send callsign
                    stream
                        .write_all(format!("{}\r\n", callsign).as_bytes())
                        .await
                        .map_err(|e| format!("Failed to send callsign: {}", e))?;

                    let _ = msg_tx
                        .send(RbnMessage::Status(format!("Logged in as {}", callsign)))
                        .await;
                    return Ok(());
                }
            }
            Err(e) => return Err(format!("Read error: {}", e)),
        }
    }
}

fn parse_spot_line(line: &str, regex: &Regex) -> Option<RawSpot> {
    if !line.starts_with("DX de") {
        return None;
    }

    let caps = regex.captures(line)?;

    Some(RawSpot::new(
        caps.get(1)?.as_str().trim_end_matches(|c| c == '-' || c == '#' || c == ':').to_string(),
        caps.get(3)?.as_str().to_string(),
        caps.get(2)?.as_str().parse().ok()?,
        caps.get(5)?.as_str().parse().ok()?,
        caps.get(6)?.as_str().parse().ok()?,
        caps.get(4)?.as_str().to_string(),
    ))
}
