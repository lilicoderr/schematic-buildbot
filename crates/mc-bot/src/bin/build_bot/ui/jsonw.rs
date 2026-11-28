//! Writing JSON (reading is [`mc_bot::json`]). Values are already-encoded JSON text.

pub fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn array(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

pub fn object(fields: &[(&str, String)]) -> String {
    format!("{{{}}}", fields.iter().map(|(k, v)| format!("{}:{v}", text(k))).collect::<Vec<_>>().join(","))
}

pub fn texts<S: AsRef<str>>(items: &[S]) -> String {
    array(items.iter().map(|s| text(s.as_ref())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_bot::json::{parse, Json};

    #[test]
    fn what_is_written_reads_back() {
        let nasty = "a\"b\\c\nd\u{1}</script> 한글";
        let j = parse(&object(&[("k", text(nasty)), ("n", "5".into()), ("l", texts(&["x"]))])).unwrap();
        assert_eq!(j.get("k"), Some(&Json::Str(nasty.into())));
        assert_eq!(j.get("n"), Some(&Json::Raw("5".into())));
    }
}
