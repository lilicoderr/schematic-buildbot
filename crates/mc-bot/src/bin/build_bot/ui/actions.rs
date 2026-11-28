//! What the POST routes do. Every request here has already passed the host, origin and token
//! checks.

use super::analyze;
use super::http::{Request, Status};
use super::routes::{error, json, App, Reply};
use super::runner::{self, lock};
use super::{jsonw, security};
use mc_bot::fetch::{self, Source};
use mc_bot::json::{self as read, Json};
use mc_bot::{i18n, tr, version};
use std::net::TcpStream;
use std::sync::{MutexGuard, TryLockError};
use std::time::Duration;

const MAX_JSON: u64 = 8 << 10;
const UPLOAD_TIME: Duration = Duration::from_secs(120);

pub fn info() -> Reply {
    let versions: Vec<&str> = version::SUPPORTED.iter().map(|v| v.name).collect();
    json(jsonw::object(&[
        ("lang", jsonw::text(if i18n::is_ko() { "ko" } else { "en" })),
        ("versions", jsonw::texts(&versions)),
        ("preview_version", jsonw::text(version::DEFAULT.name)),
        ("max_upload", fetch::MAX_BYTES.to_string()),
    ]))
}

/// The one-at-a-time lock, refused while a build runs.
fn exclusive(app: &App) -> Result<MutexGuard<'_, ()>, Reply> {
    let guard = match app.busy.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => return Err(error(Status::CONFLICT, &tr!("another file is still being processed", "다른 파일을 처리하는 중입니다"))),
    };
    if lock(&app.job).phase.active() {
        return Err(error(Status::CONFLICT, &tr!("a build is running; stop it first", "건축이 진행 중입니다. 먼저 중지하세요")));
    }
    Ok(guard)
}

/// A small JSON object body.
fn object(req: &mut Request, stream: &mut TcpStream) -> Result<Json, Reply> {
    if !req.single("content-type").is_some_and(|t| t.starts_with("application/json")) {
        return Err(Status::BAD_REQUEST.into());
    }
    let body = req.body(stream, MAX_JSON, Duration::from_secs(10))?;
    std::str::from_utf8(&body).ok().and_then(read::parse).ok_or_else(|| Status::BAD_REQUEST.into())
}

fn field<'a>(j: &'a Json, key: &str) -> &'a str {
    match j.get(key) {
        Some(Json::Str(s)) => s.trim(),
        _ => "",
    }
}

fn flag(j: &Json, key: &str) -> bool {
    matches!(j.get(key), Some(Json::Raw(v)) if v == "true")
}

fn optional(s: String) -> Option<String> {
    (!s.is_empty() && s != "auto").then_some(s)
}

/// Keep the file, describe it, and remember it as the one `/api/build` builds.
fn accept(app: &App, source: Source, release: Option<String>) -> Reply {
    *lock(&app.upload) = None;
    let result = analyze::store(&app.state_dir, &source.name, &source.bytes)
        .and_then(|stored| analyze::describe(&stored, source, release).map(|text| (stored, text)));
    match result {
        Ok((stored, text)) => {
            *lock(&app.upload) = Some(stored);
            json(text)
        }
        Err(e) => error(Status::UNPROCESSABLE, &e),
    }
}

/// `POST /api/upload?name=<file name>[&version=V]`, the file as the raw body.
pub fn upload(app: &App, req: &mut Request, stream: &mut TcpStream) -> Reply {
    let _one = match exclusive(app) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let name = match security::safe_file_name(&req.param("name").unwrap_or_default()) {
        Ok(n) => n,
        Err(e) => return error(Status::BAD_REQUEST, &e),
    };
    match req.body(stream, fetch::MAX_BYTES, UPLOAD_TIME) {
        Ok(bytes) => accept(app, Source { name, bytes }, req.param("version").and_then(optional)),
        Err(status) => status.into(),
    }
}

/// `POST /api/url {"url": ..., "version": ...}`: http(s) only, with the limits of `build`.
pub fn url(app: &App, req: &mut Request, stream: &mut TcpStream) -> Reply {
    let (_one, j) = match exclusive(app).and_then(|g| Ok((g, object(req, stream)?))) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let url = field(&j, "url");
    match security::check_url(url).and_then(|()| fetch::fetch(url)) {
        Ok(mut source) => {
            source.name = analyze::downloaded_name(&source);
            accept(app, source, optional(field(&j, "version").to_string()))
        }
        Err(e) => error(Status::UNPROCESSABLE, &e),
    }
}

/// The arguments of the build process; every value from the page is checked first.
fn build_args(app: &App, j: &Json) -> Result<Vec<String>, String> {
    let upload = lock(&app.upload).clone().filter(|u| u.id == field(j, "id"));
    let upload = upload.ok_or_else(|| tr!("load the schematic again", "설계도를 다시 불러오세요"))?;
    let (server, name) = (field(j, "server"), field(j, "name"));
    security::check_server(server)?;
    security::check_name(name)?;
    let text = |p: &std::path::Path| p.to_str().map(String::from).ok_or_else(|| "the state folder path is not valid UTF-8".to_string());
    let lang = if i18n::is_ko() { "ko" } else { "en" };
    let mut args: Vec<String> = ["build", &text(&upload.path)?, server, "--name", name, "--state-dir", &text(&app.state_dir)?, "--lang", lang]
        .map(String::from)
        .to_vec();
    if let Some(v) = optional(field(j, "version").to_string()) {
        args.extend(["--version".to_string(), version::lookup(&v)?.name.to_string()]);
    }
    let origin = field(j, "origin");
    if !origin.is_empty() {
        let parts: Option<Vec<i32>> = origin.split(',').map(|p| p.trim().parse().ok()).collect();
        let Some(&[x, y, z]) = parts.as_deref() else { return Err(tr!("the origin needs x,y,z (whole numbers)", "원점은 x,y,z (정수) 형식입니다")) };
        args.extend(["--origin".to_string(), format!("{x},{y},{z}")]);
    }
    for (key, option) in [("allow_partial", "--allow-partial"), ("fresh", "--fresh")] {
        if flag(j, key) {
            args.push(option.to_string());
        }
    }
    Ok(args)
}

/// `POST /api/build {"id", "server", "name", "version"?, "origin"?, "allow_partial"?, "fresh"?}`.
pub fn build(app: &App, req: &mut Request, stream: &mut TcpStream) -> Reply {
    let (_one, j) = match exclusive(app).and_then(|g| Ok((g, object(req, stream)?))) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let args = match build_args(app, &j) {
        Ok(a) => a,
        Err(e) => return error(Status::BAD_REQUEST, &e),
    };
    lock(&app.job).start();
    match runner::start(app.exe.clone(), args, app.job.clone(), app.control.clone()) {
        Ok(()) => json(jsonw::object(&[("started", "true".into())])),
        Err(e) => {
            let mut job = lock(&app.job);
            job.say(&e);
            job.phase = super::job::Phase::Failed;
            error(Status::UNPROCESSABLE, &e)
        }
    }
}
