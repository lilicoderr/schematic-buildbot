//! Drive a real `build-bot ui` process with hand-written HTTP requests (so that tests can send
//! what a browser never would).

use mc_bot::json::{self, Json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Ui {
    child: Child,
    pub port: u16,
    pub token: String,
    /// The folder the UI runs in; its state folder is `<dir>/state`.
    pub dir: PathBuf,
}

impl Ui {
    pub fn start(test: &str) -> Ui {
        let dir = std::env::temp_dir().join(format!("bb-ui-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_build-bot"))
            .current_dir(&dir)
            .args(["ui", "--no-open", "--lang", "en", "--state-dir", "state"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        let url = line.split("http://127.0.0.1:").nth(1).expect(&line).trim();
        let (port, token) = url.split_once("/#token=").expect(&line);
        // keep draining, so that the UI never blocks on a full pipe
        std::thread::spawn(move || std::io::copy(&mut out, &mut std::io::sink()));
        Ui { child, port: port.parse().unwrap(), token: token.to_string(), dir }
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Send exactly these bytes: (status, headers and body).
    pub fn raw(&self, request: &[u8]) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        s.write_all(request).unwrap();
        let mut reply = Vec::new();
        let _ = s.read_to_end(&mut reply);
        let text = String::from_utf8_lossy(&reply).into_owned();
        let status = text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, text)
    }

    /// A request with exactly these headers (plus `Content-Length` when there is a body).
    pub fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> (u16, String) {
        let mut head = format!("{method} {path} HTTP/1.1\r\n");
        for (k, v) in headers {
            head += &format!("{k}: {v}\r\n");
        }
        if !body.is_empty() {
            head += &format!("Content-Length: {}\r\n", body.len());
        }
        let mut bytes = (head + "\r\n").into_bytes();
        bytes.extend_from_slice(body);
        self.raw(&bytes)
    }

    /// What the page itself sends: right host, origin and token.
    pub fn api(&self, method: &str, path: &str, body: &[u8]) -> (u16, Json) {
        let (host, origin) = (self.host(), format!("http://{}", self.host()));
        let mut headers = vec![("Host", host.as_str()), ("X-BuildBot-Token", self.token.as_str())];
        if method == "POST" {
            headers.push(("Origin", &origin));
            headers.push(("Content-Type", "application/json"));
        }
        let (status, text) = self.send(method, path, &headers, body);
        let body = text.split_once("\r\n\r\n").map_or("", |x| x.1);
        (status, json::parse(body).unwrap_or(Json::Raw(String::new())))
    }

    /// Poll the status until `done` says so.
    pub fn wait(&self, what: &str, done: impl Fn(&Json) -> bool) -> Json {
        let end = Instant::now() + Duration::from_secs(90);
        loop {
            let (status, j) = self.api("GET", "/api/status", b"");
            assert_eq!(status, 200);
            if done(&j) {
                return j;
            }
            assert!(Instant::now() < end, "timed out waiting for {what}: {j:?}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// End the UI process abruptly, like closing its window.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        self.kill();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn text<'a>(j: &'a Json, key: &str) -> &'a str {
    match j.get(key) {
        Some(Json::Str(s)) | Some(Json::Raw(s)) => s,
        _ => "",
    }
}
