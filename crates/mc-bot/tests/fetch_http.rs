//! Downloads against a local one-shot HTTP server: limits, errors and web pages.

use mc_bot::fetch::{self, fetch_limited};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

/// Serve one canned response after `delay`; returns the base URL.
fn serve(response: Vec<u8>, delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let _ = s.read(&mut [0u8; 4096]);
        thread::sleep(delay);
        let _ = s.write_all(&response);
    });
    url
}

fn ok(body: &str, headers: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{headers}\r\n{body}").into_bytes()
}

fn error(url: &str, max: u64, timeout: Duration) -> String {
    fetch_limited(url, max, timeout).err().expect("must fail")
}

const T: Duration = Duration::from_secs(5);

#[test]
fn downloads_and_parses_ignoring_the_query_string() {
    let url = serve(ok("palette S stone\nlayer 0\nSS\n", ""), Duration::ZERO);
    let src = fetch_limited(&format!("{url}/a/wall.txt?token=1"), 1000, T).unwrap();
    assert_eq!(src.name, "wall.txt");
    assert_eq!(fetch::parse(&src).unwrap().blocks.len(), 2);
}

#[test]
fn size_limit_applies_with_and_without_content_length() {
    let big = "x".repeat(2000);
    for headers in ["Content-Length: 2000\r\n", ""] {
        let url = serve(ok(&big, headers), Duration::ZERO);
        assert!(error(&format!("{url}/a.schem"), 1000, T).contains("limit"), "{headers}");
    }
}

#[test]
fn a_silent_server_times_out() {
    let url = serve(ok("x", ""), Duration::from_secs(20));
    let started = Instant::now();
    assert!(error(&format!("{url}/a.schem"), 1000, Duration::from_millis(500)).contains("download failed"));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn web_pages_http_errors_and_other_schemes_are_explained() {
    let url = serve(ok("\n<!DOCTYPE html><html></html>", ""), Duration::ZERO);
    assert!(error(&format!("{url}/schematic/123"), 1000, T).contains("web page"));
    let url = serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(), Duration::ZERO);
    assert!(error(&format!("{url}/a.schem"), 1000, T).contains("404"));
    for bad in ["file:///etc/passwd", "ftp://example.com/a.schem"] {
        assert!(error(bad, 1000, T).contains("only http"), "{bad}");
    }
}
