//! Minimal runtime language switch (English / Korean) for user-facing text.
//!
//! Use [`tr!`](crate::tr) with an English and a Korean format string:
//! `tr!("placed {}", "설치 {}", n)`.

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

static KOREAN: AtomicBool = AtomicBool::new(false);

pub fn set(lang: Lang) {
    KOREAN.store(lang == Lang::Ko, Ordering::Relaxed);
}

pub fn get() -> Lang {
    if is_ko() {
        Lang::Ko
    } else {
        Lang::En
    }
}

pub fn is_ko() -> bool {
    KOREAN.load(Ordering::Relaxed)
}

pub fn parse(s: &str) -> Option<Lang> {
    match s.trim().to_ascii_lowercase().as_str() {
        "en" | "eng" | "english" => Some(Lang::En),
        "ko" | "kr" | "kor" | "korean" => Some(Lang::Ko),
        _ => None,
    }
}

/// Pick the language: explicit flag value, then `BUILDBOT_LANG`, then a Korean
/// system locale (`LANG`/`LC_ALL`), otherwise English.
pub fn detect(flag: Option<&str>) -> Lang {
    if let Some(l) = flag.and_then(parse) {
        return l;
    }
    if let Some(l) = std::env::var("BUILDBOT_LANG").ok().as_deref().and_then(parse) {
        return l;
    }
    let locale = std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default();
    if locale.to_ascii_lowercase().starts_with("ko") {
        Lang::Ko
    } else {
        Lang::En
    }
}

/// `tr!("English {}", "한국어 {}", arg)` → `String` in the active language.
#[macro_export]
macro_rules! tr {
    ($en:literal, $ko:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_ko() {
            format!($ko $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_names() {
        assert_eq!(parse("EN"), Some(Lang::En));
        assert_eq!(parse(" ko "), Some(Lang::Ko));
        assert_eq!(parse("fr"), None);
    }

    #[test]
    fn explicit_flag_wins() {
        assert_eq!(detect(Some("ko")), Lang::Ko);
        assert_eq!(detect(Some("en")), Lang::En);
    }
}
