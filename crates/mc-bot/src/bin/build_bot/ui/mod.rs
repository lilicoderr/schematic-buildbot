//! `build-bot ui`: a local web page for `build`. It listens on the loopback address only.

mod actions;
mod analyze;
mod assets;
mod http;
mod job;
mod jsonw;
mod routes;
mod runner;
mod security;

use super::plan_cmds::fail;
use mc_bot::tr;
use routes::App;
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// More simultaneous connections than one page ever needs.
const MAX_CONNECTIONS: usize = 24;

#[derive(Debug)]
struct Args {
    port: u16,
    open: bool,
    state_dir: PathBuf,
}

/// There is deliberately no option for another listening address.
fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { port: 0, open: true, state_dir: PathBuf::from(".buildbot") };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match (arg.as_str(), matches!(arg.as_str(), "--port" | "--state-dir" | "--lang")) {
            ("--no-open", _) => a.open = false,
            (flag, true) => {
                let v = it.next().ok_or_else(|| tr!("{} needs a value", "{} 에 값이 필요합니다", flag))?;
                match flag {
                    "--port" => a.port = v.parse().map_err(|_| tr!("{} needs a number", "{} 에는 숫자가 필요합니다", flag))?,
                    "--state-dir" => a.state_dir = PathBuf::from(v),
                    _ => {} // --lang was applied before parsing
                }
            }
            _ => {
                return Err(tr!(
                    "usage: build-bot ui [--port N] [--no-open] [--state-dir DIR] [--lang en|ko]",
                    "사용법: build-bot ui [--port 번호] [--no-open] [--state-dir 폴더] [--lang en|ko]"
                ))
            }
        }
    }
    Ok(a)
}

/// Open the default browser, without a shell. Windows only: elsewhere a program's arguments
/// (and so the token) can be visible to other users of the machine; open the printed link.
fn open_browser(url: &str) {
    if cfg!(windows) {
        let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    }
}

pub fn ui_cmd(raw: &[String]) {
    let args = parse(raw).unwrap_or_else(|e| fail(e));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, args.port)).unwrap_or_else(|e| fail(format!("127.0.0.1:{}: {e}", args.port)));
    let port = listener.local_addr().unwrap_or_else(|e| fail(e.to_string())).port();
    let app = Arc::new(App {
        port,
        token: security::new_token().unwrap_or_else(|e| fail(e)),
        state_dir: args.state_dir,
        exe: std::env::current_exe().unwrap_or_else(|e| fail(e.to_string())),
        job: Default::default(),
        control: Default::default(),
        upload: Mutex::new(None),
        busy: Mutex::new(()),
    });
    // the token travels in the fragment, which browsers never send to a server or as a referrer
    let url = format!("http://127.0.0.1:{port}/#token={}", app.token);
    println!(
        "{}",
        tr!(
            "build-bot UI (this computer only): {}\nKeep this window open. Ctrl+C closes the UI and stops a running build (it can be resumed).",
            "build-bot UI (이 컴퓨터에서만 접속 가능): {}\n이 창을 열어 두세요. Ctrl+C 를 누르면 UI 가 닫히고 진행 중인 건축이 멈춥니다 (이어서 가능).",
            url
        )
    );
    let _ = std::io::stdout().flush();
    if args.open {
        open_browser(&url);
    }
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            let _ = http::respond(&mut stream, http::Status::BUSY, "text/plain", b"busy");
            continue;
        }
        let (app, open) = (app.clone(), open.clone());
        std::thread::spawn(move || {
            routes::serve(&app, stream);
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options() {
        let p = |l: &[&str]| parse(&l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let a = p(&["--port", "8123", "--no-open"]).unwrap();
        assert_eq!((a.port, a.open), (8123, false));
        for bad in [&["--bind", "0.0.0.0"][..], &["--host"], &["--port"], &["--port", "x"], &["stray"]] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }
}
