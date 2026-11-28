//! Loading a schematic file or an http(s) download, with size and time limits. A download is
//! untrusted input: only http and https are fetched and the size is capped before parsing.

use crate::schematic::Schematic;
use crate::{litematic, schem, tr};
use std::io::Read;
use std::time::Duration;

/// Largest schematic file accepted (compressed size).
pub const MAX_BYTES: u64 = 32 << 20;
/// Time limit for the whole download, redirects included.
pub const TIMEOUT: Duration = Duration::from_secs(60);

pub struct Source {
    /// File name without any query string; the extension selects the parser.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// `true` for a URL, `false` for a local path, an error for any other `scheme://`.
fn is_url(src: &str) -> Result<bool, String> {
    let Some((scheme, _)) = src.split_once("://") else { return Ok(false) };
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        Ok(true)
    } else {
        Err(tr!(
            "only http:// and https:// links can be downloaded, not {}://",
            "http:// 와 https:// 링크만 내려받을 수 있습니다 ({}:// 는 안 됨)",
            scheme
        ))
    }
}

fn too_large(max: u64) -> String {
    tr!("the file is larger than the {} MB limit", "파일이 {} MB 제한보다 큽니다", max >> 20)
}

fn read_limited(r: impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    r.take(max + 1).read_to_end(&mut buf).map_err(|e| tr!("the download failed: {}", "다운로드 실패: {}", e))?;
    if buf.len() as u64 > max {
        return Err(too_large(max));
    }
    Ok(buf)
}

fn download(url: &str, max: u64, timeout: Duration) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(timeout).build();
    let resp = agent.get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => tr!(
            "the download failed: HTTP {} (is the link still valid, and a direct file link?)",
            "다운로드 실패: HTTP {} (링크가 유효한 직접 파일 링크인지 확인하세요)",
            code
        ),
        other => tr!("the download failed: {}", "다운로드 실패: {}", other),
    })?;
    if resp.header("content-length").and_then(|v| v.parse::<u64>().ok()).is_some_and(|n| n > max) {
        return Err(too_large(max));
    }
    read_limited(resp.into_reader(), max)
}

fn looks_like_web_page(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace() && !matches!(b, 0xef | 0xbb | 0xbf));
    start.is_some_and(|i| bytes[i] == b'<')
}

/// Read `src` (path or http(s) URL) with the default limits.
pub fn fetch(src: &str) -> Result<Source, String> {
    fetch_limited(src, MAX_BYTES, TIMEOUT)
}

pub fn fetch_limited(src: &str, max: u64, timeout: Duration) -> Result<Source, String> {
    let url = is_url(src)?;
    let path_part = if url { src.split(['?', '#']).next().unwrap_or(src) } else { src };
    let name = path_part.rsplit(['/', '\\']).next().unwrap_or("blueprint").to_string();
    let bytes = if url {
        download(src, max, timeout)?
    } else {
        let file = std::fs::File::open(src).map_err(|e| e.to_string())?;
        read_limited(file, max)?
    };
    if looks_like_web_page(&bytes) {
        return Err(tr!(
            "this is a web page, not a schematic file. On the sharing site, copy the address of the download button (a direct .schem / .litematic link), or download the file and give its path.",
            "설계도 파일이 아니라 웹 페이지입니다. 공유 사이트에서 다운로드 버튼의 링크 주소(.schem / .litematic 직접 링크)를 복사하거나, 파일을 내려받아 경로를 입력하세요."
        ));
    }
    Ok(Source { name, bytes })
}

/// Parse by extension, falling back to sniffing: gzip NBT is a schematic, anything else is the
/// text format. Item names are checked against the active version.
pub fn parse(source: &Source) -> Result<Schematic, String> {
    let (name, bytes) = (source.name.as_str(), source.bytes.as_slice());
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let gzip = bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b;
    match ext.as_str() {
        "litematic" => litematic::parse(bytes, name),
        "schem" | "schematic" => schem::parse(bytes, name),
        _ if gzip => schem::parse(bytes, name).or_else(|e| litematic::parse(bytes, name).map_err(|_| e)),
        _ => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| tr!("the text blueprint is not valid UTF-8", "텍스트 설계도가 UTF-8 이 아닙니다."))?;
            Schematic::parse(text)
        }
    }
}
