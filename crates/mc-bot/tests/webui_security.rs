//! Attacks on `build-bot ui`: wrong or missing token, foreign origin and host (DNS rebinding),
//! path traversal, oversized and malformed uploads, argument injection, other addresses.

#[allow(dead_code)]
mod uiclient;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;
use uiclient::{text, Ui};

const POSTS: [&str; 4] = ["/api/upload?name=a.txt", "/api/url", "/api/build", "/api/stop"];
const BLUEPRINT: &[u8] = b"palette S stone\nlayer 0\nSS\n";

#[test]
fn the_token_is_required_and_checked_whole() {
    let ui = Ui::start("token");
    let (host, origin) = (ui.host(), format!("http://{}", ui.host()));
    let almost = &ui.token[..63];
    let longer = format!("{}0", ui.token);
    for token in [None, Some(""), Some("0"), Some(almost), Some(longer.as_str())] {
        for path in POSTS.iter().chain(&["/api/status", "/api/info"]) {
            let method = if POSTS.contains(path) { "POST" } else { "GET" };
            let mut headers = vec![("Host", host.as_str()), ("Origin", origin.as_str()), ("Content-Type", "application/json")];
            headers.extend(token.map(|t| ("X-BuildBot-Token", t)));
            let (status, reply) = ui.send(method, path, &headers, b"{}");
            assert_eq!(status, 401, "{method} {path} {token:?}: {reply}");
            assert!(!reply.contains(&ui.token));
        }
    }
    // the token in the query string is not accepted instead
    let (status, _) = ui.send("GET", &format!("/api/status?token={}", ui.token), &[("Host", &host)], b"");
    assert_eq!(status, 401);
    assert_eq!(ui.api("GET", "/api/status", b"").0, 200);
}

#[test]
fn foreign_origins_and_hosts_are_refused_even_with_the_token() {
    let ui = Ui::start("origin");
    let (host, good) = (ui.host(), format!("http://{}", ui.host()));
    let other_port = format!("http://127.0.0.1:{}", ui.port.wrapping_add(1));
    let token = ("X-BuildBot-Token", ui.token.as_str());
    let https = format!("https://{host}");
    let suffix = format!("{good}.evil.example");
    let localhost = format!("http://localhost:{}", ui.port);
    // `localhost` is only right when the page was loaded from localhost
    for origin in ["http://evil.example", "null", &other_port, &https, &suffix, &localhost] {
        for path in POSTS {
            let (status, reply) = ui.send("POST", path, &[("Host", &host), ("Origin", origin), token], b"{}");
            assert_eq!(status, 403, "{origin} {path}: {reply}");
        }
    }
    let (status, _) = ui.send("POST", "/api/stop", &[("Host", &host), token], b"{}");
    assert_eq!(status, 403, "a state change without an Origin");
    let (status, _) = ui.send("POST", "/api/stop", &[("Host", &host), ("Origin", &good), ("Sec-Fetch-Site", "cross-site"), token], b"{}");
    assert_eq!(status, 403);
    // DNS rebinding: a foreign name that resolves here, with an origin that matches that name
    let rebound = format!("evil.example:{}", ui.port);
    let rebound_origin = format!("http://{rebound}");
    for h in [rebound.as_str(), "127.0.0.1", "localhost", "127.0.0.1:1", "[::1]:80", ""] {
        for path in ["/", "/ui.js", "/api/status"] {
            let (status, _) = ui.send("GET", path, &[("Host", h), token], b"");
            assert_eq!(status, 403, "Host {h:?} {path}");
        }
        let (status, _) = ui.send("POST", "/api/stop", &[("Host", h), ("Origin", &rebound_origin), token], b"{}");
        assert_eq!(status, 403);
    }
    assert_eq!(ui.send("GET", "/", &[], b"").0, 403, "no Host at all");
    assert_eq!(ui.send("GET", "/", &[("Host", &host), ("Host", &rebound)], b"").0, 403, "two Hosts");
    // no cross-origin preflight succeeds and nothing invites other origins
    let (status, reply) = ui.send("OPTIONS", "/api/build", &[("Host", &host), ("Origin", "http://evil.example")], b"");
    assert!(status == 403 && !reply.contains("Access-Control"), "{reply}");
    let (status, page) = ui.send("GET", "/", &[("Host", &host)], b"");
    assert!(status == 200 && page.contains("frame-ancestors 'none'") && page.contains("script-src 'self';") && !page.contains(&ui.token));
}

#[test]
fn nothing_outside_the_fixed_files_can_be_read_or_written() {
    let ui = Ui::start("traversal");
    std::fs::write(ui.dir.join("secret.txt"), "top secret").unwrap();
    let host = ui.host();
    for path in ["/../secret.txt", "/%2e%2e/secret.txt", "/..%2fsecret.txt", "/ui.js/../../secret.txt", "/secret.txt", "/state/uploads", "//etc/passwd", "/C:/Windows/win.ini"] {
        let (status, reply) = ui.send("GET", path, &[("Host", &host)], b"");
        assert!(status == 404 && !reply.contains("top secret"), "{path}: {reply}");
    }
    for name in ["..%2F..%2Fevil.txt", "..%5C..%5Cevil.txt", "%2Ftmp%2Fevil.txt", "C%3A%5Cevil.txt", "..", "evil.exe", "evil.txt%00.exe", ""] {
        let (status, _) = ui.api("POST", &format!("/api/upload?name={name}"), BLUEPRINT);
        assert_eq!(status, 400, "{name}");
    }
    assert!(!ui.dir.join("evil.txt").exists() && !ui.dir.join("state/evil.txt").exists());
    // the link box must not read local files
    let local = ui.dir.join("secret.txt").to_string_lossy().replace('\\', "/");
    for url in [local.clone(), format!("file:///{local}"), "ftp://example.com/a.schem".into(), "secret.txt".into()] {
        let (status, reply) = ui.api("POST", "/api/url", format!("{{\"url\":\"{url}\"}}").as_bytes());
        assert!(status == 422 && text(&reply, "error").contains("http://"), "{url}: {reply:?}");
    }
    // a good upload lands under the state folder, in a folder named by its contents
    let (status, reply) = ui.api("POST", "/api/upload?name=ok.txt", BLUEPRINT);
    assert_eq!((status, text(&reply, "blocks")), (200, "2"), "{reply:?}");
    assert!(ui.dir.join("state/uploads").join(text(&reply, "id")).join("ok.txt").exists());
}

#[test]
fn oversized_and_malformed_requests_are_refused() {
    let ui = Ui::start("size");
    let head = |extra: &str| format!("POST /api/upload?name=a.txt HTTP/1.1\r\nHost: {0}\r\nOrigin: http://{0}\r\nX-BuildBot-Token: {1}\r\n{extra}\r\n\r\n", ui.host(), ui.token);
    // one byte over the limit is refused from the header alone
    assert_eq!(ui.raw(head(&format!("Content-Length: {}", (32 << 20) + 1)).as_bytes()).0, 413);
    assert_eq!(ui.raw(head("Content-Length: 99999999999999999999999999").as_bytes()).0, 413);
    assert_eq!(ui.raw(head("Transfer-Encoding: chunked").as_bytes()).0, 411);
    assert_eq!(ui.raw(head("Content-Length: 5\r\nContent-Length: 50").as_bytes()).0, 400);
    assert_eq!(ui.raw(head("X: y").as_bytes()).0, 411);
    assert_eq!(ui.api("POST", "/api/url", &[b' '; 9000]).0, 413, "small JSON limit");
    let long = format!("GET / HTTP/1.1\r\nHost: {}\r\nX: {}\r\n\r\n", ui.host(), "a".repeat(20_000));
    assert_eq!(ui.raw(long.as_bytes()).0, 400);
    assert_eq!(ui.api("POST", "/api/upload?name=a.schem", b"\x1f\x8b broken").0, 422);
    assert_eq!(ui.api("GET", "/api/status", b"").0, 200, "still serving");
}

#[test]
fn build_fields_cannot_become_options_or_other_files() {
    let ui = Ui::start("inject");
    let (status, up) = ui.api("POST", "/api/upload?name=a.txt", BLUEPRINT);
    assert_eq!(status, 200);
    let id = text(&up, "id");
    let body = |id: &str, server: &str, name: &str, more: &str| format!("{{\"id\":\"{id}\",\"server\":\"{server}\",\"name\":\"{name}\"{more}}}");
    for bad in [
        body("../../x", "127.0.0.1:1", "Bot", ""),
        body(id, "--fresh", "Bot", ""),
        body(id, "127.0.0.1:1 --allow-partial", "Bot", ""),
        body(id, "host:1;calc.exe", "Bot", ""),
        body(id, "127.0.0.1:1", "--fresh", ""),
        body(id, "127.0.0.1:1", "a b", ""),
        body(id, "127.0.0.1:1", "Bot", ",\"origin\":\"--state-dir\""),
        body(id, "127.0.0.1:1", "Bot", ",\"version\":\"--x\""),
    ] {
        let (status, reply) = ui.api("POST", "/api/build", bad.as_bytes());
        assert_eq!(status, 400, "{bad}: {reply:?}");
    }
    let (_, s) = ui.api("GET", "/api/status", b"");
    assert_eq!(text(&s, "phase"), "idle", "nothing was started");
    // a form post from another page cannot even set the JSON content type
    let (host, origin) = (ui.host(), format!("http://{}", ui.host()));
    let headers = [("Host", host.as_str()), ("Origin", &origin), ("X-BuildBot-Token", &ui.token), ("Content-Type", "text/plain")];
    assert_eq!(ui.send("POST", "/api/build", &headers, body(id, "127.0.0.1:1", "Bot", "").as_bytes()).0, 400);
}

#[test]
fn only_the_loopback_address_answers() {
    let ui = Ui::start("loopback");
    // the address this machine uses towards other networks (connect on UDP sends nothing)
    let own = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").and_then(|()| s.local_addr()));
    let Ok(IpAddr::V4(ip)) = own.map(|a| a.ip()) else { return };
    if !ip.is_loopback() && !ip.is_unspecified() {
        let reached = TcpStream::connect_timeout(&SocketAddr::new(ip.into(), ui.port), Duration::from_secs(2));
        assert!(reached.is_err(), "reachable on {ip}");
    }
}
