//! Everything that decides whether a request may do something. The page is served on loopback
//! only, but other web sites open in the same browser can still send requests there: DNS
//! rebinding is stopped by the `Host` check, cross-site requests by `Origin` and by a random
//! per-run token that only the page opened from the printed link knows.

use super::http::{Request, Status};
use mc_bot::tr;

pub const TOKEN_HEADER: &str = "x-buildbot-token";

/// 32 bytes from the operating system's random generator, as hex.
pub fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("random generator: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Compare without telling an attacker how many leading characters were right.
fn same_secret(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn own_host(value: &str, port: u16) -> bool {
    value == format!("127.0.0.1:{port}") || value.eq_ignore_ascii_case(&format!("localhost:{port}"))
}

/// Every request: the browser must believe it is talking to this machine's loopback address. A
/// foreign name that resolves to 127.0.0.1 arrives with that name in `Host`.
pub fn check_host(req: &Request, port: u16) -> Result<(), Status> {
    match req.single("host") {
        Some(h) if own_host(h, port) => Ok(()),
        _ => Err(Status::FORBIDDEN),
    }
}

/// API requests: no foreign origin ever, an origin is required for anything but GET, and the
/// token is required for everything.
pub fn check_api(req: &Request, port: u16, token: &str) -> Result<(), Status> {
    let host = req.single("host").unwrap_or_default();
    match req.headers_named("origin").as_slice() {
        [] if req.method == "GET" => {}
        [o] if o.strip_prefix("http://").is_some_and(|h| own_host(h, port) && h.eq_ignore_ascii_case(host)) => {}
        _ => return Err(Status::FORBIDDEN),
    }
    if req.headers_named("sec-fetch-site").iter().any(|s| !matches!(*s, "same-origin" | "none")) {
        return Err(Status::FORBIDDEN);
    }
    match req.single(TOKEN_HEADER) {
        Some(t) if same_secret(t, token) => Ok(()),
        _ => Err(Status::UNAUTHORIZED),
    }
}

/// Extensions the parsers understand.
const EXTENSIONS: [&str; 4] = ["schem", "schematic", "litematic", "txt"];

/// The file name an upload is stored under. Anything that could name another folder is refused
/// outright; other unusual characters are replaced.
pub fn safe_file_name(name: &str) -> Result<String, String> {
    let bad = || tr!("that file name is not allowed", "허용되지 않는 파일 이름입니다");
    if name.is_empty() || name.len() > 255 || name.contains(['/', '\\', ':', '\0']) || name.contains("..") {
        return Err(bad());
    }
    let (stem, ext) = name.rsplit_once('.').ok_or_else(|| unknown_type(name))?;
    let ext = ext.to_ascii_lowercase();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(unknown_type(name));
    }
    let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || "-_".contains(c) { c } else { '_' }).take(60).collect();
    let stem = if stem.is_empty() { "blueprint".to_string() } else { stem };
    // "CON.txt" and friends are devices on Windows, not files
    let device = matches!(stem.to_ascii_uppercase().as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && matches!(stem[..3].to_ascii_uppercase().as_str(), "COM" | "LPT") && stem.ends_with(|c: char| c.is_ascii_digit()));
    Ok(format!("{}{stem}.{ext}", if device { "_" } else { "" }))
}

fn unknown_type(name: &str) -> String {
    let shown: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    tr!("'{}' is not a .schem, .litematic or .txt file", "'{}' 는 .schem, .litematic, .txt 파일이 아닙니다", shown)
}

/// A server address that is safe to hand to the build as one argument.
pub fn check_server(addr: &str) -> Result<(), String> {
    let allowed = |c: char| c.is_alphanumeric() || "-_.:[]".contains(c);
    if addr.len() > 260 || addr.starts_with('-') || !addr.chars().all(allowed) {
        return Err(tr!("the server address must look like host:port", "서버 주소는 host:port 형식이어야 합니다"));
    }
    mc_bot::preflight::split_address(addr).map(|_| ())
}

pub fn check_name(name: &str) -> Result<(), String> {
    if (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Ok(());
    }
    Err(tr!("the bot name must be 3 to 16 letters, digits or _", "봇 이름은 영문, 숫자, _ 3~16자여야 합니다"))
}

/// Only web links are fetched for the page: a local path would let it read any file.
pub fn check_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    let web = lower.starts_with("http://") || lower.starts_with("https://");
    if !web || url.len() > 2048 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(tr!("paste a link that starts with http:// or https://", "http:// 또는 https:// 로 시작하는 링크를 붙여넣으세요"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_and_compared_whole() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(same_secret(&a, &a.clone()));
        assert!(!same_secret(&a, &a[..63]) && !same_secret(&a, &b) && !same_secret("", &a));
    }

    #[test]
    fn upload_names_cannot_leave_the_folder() {
        for bad in ["", "..", "../x.schem", "..\\x.schem", "a/b.txt", "C:x.txt", "/etc/x.txt", "x..schem", "x\0.txt", "x.exe", "noext"] {
            assert!(safe_file_name(bad).is_err(), "{bad:?}");
        }
        assert_eq!(safe_file_name("My Castle (1).SCHEM").unwrap(), "My_Castle__1_.schem");
        assert_eq!(safe_file_name("성 하나.litematic").unwrap(), "성_하나.litematic");
        assert_eq!(safe_file_name(".txt").unwrap(), "blueprint.txt");
        assert_eq!(safe_file_name("con.txt").unwrap(), "_con.txt");
        assert_eq!(safe_file_name("COM3.schem").unwrap(), "_COM3.schem");
    }

    #[test]
    fn build_arguments_cannot_become_options_or_paths() {
        assert!(check_server("127.0.0.1:25565").is_ok() && check_server("[::1]:25565").is_ok());
        for bad in ["", "--fresh", "-x:1", "a b:1", "http://x:1", "h:1;calc", "h:99999", "h:1\n"] {
            assert!(check_server(bad).is_err(), "{bad:?}");
        }
        assert!(check_name("Build_Bot1").is_ok());
        assert!(check_name("--x").is_err() && check_name("ab").is_err() && check_name("a b c").is_err());
        assert!(check_url("HTTPS://example.com/a.schem").is_ok());
        for bad in ["file:///etc/passwd", "C:\\x.schem", "/etc/passwd", "ftp://x/y", "http://x/ y", "x.schem"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }
}
