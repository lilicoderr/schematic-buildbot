//! The page and everything it loads, compiled into the binary so it works offline. The 3D view
//! is the plan previewer of `tools/plan_preview`; three.js is vendored with its licence.

const JS: &str = "text/javascript; charset=utf-8";
const TEXT: &str = "text/plain; charset=utf-8";

const PAGE: &str = include_str!("../../../../webui/index.html");
const STYLE: &str = include_str!("../../../../webui/ui.css");
const APP: &str = include_str!("../../../../webui/ui.js");
const LIVE: &str = include_str!("../../../../webui/live.js");
const TEXTS: &str = include_str!("../../../../webui/texts.js");
const PLAN: &str = include_str!("../../../../../../tools/plan_preview/plan.js");
const VIEWER: &str = include_str!("../../../../../../tools/plan_preview/viewer.js");
const THREE: &str = include_str!("../../../../webui/vendor/three/three.module.min.js");
const ORBIT: &str = include_str!("../../../../webui/vendor/three/OrbitControls.js");
const THREE_LICENSE: &str = include_str!("../../../../webui/vendor/three/LICENSE");

/// The previewer names three.js by bare module names, which would need an inline import map (and
/// so a weaker content policy): point them at the vendored files instead.
fn local_imports(source: &str) -> String {
    source
        .replace("from 'three/addons/controls/OrbitControls.js'", "from './OrbitControls.js'")
        .replace("from 'three'", "from './three.module.min.js'")
}

/// (content type, body) for exactly these paths.
pub fn find(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let plain = |t: &'static str, s: &str| Some((t, s.as_bytes().to_vec()));
    match path {
        "/" => plain("text/html; charset=utf-8", PAGE),
        "/ui.css" => plain("text/css; charset=utf-8", STYLE),
        "/ui.js" => plain(JS, APP),
        "/live.js" => plain(JS, LIVE),
        "/texts.js" => plain(JS, TEXTS),
        "/plan.js" => plain(JS, PLAN),
        "/viewer.js" => Some((JS, local_imports(VIEWER).into_bytes())),
        "/OrbitControls.js" => Some((JS, local_imports(ORBIT).into_bytes())),
        "/three.module.min.js" => plain(JS, THREE),
        "/three.LICENSE.txt" => plain(TEXT, THREE_LICENSE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_the_page_loads_is_remote_or_a_bare_module() {
        for path in ["/", "/ui.js", "/live.js", "/texts.js", "/viewer.js", "/OrbitControls.js", "/ui.css"] {
            let text = String::from_utf8(find(path).unwrap().1).unwrap();
            assert!(!text.contains("from 'three"), "{path}");
            assert!(!text.contains("cdn.") && !text.contains("unpkg"), "{path}");
        }
        let viewer = String::from_utf8(find("/viewer.js").unwrap().1).unwrap();
        assert!(viewer.contains("from './three.module.min.js'") && viewer.contains("from './OrbitControls.js'"));
        assert!(THREE_LICENSE.contains("MIT"));
        for outside in ["/../Cargo.toml", "/ui.js/", "/vendor/three/LICENSE", "/%2e%2e/", "//ui.js"] {
            assert!(find(outside).is_none(), "{outside}");
        }
    }

    #[test]
    fn the_page_has_no_inline_script_or_style() {
        assert!(!PAGE.contains("<style") && !PAGE.contains(" style=") && !PAGE.contains(" onclick"));
        assert!(PAGE.split("<script").skip(1).all(|s| s.trim_start().starts_with("src=") || s.starts_with(" type=\"module\" src=") || s.starts_with(" src=")));
    }
}
