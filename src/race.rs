use crate::tunnel::{self, BoreTunnel};

use rand::Rng;
use std::{
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(3);
const LOCAL_RACE_PORT: u16 = 7878;
const PUBLIC_TUNNEL_HOST: &str = "bore.pub";

/// Event received from the opponent during a race.
#[derive(Debug, PartialEq)]
pub enum RaceEvent {
    OpponentProgress(usize),
    OpponentFinished { wpm: f64, accuracy: f64 },
    Disconnected(String),
    SyncWords(Vec<String>),
    Start,
}

/// Result produced by the host lobby background accept loop.
pub enum LobbyEvent {
    OpponentConnected(RaceSession),
    Cancelled,
    Failed(String),
}

/// Parsed race join argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RaceInvite {
    pub addr: String,
    pub room_code: String,
}

/// Host-side lobby data and resources.
pub struct HostLobby {
    room_code: String,
    public_addr: String,
    invite_command: String,
    receiver: Receiver<LobbyEvent>,
    cancel: Arc<AtomicBool>,
    tunnel: Option<BoreTunnel>,
}

impl HostLobby {
    /// Creates a hosted lobby using a public bore tunnel.
    pub fn start(bind_addr: &str, words: Vec<String>) -> io::Result<Self> {
        let listener = TcpListener::bind(bind_addr)?;
        listener.set_nonblocking(true)?;

        let room_code = generate_room_code();
        let bore_path = tunnel::ensure_bore_installed()?;
        let tunnel = tunnel::spawn_bore(&bore_path, LOCAL_RACE_PORT, PUBLIC_TUNNEL_HOST)?;
        let public_addr = tunnel.public_addr().to_string();
        let invite_command = format!("ttyper --race {public_addr}#{room_code}");
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();

        spawn_host_accept_loop(
            listener,
            room_code.clone(),
            words,
            Arc::clone(&cancel),
            sender,
        );

        Ok(Self {
            room_code,
            public_addr,
            invite_command,
            receiver,
            cancel,
            tunnel: Some(tunnel),
        })
    }

    /// Creates a local-only hosted lobby (no bore tunnel required).
    /// The invite command uses the machine's LAN IP so friends on the same
    /// network can join with `ttyper --race <LAN_IP>:7878#CODE`.
    pub fn start_local(words: Vec<String>) -> io::Result<Self> {
        let bind_addr = format!("0.0.0.0:{LOCAL_RACE_PORT}");
        let listener = TcpListener::bind(&bind_addr)?;
        listener.set_nonblocking(true)?;

        let room_code = generate_room_code();
        let local_ip = detect_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
        let public_addr = format!("{local_ip}:{LOCAL_RACE_PORT}");
        let invite_command = format!("ttyper --race {public_addr}#{room_code}");
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();

        spawn_host_accept_loop(
            listener,
            room_code.clone(),
            words,
            Arc::clone(&cancel),
            sender,
        );

        Ok(Self {
            room_code,
            public_addr,
            invite_command,
            receiver,
            cancel,
            tunnel: None,
        })
    }

    pub fn room_code(&self) -> &str {
        &self.room_code
    }

    pub fn public_addr(&self) -> &str {
        &self.public_addr
    }

    pub fn invite_command(&self) -> &str {
        &self.invite_command
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn poll(&mut self) -> Option<LobbyEvent> {
        self.receiver.try_recv().ok()
    }

    pub fn take_tunnel(&mut self) -> Option<BoreTunnel> {
        self.tunnel.take()
    }
}

impl Drop for HostLobby {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Active TCP race connection used by the main event loop.
pub struct RaceSession {
    writer: Arc<Mutex<TcpStream>>,
    events: Receiver<RaceEvent>,
    closed: Arc<AtomicBool>,
    tunnel: Option<BoreTunnel>,
}

impl RaceSession {
    /// Starts reader and heartbeat threads for an established race stream.
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let writer = Arc::new(Mutex::new(stream));
        let (sender, events) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let last_pong = Arc::new(Mutex::new(Instant::now()));

        spawn_reader_thread(
            reader,
            Arc::clone(&writer),
            sender.clone(),
            Arc::clone(&closed),
            Arc::clone(&last_pong),
        );
        spawn_heartbeat_thread(writer.clone(), sender, Arc::clone(&closed), last_pong);

        Ok(Self {
            writer,
            events,
            closed,
            tunnel: None,
        })
    }

    pub fn keep_tunnel(&mut self, tunnel: BoreTunnel) {
        self.tunnel = Some(tunnel);
    }

    /// Sends a new word list to the opponent.
    pub fn send_words(&mut self, words: &[String]) -> io::Result<()> {
        let encoded = serde_json::to_string(words)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        write_protocol_line(&self.writer, &format!("WORDS {encoded}"))
    }

    /// Sends the start signal to the opponent.
    pub fn send_start(&mut self) -> io::Result<()> {
        write_protocol_line(&self.writer, "START")
    }

    /// Sends the local completed-word index to the opponent.
    pub fn send_progress(&mut self, word_index: usize) -> io::Result<()> {
        write_protocol_line(&self.writer, &format!("PROGRESS {word_index}"))
    }

    /// Sends the local finish signal and final metrics.
    pub fn send_finish(&mut self, wpm: f64, accuracy: f64) -> io::Result<()> {
        write_protocol_line(&self.writer, &format!("FINISH {wpm:.2} {accuracy:.2}"))
    }

    /// Drains all queued network events without blocking the UI loop.
    pub fn drain_events(&mut self) -> Vec<RaceEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for RaceSession {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// Connects to a race host and enters the lobby.
pub fn client(invite: &RaceInvite) -> io::Result<(Vec<String>, RaceSession)> {
    let mut stream = TcpStream::connect(&invite.addr)?;
    stream.set_nodelay(true)?;
    writeln!(stream, "ROOM {}", invite.room_code)?;
    stream.flush()?;

    let reader = stream.try_clone()?;
    let mut reader = BufReader::new(reader);
    let mut status = String::new();
    if reader.read_line(&mut status)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "race host closed before accepting room code",
        ));
    }
    trim_protocol_line(&mut status);

    match status.as_str() {
        "OK" => {}
        "WRONG" => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Wrong room code. Connection refused.",
            ));
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid race room response",
            ));
        }
    }

    let words = read_word_list(&mut reader)?;
    read_start(&mut reader)?;

    Ok((words, RaceSession::new(stream)?))
}

fn read_word_list<R: BufRead>(reader: &mut R) -> io::Result<Vec<String>> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "race host closed before sending words",
            ));
        }
        trim_protocol_line(&mut line);
        if line == "PING" {
            continue;
        }
        if let Some(encoded) = line.strip_prefix("WORDS ") {
            return serde_json::from_str(encoded)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid race word list",
        ));
    }
}

fn read_start<R: BufRead>(reader: &mut R) -> io::Result<()> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "race host closed before sending start signal",
            ));
        }
        trim_protocol_line(&mut line);
        if line == "PING" {
            continue;
        }
        if line == "START" {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid race start signal",
        ));
    }
}

/// Parses a race invite from several accepted formats:
///   - Full CLI command: `ttyper --race 192.168.1.5:7878#1234`
///   - Address + code:   `192.168.1.5:7878#1234`
///   - IP + code:        `192.168.1.5#1234`  (port defaults to 7878)
pub fn parse_invite(input: &str) -> io::Result<RaceInvite> {
    // Strip the CLI prefix if the user pasted the full copied command.
    let input = input
        .trim()
        .strip_prefix("ttyper --race ")
        .unwrap_or(input)
        .trim();

    let (addr, room_code) = input.split_once('#').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "race address must look like 192.168.1.5:7878#1234",
        )
    })?;

    let addr = addr.trim();
    if addr.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "race address host is empty",
        ));
    }
    if !is_valid_room_code(room_code) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "race room code must be exactly 4 digits",
        ));
    }

    // Default port to 7878 if the user only gave an IP without a port.
    let addr = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{addr}:{LOCAL_RACE_PORT}")
    };

    Ok(RaceInvite {
        addr,
        room_code: room_code.to_string(),
    })
}

fn spawn_host_accept_loop(
    listener: TcpListener,
    room_code: String,
    words: Vec<String>,
    cancel: Arc<AtomicBool>,
    sender: mpsc::Sender<LobbyEvent>,
) {
    thread::spawn(move || loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = sender.send(LobbyEvent::Cancelled);
            return;
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                // On Windows the accepted socket inherits the listener's non-blocking
                // flag, so read_line() in the handshake would immediately return
                // WouldBlock (os error 10035). Reset to blocking here; the handshake
                // runs in this background thread so blocking is safe.
                let _ = stream.set_nonblocking(false);
                match handle_lobby_client(&mut stream, &room_code, &words) {
                    Ok(true) => match RaceSession::new(stream) {
                        Ok(session) => {
                            let _ = sender.send(LobbyEvent::OpponentConnected(session));
                            return;
                        }
                        Err(error) => {
                            let _ = sender.send(LobbyEvent::Failed(format!(
                                "could not start race session: {error}"
                            )));
                            return;
                        }
                    },
                    // Wrong room code or empty read — ignore, keep waiting.
                    Ok(false) => {}
                    // Bad data (e.g. non-UTF-8, browser probe) — ignore, keep waiting.
                    Err(_) => {}
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                let _ = sender.send(LobbyEvent::Failed(format!(
                    "could not accept race opponent: {error}"
                )));
                return;
            }
        }
    });
}

fn handle_lobby_client(
    stream: &mut TcpStream,
    room_code: &str,
    words: &[String],
) -> io::Result<bool> {
    stream.set_nodelay(true)?;
    let reader = stream.try_clone()?;
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(false);
    }
    trim_protocol_line(&mut line);

    if line != format!("ROOM {room_code}") {
        writeln!(stream, "WRONG")?;
        stream.flush()?;
        return Ok(false);
    }

    writeln!(stream, "OK")?;

    let encoded = serde_json::to_string(words)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writeln!(stream, "WORDS {encoded}")?;
    writeln!(stream, "START")?;

    stream.flush()?;
    Ok(true)
}

fn spawn_reader_thread(
    reader: TcpStream,
    writer: Arc<Mutex<TcpStream>>,
    sender: mpsc::Sender<RaceEvent>,
    closed: Arc<AtomicBool>,
    last_pong: Arc<Mutex<Instant>>,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        loop {
            if closed.load(Ordering::SeqCst) {
                break;
            }

            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = sender.send(RaceEvent::Disconnected(
                        "opponent disconnected from the race".into(),
                    ));
                    break;
                }
                Ok(_) => {
                    trim_protocol_line(&mut line);
                    match parse_peer_message(&line) {
                        Some(PeerMessage::Event(event)) => {
                            if sender.send(event).is_err() {
                                break;
                            }
                        }
                        Some(PeerMessage::Ping) => {
                            let _ = write_protocol_line(&writer, "PONG");
                        }
                        Some(PeerMessage::Pong) => {
                            if let Ok(mut last_pong) = last_pong.lock() {
                                *last_pong = Instant::now();
                            }
                        }
                        None => {
                            let _ = sender.send(RaceEvent::Disconnected(format!(
                                "received invalid race message: {line}"
                            )));
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(RaceEvent::Disconnected(format!(
                        "race connection failed: {error}"
                    )));
                    break;
                }
            }
        }
        closed.store(true, Ordering::SeqCst);
    });
}

fn spawn_heartbeat_thread(
    writer: Arc<Mutex<TcpStream>>,
    sender: mpsc::Sender<RaceEvent>,
    closed: Arc<AtomicBool>,
    last_pong: Arc<Mutex<Instant>>,
) {
    thread::spawn(move || loop {
        thread::sleep(HEARTBEAT_INTERVAL);
        if closed.load(Ordering::SeqCst) {
            break;
        }

        let timed_out = last_pong
            .lock()
            .map(|last_pong| last_pong.elapsed() > HEARTBEAT_TIMEOUT)
            .unwrap_or(true);
        if timed_out {
            let _ = sender.send(RaceEvent::Disconnected(
                "opponent heartbeat timed out".into(),
            ));
            closed.store(true, Ordering::SeqCst);
            break;
        }

        if let Err(error) = write_protocol_line(&writer, "PING") {
            let _ = sender.send(RaceEvent::Disconnected(format!(
                "race heartbeat failed: {error}"
            )));
            closed.store(true, Ordering::SeqCst);
            break;
        }
    });
}

enum PeerMessage {
    Event(RaceEvent),
    Ping,
    Pong,
}

/// Parses an opponent progress, finish, or heartbeat message.
fn parse_peer_message(line: &str) -> Option<PeerMessage> {
    if line == "PING" {
        return Some(PeerMessage::Ping);
    }
    if line == "PONG" {
        return Some(PeerMessage::Pong);
    }
    if line == "START" {
        return Some(PeerMessage::Event(RaceEvent::Start));
    }
    if let Some(encoded) = line.strip_prefix("WORDS ") {
        let words: Vec<String> = serde_json::from_str(encoded).ok()?;
        return Some(PeerMessage::Event(RaceEvent::SyncWords(words)));
    }

    let (kind, value) = line.split_once(' ')?;
    match kind {
        "PROGRESS" => Some(PeerMessage::Event(RaceEvent::OpponentProgress(
            value.parse::<usize>().ok()?,
        ))),
        "FINISH" => {
            let mut parts = value.split_whitespace();
            let wpm = parts.next()?.parse::<f64>().ok()?;
            let accuracy = parts.next()?.parse::<f64>().ok()?;
            Some(PeerMessage::Event(RaceEvent::OpponentFinished {
                wpm,
                accuracy,
            }))
        }
        _ => None,
    }
}

fn write_protocol_line(writer: &Arc<Mutex<TcpStream>>, line: &str) -> io::Result<()> {
    let mut writer = writer
        .lock()
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "race writer lock poisoned"))?;
    writeln!(writer, "{line}")?;
    writer.flush()
}

/// Removes protocol line endings without trimming word content.
fn trim_protocol_line(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

fn generate_room_code() -> String {
    format!("{:04}", rand::thread_rng().gen_range(0..10_000))
}

fn is_valid_room_code(code: &str) -> bool {
    code.len() == 4 && code.chars().all(|character| character.is_ascii_digit())
}

/// Detects the machine's primary LAN IP address by connecting a UDP socket
/// to an external address (no packets are actually sent).
fn detect_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket
        .local_addr()
        .ok()
        .map(|addr| addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::Duration;

    #[test]
    fn parses_peer_progress_messages() {
        assert!(matches!(
            parse_peer_message("PROGRESS 12"),
            Some(PeerMessage::Event(RaceEvent::OpponentProgress(12)))
        ));
        assert!(matches!(
            parse_peer_message("FINISH 87.50 98.25"),
            Some(PeerMessage::Event(RaceEvent::OpponentFinished {
                wpm: 87.5,
                accuracy: 98.25,
            }))
        ));
        assert!(matches!(
            parse_peer_message("PING"),
            Some(PeerMessage::Ping)
        ));
        assert!(matches!(
            parse_peer_message("PONG"),
            Some(PeerMessage::Pong)
        ));
        assert!(parse_peer_message("BAD 50").is_none());
    }

    #[test]
    fn reads_json_word_list_without_trimming_word_spaces() {
        let data = b"WORDS [\"hello world \",\"START\"]\nSTART\n";
        let mut reader = Cursor::new(data);

        let words = read_word_list(&mut reader).unwrap();
        read_start(&mut reader).unwrap();

        assert_eq!(words, vec!["hello world ", "START"]);
    }

    #[test]
    fn parses_race_invite_with_room_code() {
        assert_eq!(
            parse_invite("bore.pub:43821#4829").unwrap(),
            RaceInvite {
                addr: "bore.pub:43821".into(),
                room_code: "4829".into(),
            }
        );
        assert!(parse_invite("bore.pub:43821").is_err());
        assert!(parse_invite("bore.pub:43821#abcd").is_err());
    }

    #[test]
    fn room_code_is_four_digits() {
        let code = generate_room_code();

        assert!(is_valid_room_code(&code));
    }

    #[test]
    fn local_room_code_handshake_syncs_words_and_progress() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let words = vec!["the".into(), "quick".into(), "brown".into()];

        spawn_host_accept_loop(
            listener,
            "4829".into(),
            words.clone(),
            Arc::clone(&cancel),
            sender,
        );

        let invite = RaceInvite {
            addr: addr.to_string(),
            room_code: "4829".into(),
        };
        let (client_words, mut client_session) = client(&invite).unwrap();
        assert_eq!(client_words, words);

        let mut host_session = match receiver.recv_timeout(Duration::from_secs(1)).unwrap() {
            LobbyEvent::OpponentConnected(session) => session,
            LobbyEvent::Cancelled => panic!("lobby cancelled unexpectedly"),
            LobbyEvent::Failed(message) => panic!("{message}"),
        };

        client_session.send_progress(1).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(host_session
            .drain_events()
            .contains(&RaceEvent::OpponentProgress(1)));

        cancel.store(true, Ordering::SeqCst);
    }
}
