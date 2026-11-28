//! TCP connection wrapper: hides framing (incl. compression) and exposes packet-level send/receive.

use mc_proto::buf::PacketBuf;
use mc_proto::frame;
use mc_proto::State;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// No TCP connection could be made at all (unknown host, refused, timed out).
#[derive(Debug)]
struct ConnectError(io::Error);

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for ConnectError {}

pub fn is_connect_error(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<ConnectError>())
}

fn open(addr: &str) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "the host name has no address");
    for a in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

pub struct Connection {
    reader: TcpStream,
    /// Bytes received but not yet parsed; a frame split by a read timeout stays here intact.
    inbox: Vec<u8>,
    writer: TcpStream,
    threshold: Option<usize>,
    pub state: State,
}

impl Connection {
    pub fn connect(addr: &str) -> io::Result<Self> {
        let stream = open(addr).map_err(|e| io::Error::new(e.kind(), ConnectError(e)))?;
        stream.set_nodelay(true).ok();
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: stream,
            inbox: Vec::new(),
            writer,
            threshold: None,
            state: State::Handshaking,
        })
    }

    pub fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.reader.set_read_timeout(dur)
    }

    /// Set the compression threshold after Set Compression. None = uncompressed.
    pub fn set_threshold(&mut self, threshold: Option<usize>) {
        self.threshold = threshold;
    }
    pub fn threshold(&self) -> Option<usize> {
        self.threshold
    }

    /// Send one packet (id + body bytes).
    pub fn send(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        frame::write_frame(&mut self.writer, id, body, self.threshold)?;
        self.writer.flush()
    }

    /// Receive one packet → (id, body buffer). A timeout keeps any partial frame for the next call.
    pub fn recv(&mut self) -> io::Result<(i32, PacketBuf)> {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if let Some((used, id, body)) = frame::try_parse_frame(&self.inbox, self.threshold)? {
                self.inbox.drain(..used);
                return Ok((id, body));
            }
            let n = self.reader.read(&mut chunk)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed"));
            }
            self.inbox.extend_from_slice(&chunk[..n]);
        }
    }
}
