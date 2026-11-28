//! The routes. Order matters: `Host` first, then (for `/api`) origin and token, and only then is
//! a request body read. Static files come from a fixed table, never from the file system.

use super::http::{self, Request, Status};
use super::job::Job;
use super::runner::{self, lock, Control, Shared};
use super::{actions, assets, jsonw, security};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

pub struct App {
    pub port: u16,
    pub token: String,
    pub state_dir: PathBuf,
    /// The program started for a build (this one).
    pub exe: PathBuf,
    pub job: Shared<Job>,
    pub control: Shared<Control>,
    pub upload: Mutex<Option<super::analyze::Stored>>,
    /// Held while a file is analysed or a build is being started: one at a time.
    pub busy: Mutex<()>,
}

pub struct Reply {
    pub status: Status,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub fn json(body: String) -> Reply {
    Reply { status: Status::OK, content_type: "application/json", body: body.into_bytes() }
}

pub fn error(status: Status, message: &str) -> Reply {
    let body = jsonw::object(&[("error", jsonw::text(message))]).into_bytes();
    Reply { status, content_type: "application/json", body }
}

impl From<Status> for Reply {
    fn from(s: Status) -> Reply {
        error(s, s.1)
    }
}

pub fn serve(app: &App, mut stream: TcpStream) {
    let reply = match http::read_request(&mut stream) {
        Ok(mut req) => route(app, &mut req, &mut stream),
        Err(status) => status.into(),
    };
    let _ = http::respond(&mut stream, reply.status, reply.content_type, &reply.body);
    http::close(stream);
}

fn route(app: &App, req: &mut Request, stream: &mut TcpStream) -> Reply {
    if let Err(status) = security::check_host(req, app.port) {
        return status.into();
    }
    if !req.path.starts_with("/api/") {
        return match assets::find(&req.path) {
            Some(_) if req.method != "GET" => Status::METHOD.into(),
            Some((content_type, body)) => Reply { status: Status::OK, content_type, body },
            None => Status::NOT_FOUND.into(),
        };
    }
    if let Err(status) = security::check_api(req, app.port, &app.token) {
        return status.into();
    }
    let path = req.path.clone();
    match (req.method.as_str(), path.as_str()) {
        ("GET", "/api/info") => actions::info(),
        ("GET", "/api/status") => {
            let since = req.param("since").and_then(|v| v.parse().ok()).unwrap_or(0);
            json(lock(&app.job).json(since, Instant::now()))
        }
        ("POST", "/api/upload") => actions::upload(app, req, stream),
        ("POST", "/api/url") => actions::url(app, req, stream),
        ("POST", "/api/build") => actions::build(app, req, stream),
        ("POST", "/api/stop") => {
            let was_running = runner::stop(&app.job, &app.control);
            json(jsonw::object(&[("stopping", was_running.to_string())]))
        }
        (_, "/api/info" | "/api/status" | "/api/upload" | "/api/url" | "/api/build" | "/api/stop") => Status::METHOD.into(),
        _ => Status::NOT_FOUND.into(),
    }
}
