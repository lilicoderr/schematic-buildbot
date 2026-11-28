//! Just enough JSON to read the server's status response. The input comes from an arbitrary
//! server, so it is bounded in depth and never panics.

#[derive(Debug, PartialEq)]
pub enum Json {
    Str(String),
    /// Numbers, booleans and null, as written.
    Raw(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 64;

pub fn parse(text: &str) -> Option<Json> {
    value(&mut text.chars().peekable(), 0)
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn skip_space(c: &mut Chars) {
    while c.next_if(|ch| ch.is_whitespace()).is_some() {}
}

fn value(c: &mut Chars, depth: usize) -> Option<Json> {
    if depth > MAX_DEPTH {
        return None;
    }
    skip_space(c);
    match *c.peek()? {
        '"' => string(c).map(Json::Str),
        open @ ('{' | '[') => {
            c.next();
            let close = if open == '{' { '}' } else { ']' };
            let (mut fields, mut items) = (Vec::new(), Vec::new());
            loop {
                skip_space(c);
                if c.next_if_eq(&close).is_some() {
                    break;
                }
                if open == '{' {
                    let key = string(c)?;
                    skip_space(c);
                    c.next_if_eq(&':')?;
                    fields.push((key, value(c, depth + 1)?));
                } else {
                    items.push(value(c, depth + 1)?);
                }
                skip_space(c);
                if c.next_if_eq(&',').is_none() {
                    c.next_if_eq(&close)?;
                    break;
                }
            }
            Some(if open == '{' { Json::Object(fields) } else { Json::Array(items) })
        }
        _ => {
            let mut raw = String::new();
            while let Some(ch) = c.next_if(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '+' | '.')) {
                raw.push(ch);
            }
            (!raw.is_empty()).then_some(Json::Raw(raw))
        }
    }
}

fn string(c: &mut Chars) -> Option<String> {
    c.next_if_eq(&'"')?;
    let mut out = String::new();
    loop {
        match c.next()? {
            '"' => return Some(out),
            '\\' => match c.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' | 'b' | 'f' => {}
                'u' => {
                    let mut code = 0;
                    for _ in 0..4 {
                        code = code * 16 + c.next()?.to_digit(16)?;
                    }
                    // surrogate pairs (emoji in a server description) become the replacement mark
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_status_response() {
        let j = parse(r#"{"description":{"text":"\"protocol\": 1 §a","extra":[1,true,null]},"version":{"name":"1.21.1","protocol":767}}"#).unwrap();
        let v = j.get("version").unwrap();
        assert_eq!(v.get("protocol"), Some(&Json::Raw("767".into())));
        assert_eq!(v.get("name"), Some(&Json::Str("1.21.1".into())));
    }

    #[test]
    fn broken_or_hostile_input_is_none() {
        for bad in ["", "{", "{\"a\"}", "\"abc", &"[".repeat(100_000)] {
            assert_eq!(parse(bad), None);
        }
    }
}
