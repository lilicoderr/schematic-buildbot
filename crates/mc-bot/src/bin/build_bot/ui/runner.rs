//! The build runs as `build-bot build ...` in its own process, started with an argument list
//! (never through a shell). It does everything the command line does, and a crash or a stop
//! cannot take the page with it.

use super::job::{Job, Phase};
use mc_bot::{tr, ui_link};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A build that has not reached a safe point this long after Stop is ended; its progress file is
/// written after every step, so it can still be resumed.
const STOP_LIMIT: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct Control {
    stdin: Option<ChildStdin>,
    stop_asked: Option<Instant>,
}

pub type Shared<T> = Arc<Mutex<T>>;

/// A poisoned lock only means a thread panicked while holding it; the data is still usable.
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_lines(pipe: impl Read + Send + 'static, job: Shared<Job>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut r = BufReader::new(pipe);
        let mut line = Vec::new();
        while r.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            lock(&job).line(String::from_utf8_lossy(&line).trim_end(), Instant::now());
            line.clear();
        }
    })
}

/// Start the build. The caller has checked every argument and set the job to `Starting`.
pub fn start(exe: PathBuf, args: Vec<String>, job: Shared<Job>, control: Shared<Control>) -> Result<(), String> {
    let mut child = Command::new(exe)
        .args(&args)
        .env(ui_link::ENV, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tr!("cannot start the build: {}", "건축을 시작할 수 없습니다: {}", e))?;
    *lock(&control) = Control { stdin: child.stdin.take(), stop_asked: None };
    let readers = [
        child.stdout.take().map(|p| read_lines(p, job.clone())),
        child.stderr.take().map(|p| read_lines(p, job.clone())),
    ];
    std::thread::spawn(move || watch(child, readers.into_iter().flatten().collect(), job, control));
    Ok(())
}

fn watch(mut child: Child, readers: Vec<std::thread::JoinHandle<()>>, job: Shared<Job>, control: Shared<Control>) {
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if lock(&control).stop_asked.is_some_and(|t| t.elapsed() > STOP_LIMIT) {
            let _ = child.kill();
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for r in readers {
        let _ = r.join();
    }
    let stop_asked = {
        let mut c = lock(&control);
        c.stdin = None;
        c.stop_asked.is_some()
    };
    let mut job = lock(&job);
    job.phase = if status.is_some_and(|s| s.success()) {
        Phase::Done
    } else if stop_asked {
        job.say(&tr!(
            "stopped. Press Build again with the same file, server and origin to resume.",
            "중지했습니다. 같은 파일, 서버, 원점으로 건축을 다시 누르면 이어서 짓습니다."
        ));
        Phase::Stopped
    } else {
        Phase::Failed
    };
}

/// Ask for a safe stop. `false` when nothing is running.
pub fn stop(job: &Shared<Job>, control: &Shared<Control>) -> bool {
    let mut c = lock(control);
    let Some(stdin) = c.stdin.as_mut() else { return false };
    let _ = stdin.write_all(b"stop\n").and_then(|()| stdin.flush());
    c.stop_asked.get_or_insert_with(Instant::now);
    drop(c);
    let mut job = lock(job);
    if job.phase.active() {
        job.phase = Phase::Stopping;
    }
    true
}
