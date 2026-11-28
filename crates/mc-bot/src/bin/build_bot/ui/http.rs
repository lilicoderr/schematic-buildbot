//! Just enough HTTP/1.1 for one local page: one request per connection, bounded headers, a
//! `Content-Length` body (no chunked uploads) and a deadline for reading it.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

const MAX_HEAD: usize = 16 << 10;
const MAX_HEADERS: usize = 64;
/// Longest silence on the socket.
const IDLE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status(pub u16, pub &'static str);

impl Status {
    pub const OK: Status = Status(200, "OK");
    pub const BAD_REQUEST: Status = Status(400, "Bad Request");
    pub const UNAUTHORIZED: Status = Status(401, "Unauthorized");
    pub const FORBIDDEN: Status = Status(403, "Forbidden");
    pub const NOT_FOUND: Status = Status(404, "Not Found");
    pub const METHOD: Status = Status(405, "Method Not Allowed");
    pub const CONFLICT: Status = Status(409, "Conflict");
    pub const LENGTH_REQUIRED: Status = Status(411, "Length Required");
    pub const TOO_LARGE: Status = Status(413, "Payload Too Large");
    pub const UNPROCESSABLE: Status = Status(422, "Unprocessable Entity");
    pub const BUSY: Status = Status(503, "Service Unavailable");
}

#[derive(Debug)]
pub struct Request {
    pub method: String,
    /// Path without the query, exactly as sent (never decoded, never used as a file name).
    pub path: String,
    query: String,
    /// Names in lower case.
    headers: Vec<(String, String)>,
    /// Body bytes that arrived together with the headers.
    early: Vec<u8>,
}

impl Request {
    pub fn headers_named(&self, name: &str) -> Vec<&str> {
        self.headers.iter().filter(|h| h.0 == name).map(|h| h.1.as_str()).collect()
    }

    /// The header's value if it occurs exactly once.
    pub fn single(&self, name: &str) -> Option<&str> {
        match self.headers_named(name).as_slice() {
            [v] => Some(v),
            _ => None,
        }
    }

    /// A percent-decoded query parameter.
    pub fn param(&self, key: &str) -> Option<String> {
        let pair = self.query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == key)?;
        percent_decode(pair.1)
    }

    pub fn content_length(&self) -> Result<u64, Status> {
        if !self.headers_named("transfer-encoding").is_empty() {
            return Err(Status::LENGTH_REQUIRED);
        }
        let v = match self.headers_named("content-length").as_slice() {
            [v] => *v,
            [] => return Err(Status::LENGTH_REQUIRED),
            _ => return Err(Status::BAD_REQUEST),
        };
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Status::BAD_REQUEST);
        }
        v.parse().map_err(|_| Status::TOO_LARGE)
    }

    /// Read the body, refusing it before the first byte if it is declared larger than `max`.
    pub fn body(&mut self, stream: &mut TcpStream, max: u64, limit: Duration) -> Result<Vec<u8>, Status> {
        let len = self.content_length()?;
        if len > max {
            return Err(Status::TOO_LARGE);
        }
        let deadline = Instant::now() + limit;
        let mut body = std::mem::take(&mut self.early);
        body.truncate(len as usize);
        let mut chunk = [0u8; 64 << 10];
        while (body.len() as u64) < len {
            let want = (len as usize - body.len()).min(chunk.len());
            match stream.read(&mut chunk[..want]) {
                Ok(n) if n > 0 && Instant::now() < deadline => body.extend_from_slice(&chunk[..n]),
                _ => return Err(Status::BAD_REQUEST),
            }
        }
        Ok(body)
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Parse the request line and headers (`head` ends before the empty line).
pub fn parse_head(head: &str, early: Vec<u8>) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, proto) = (first.next()?, first.next()?, first.next()?);
    if !proto.starts_with("HTTP/1.") || !target.starts_with('/') || first.next().is_some() {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    for line in lines {
        let (k, v) = line.split_once(':')?;
        if k.is_empty() || k.contains([' ', '\t']) || line.contains(['\r', '\n']) || headers.len() >= MAX_HEADERS {
            return None;
        }
        headers.push((k.to_ascii_lowercase(), v.trim().to_string()));
    }
    Some(Request { method: method.into(), path: path.into(), query: query.into(), headers, early })
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, Status> {
    stream.set_read_timeout(Some(IDLE)).map_err(|_| Status::BAD_REQUEST)?;
    stream.set_write_timeout(Some(IDLE * 3)).map_err(|_| Status::BAD_REQUEST)?;
    let deadline = Instant::now() + IDLE;
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 2048]);
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let early = buf.split_off(end + 4);
            let head = std::str::from_utf8(&buf[..end]).map_err(|_| Status::BAD_REQUEST)?;
            return parse_head(head, early).ok_or(Status::BAD_REQUEST);
        }
        match stream.read(&mut chunk) {
            Ok(n) if n > 0 && buf.len() + n <= MAX_HEAD && Instant::now() < deadline => buf.extend_from_slice(&chunk[..n]),
            _ => return Err(Status::BAD_REQUEST),
        }
    }
}

/// Sent with every answer. The page may load and talk to nothing but this server.
const SECURITY_HEADERS: &str = "Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\n\
X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n\
Cross-Origin-Resource-Policy: same-origin\r\nCross-Origin-Opener-Policy: same-origin\r\nCache-Control: no-store\r\n";

pub fn respond(stream: &mut TcpStream, status: Status, content_type: &str, body: &[u8]) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{SECURITY_HEADERS}\r\n",
        status.0, status.1, body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// Close politely: unread input (a refused upload) must not reset the connection before the
/// browser has read the answer.
pub fn close(mut stream: TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let (deadline, mut rest, mut chunk) = (Instant::now() + Duration::from_secs(5), 64u64 << 20, [0u8; 64 << 10]);
    while let Ok(n) = stream.read(&mut chunk) {
        rest = rest.saturating_sub(n as u64);
        if n == 0 || rest == 0 || Instant::now() > deadline {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_and_queries() {
        let r = parse_head("POST /api/upload?name=a%20b.txt HTTP/1.1\r\nHost: h:1\r\nContent-Length: 5", Vec::new()).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.param("name").as_deref()), ("POST", "/api/upload", Some("a b.txt")));
        assert_eq!(r.content_length(), Ok(5));
        for bad in ["GET http://x/ HTTP/1.1", "GET / HTTP/2 x", "GET /", "GET / HTTP/1.1\r\nbad header", "GET / HTTP/1.1\r\nHost : x"] {
            assert!(parse_head(bad, Vec::new()).is_none(), "{bad}");
        }
    }

    #[test]
    fn ambiguous_or_chunked_lengths_are_refused() {
        let len = |h: &str| parse_head(&format!("POST / HTTP/1.1\r\n{h}"), Vec::new()).unwrap().content_length();
        assert_eq!(len("Content-Length: 1\r\nContent-Length: 2"), Err(Status::BAD_REQUEST));
        assert_eq!(len("Transfer-Encoding: chunked"), Err(Status::LENGTH_REQUIRED));
        assert_eq!(len("Content-Length: +4"), Err(Status::BAD_REQUEST));
        assert_eq!(len("Content-Length: 99999999999999999999999"), Err(Status::TOO_LARGE));
        assert_eq!(len("Host: x"), Err(Status::LENGTH_REQUIRED));
    }
}
