use std::{
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
    thread,
};

/// Event received from the opponent during a race.
#[derive(Debug, PartialEq, Eq)]
pub enum RaceEvent {
    OpponentProgress(usize),
    OpponentFinished(usize),
    Disconnected(String),
}

/// Active TCP race connection used by the main event loop.
pub struct RaceSession {
    writer: TcpStream,
    events: Receiver<RaceEvent>,
}

impl RaceSession {
    /// Starts the background reader thread for an established race stream.
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let (sender, events) = mpsc::channel();

        thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            loop {
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
                        let Some(event) = parse_peer_event(&line) else {
                            let _ = sender.send(RaceEvent::Disconnected(format!(
                                "received invalid race message: {line}"
                            )));
                            break;
                        };
                        if sender.send(event).is_err() {
                            break;
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
        });

        Ok(Self {
            writer: stream,
            events,
        })
    }

    /// Sends the local completed-word index to the opponent.
    pub fn send_progress(&mut self, word_index: usize) -> io::Result<()> {
        writeln!(self.writer, "PROGRESS {word_index}")?;
        self.writer.flush()
    }

    /// Sends the local finish signal and final completed-word index.
    pub fn send_finish(&mut self, word_index: usize) -> io::Result<()> {
        writeln!(self.writer, "FINISH {word_index}")?;
        self.writer.flush()
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

/// Hosts a race, sends the generated words, and waits for one client.
pub fn host(addr: &str, words: &[String]) -> io::Result<RaceSession> {
    let listener = TcpListener::bind(addr)?;
    let (mut stream, _) = listener.accept()?;
    send_word_list(&mut stream, words)?;
    writeln!(stream, "START")?;
    stream.flush()?;
    RaceSession::new(stream)
}

/// Connects to a race host and receives the authoritative word list.
pub fn client(addr: &str) -> io::Result<(Vec<String>, RaceSession)> {
    let stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;

    let reader = stream.try_clone()?;
    let mut reader = BufReader::new(reader);
    let words = read_word_list(&mut reader)?;
    read_start(&mut reader)?;
    drop(reader);

    Ok((words, RaceSession::new(stream)?))
}

/// Writes the race word list in a simple newline protocol.
fn send_word_list(stream: &mut TcpStream, words: &[String]) -> io::Result<()> {
    writeln!(stream, "WORDS {}", words.len())?;
    for word in words {
        writeln!(stream, "{word}")?;
    }
    stream.flush()
}

/// Reads the host-provided word list from the race protocol.
fn read_word_list<R: BufRead>(reader: &mut R) -> io::Result<Vec<String>> {
    let mut header = String::new();
    if reader.read_line(&mut header)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "race host closed before sending words",
        ));
    }
    trim_protocol_line(&mut header);

    let count = header
        .strip_prefix("WORDS ")
        .and_then(|count| count.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid race word header"))?;

    let mut words = Vec::with_capacity(count);
    for _ in 0..count {
        let mut word = String::new();
        if reader.read_line(&mut word)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "race host closed before sending all words",
            ));
        }
        trim_protocol_line(&mut word);
        words.push(word);
    }

    Ok(words)
}

/// Reads the start signal that releases both players into the race.
fn read_start<R: BufRead>(reader: &mut R) -> io::Result<()> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "race host closed before sending start",
        ));
    }
    trim_protocol_line(&mut line);

    if line == "START" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid race start signal",
        ))
    }
}

/// Parses an opponent progress or finish message.
fn parse_peer_event(line: &str) -> Option<RaceEvent> {
    let (kind, value) = line.split_once(' ')?;
    let progress = value.parse::<usize>().ok()?;

    match kind {
        "PROGRESS" => Some(RaceEvent::OpponentProgress(progress)),
        "FINISH" => Some(RaceEvent::OpponentFinished(progress)),
        _ => None,
    }
}

/// Removes protocol line endings without trimming word content.
fn trim_protocol_line(line: &mut String) {
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_peer_progress_messages() {
        assert_eq!(
            parse_peer_event("PROGRESS 12"),
            Some(RaceEvent::OpponentProgress(12))
        );
        assert_eq!(
            parse_peer_event("FINISH 50"),
            Some(RaceEvent::OpponentFinished(50))
        );
        assert_eq!(parse_peer_event("BAD 50"), None);
    }

    #[test]
    fn reads_word_list_without_trimming_word_spaces() {
        let data = b"WORDS 2\r\nhello world \nSTART\nSTART\n";
        let mut reader = Cursor::new(data);

        let words = read_word_list(&mut reader).unwrap();
        read_start(&mut reader).unwrap();

        assert_eq!(words, vec!["hello world ", "START"]);
    }
}
