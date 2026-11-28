//! `build-bot build --ask`: the questions behind the double-click helper `build.bat`. The answers
//! are read here, not by the batch file, so no shell ever parses them.

use std::io::{self, BufRead, Write};

const DEFAULT_SERVER: &str = "127.0.0.1:25565";

/// Dragging a file onto the window pastes its path in quotes.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    ['"', '\''].iter().find_map(|&q| s.strip_prefix(q)?.strip_suffix(q)).unwrap_or(s)
}

fn question(input: &mut dyn BufRead, out: &mut dyn Write, text: &str) -> Result<String, String> {
    writeln!(out, "{text}").and_then(|()| write!(out, "> ")).and_then(|()| out.flush()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    let n = input.read_line(&mut line).map_err(|e| e.to_string())?;
    let answer = unquote(line.trim_start_matches('\u{feff}'));
    if n == 0 {
        return Err("no input / 입력이 없습니다".into());
    }
    // an answer must never turn into an option
    if answer.starts_with('-') {
        return Err("an answer cannot start with '-' / '-' 로 시작할 수 없습니다".into());
    }
    Ok(answer.to_string())
}

/// The arguments of `build`, from the answers. `extra` are options already on the command line.
pub fn ask(input: &mut dyn BufRead, out: &mut dyn Write, extra: &[String]) -> Result<Vec<String>, String> {
    let mut args = extra.to_vec();
    if !args.iter().any(|a| a == "--lang") {
        let lang = question(input, out, "Language / 언어: 1 = English, 2 = 한국어 (Enter = 1)")?;
        let ko = matches!(lang.as_str(), "2" | "ko");
        args.extend(["--lang".to_string(), if ko { "ko" } else { "en" }.to_string()]);
        mc_bot::i18n::set(if ko { mc_bot::i18n::Lang::Ko } else { mc_bot::i18n::Lang::En });
    }
    let source = question(input, out, &mc_bot::tr!(
        "Schematic: paste a direct .schem / .litematic link, or drag the file onto this window",
        "설계도: .schem / .litematic 직접 링크를 붙여넣거나, 파일을 이 창에 끌어다 놓으세요"
    ))?;
    if source.is_empty() {
        return Err(mc_bot::tr!("no schematic given", "설계도를 입력하지 않았습니다"));
    }
    let server = question(input, out, &mc_bot::tr!("Server address (Enter = {})", "서버 주소 (Enter = {})", DEFAULT_SERVER))?;
    let name = question(input, out, &mc_bot::tr!("Bot name (Enter = BuildBot)", "봇 이름 (Enter = BuildBot)"))?;
    if !name.is_empty() {
        args.extend(["--name".to_string(), name]);
    }
    args.push(source);
    args.push(if server.is_empty() { DEFAULT_SERVER.to_string() } else { server });
    Ok(args)
}

/// [`ask`] on the console.
pub fn ask_console(extra: &[String]) -> Result<Vec<String>, String> {
    ask(&mut io::stdin().lock(), &mut io::stdout(), extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Result<Vec<String>, String> {
        ask(&mut text.as_bytes(), &mut Vec::new(), &["--lang".into(), "en".into()])
    }

    #[test]
    fn quoted_paths_with_shell_characters_stay_one_argument() {
        let args = run("\"C:\\my builds\\a & calc %PATH% !x!.schem\"\r\n\r\n\r\n").unwrap();
        let a = super::super::build_args::parse(&args).unwrap();
        assert_eq!(a.source, "C:\\my builds\\a & calc %PATH% !x!.schem");
        assert_eq!(a.server.as_deref(), Some(DEFAULT_SERVER));
        assert_eq!(a.name, "BuildBot");
    }

    #[test]
    fn server_and_name_answers_are_used() {
        let args = run("http://x/a.schem?b=1&c=2\nmc.example.org:25570\nHelper\n").unwrap();
        let a = super::super::build_args::parse(&args).unwrap();
        assert_eq!((a.source.as_str(), a.server.as_deref(), a.name.as_str()), ("http://x/a.schem?b=1&c=2", Some("mc.example.org:25570"), "Helper"));
    }

    #[test]
    fn options_and_empty_answers_are_refused() {
        for bad in ["--fresh\n", "\n", "", "a.schem\n--allow-partial\n\n"] {
            assert!(run(bad).is_err(), "{bad:?}");
        }
    }
}
