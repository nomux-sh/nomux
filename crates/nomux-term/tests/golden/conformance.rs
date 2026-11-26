//! Conformance cases in the style of `vttest` and `esctest`: the cases are
//! data in the directory `cases`, one file for each group.
//!
//! A case has a name, the standard or the program behaviour that it follows,
//! the output of the program and the screen of a VT terminal after it:
//!
//! ```text
//! case IL outside the scroll region moves no row
//! follows DEC STD 070 IL; xterm "CSI Ps L"
//! input \e[2;3r\e[5H\e[L
//! screen
//! |                    |
//! ...
//! cursor 4,0
//! ```
//!
//! `input` has the escapes `\e` (ESC), `\a`, `\b`, `\t`, `\n`, `\r`, `\\`,
//! `\xNN` for one byte and `\u{N}` for one character. `screen` is the
//! snapshot of `snapshot.rs`.
//!
//! A known gap is a case where the VT library is different from xterm. The
//! case has a `gap` line that says the difference. If the difference is on
//! the screen, the case also has a `library` snapshot: the screen of
//! `nomux-term` now. The case then fails if the screen of `nomux-term`
//! changes, also if it becomes the screen of xterm: then remove the gap.
//!
//! Each case runs with all the output in one call of `Terminal::feed`, and
//! with one byte in each call: a sequence that a call divides must give the
//! same screen.

use std::fs;
use std::path::{Path, PathBuf};

use crate::snapshot::screen_after;

#[derive(Debug)]
struct Case {
    file: String,
    name: String,
    input: Vec<u8>,
    gap: Option<String>,
    screen: String,
    library: Option<String>,
}

fn case_files() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cases");
    let entries = fs::read_dir(&directory).expect("the directory of the cases exists");
    let mut files: Vec<PathBuf> = entries
        .map(|entry| entry.expect("the directory can be read").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .collect();
    files.sort();
    files
}

fn cases() -> Vec<Case> {
    let cases: Vec<Case> = case_files()
        .iter()
        .flat_map(|file| parse_file(file))
        .collect();
    assert!(!cases.is_empty(), "the directory has cases");
    cases
}

fn parse_file(path: &Path) -> Vec<Case> {
    let text = fs::read_to_string(path).expect("a case file is UTF-8 text");
    let file = path
        .file_name()
        .expect("a file has a name")
        .to_string_lossy()
        .into_owned();
    text.split("\n\n")
        .map(|block| {
            block
                .lines()
                .filter(|line| !line.starts_with('#'))
                .collect::<Vec<_>>()
        })
        .filter(|lines| !lines.is_empty())
        .map(|lines| parse_case(&file, &lines))
        .collect()
}

fn parse_case(file: &str, lines: &[&str]) -> Case {
    let mut name = None;
    let mut follows = None;
    let mut input = None;
    let mut gap = None;
    let mut snapshots: Vec<(&str, String)> = Vec::new();
    for line in lines {
        let (keyword, value) = line.split_once(' ').unwrap_or((line, ""));
        match keyword {
            "case" => name = Some(value.to_owned()),
            "follows" => follows = Some(value),
            "input" => input = Some(unescape(value)),
            "gap" => gap = Some(value.to_owned()),
            "screen" | "library" => snapshots.push((keyword, String::new())),
            _ => {
                let (_, snapshot) = snapshots
                    .last_mut()
                    .unwrap_or_else(|| panic!("{file}: the line {line:?} is not in a snapshot"));
                snapshot.push_str(line);
                snapshot.push('\n');
            }
        }
    }
    let name = name.unwrap_or_else(|| panic!("{file}: a case has no name: {lines:?}"));
    let context = format!("{file}: {name}");
    assert!(
        follows.is_some_and(|follows| !follows.is_empty()),
        "{context}: no `follows`"
    );
    let snapshot = |keyword: &str| {
        snapshots
            .iter()
            .find(|(found, _)| *found == keyword)
            .map(|(_, snapshot)| snapshot.clone())
    };
    let library = snapshot("library");
    assert!(
        library.is_none() || gap.is_some(),
        "{context}: a `library` snapshot needs a `gap`"
    );
    Case {
        file: file.to_owned(),
        input: input.unwrap_or_else(|| panic!("{context}: no `input`")),
        screen: snapshot("screen").unwrap_or_else(|| panic!("{context}: no `screen`")),
        library,
        gap,
        name,
    }
}

fn unescape(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        match characters.next() {
            Some('e') => bytes.push(0x1b),
            Some('a') => bytes.push(0x07),
            Some('b') => bytes.push(0x08),
            Some('t') => bytes.push(b'\t'),
            Some('n') => bytes.push(b'\n'),
            Some('r') => bytes.push(b'\r'),
            Some('\\') => bytes.push(b'\\'),
            Some('x') => {
                let digits: String = characters.by_ref().take(2).collect();
                let byte = u8::from_str_radix(&digits, 16);
                bytes.push(byte.unwrap_or_else(|_| panic!("`\\x{digits}` is not a byte")));
            }
            Some('u') => {
                let code: String = characters
                    .by_ref()
                    .take_while(|digit| *digit != '}')
                    .collect();
                let code = code.strip_prefix('{').unwrap_or(&code);
                let character = u32::from_str_radix(code, 16).ok().and_then(char::from_u32);
                let character =
                    character.unwrap_or_else(|| panic!("`\\u{{{code}}}` is not a character"));
                let mut buffer = [0; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
            escape => panic!("`\\{escape:?}` is not an escape of a case"),
        }
    }
    bytes
}

/// The difference of one case, or `None` if the case passes.
fn failure(case: &Case) -> Option<String> {
    let screen = screen_after([case.input.as_slice()]);
    let expected = case.library.as_ref().unwrap_or(&case.screen);
    let title = format!("{}: {}", case.file, case.name);
    if screen != *expected {
        let gap = case
            .gap
            .as_ref()
            .map_or(String::new(), |gap| format!("gap: {gap}\n"));
        return Some(format!(
            "{title}\n{gap}--- expected\n{expected}--- screen\n{screen}---"
        ));
    }
    if case.library.as_ref() == Some(&case.screen) {
        return Some(format!(
            "{title}\nthe library has the screen of xterm: remove the gap"
        ));
    }
    let divided = screen_after(case.input.chunks(1));
    if divided != screen {
        return Some(format!(
            "{title}\n--- one call\n{screen}--- one byte in each call\n{divided}---"
        ));
    }
    None
}

#[test]
fn each_case_gives_its_screen() {
    let failures: Vec<String> = cases().iter().filter_map(failure).collect();
    assert!(
        failures.is_empty(),
        "{} cases failed\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
