//! TCP 연결 래퍼 — 프레이밍(압축 포함)을 숨기고 패킷 단위 송수신을 제공.

use mc_proto::buf::PacketBuf;
use mc_proto::frame;
use mc_proto::State;
use std::io::{self, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    threshold: Option<usize>,
    pub state: State,
}

impl Connection {
    pub fn connect(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true).ok();
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            threshold: None,
            state: State::Handshaking,
        })
    }

    pub fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(dur)
    }

    /// Set Compression 수신 후 임계값을 설정. None = 비압축.
    pub fn set_threshold(&mut self, threshold: Option<usize>) {
        self.threshold = threshold;
    }
    pub fn threshold(&self) -> Option<usize> {
        self.threshold
    }

    /// 패킷 하나 전송 (id + body 바이트).
    pub fn send(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        frame::write_frame(&mut self.writer, id, body, self.threshold)?;
        self.writer.flush()
    }

    /// 패킷 하나 수신 → (id, body 버퍼).
    pub fn recv(&mut self) -> io::Result<(i32, PacketBuf)> {
        frame::read_frame(&mut self.reader, self.threshold)
    }
}
