//! The rules of AGENTS.md that a line of code can break: names, `#[allow]`,
//! globals, and the depth of nesting (a heuristic from the indentation).

use super::sources::Source;
use super::{Finding, Level};

/// Names AGENTS.md ("Naming") bans, as a whole identifier.
const BANNED_NAMES: [&str; 6] = ["mgr", "ctx", "tmp", "util", "utils", "helper"];

/// An `if`, `match` or loop deeper than this many levels is a warning.
const MAX_NESTING: usize = 3;

/// Every rule, over the code of `files`.
pub(super) fn check(files: &[Source]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        for (index, line) in file.code().iter().enumerate() {
            let place = format!("{}:{}", file.path, index + 1);
            let code = strip(line);
            names(file, &code, &place, &mut findings);
            attributes(&code, &place, &mut findings);
        }
        nesting(file, &mut findings);
    }
    findings
}

/// `line` without its comment and the inside of its string literals.
fn strip(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let (mut in_string, mut i) = (false, 0);
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            match c {
                '\\' => i += 1,
                '"' => in_string = false,
                _ => {}
            }
        } else if c == '\'' && chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'\'') {
            i += 2;
        } else if c == '"' {
            in_string = true;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// The identifiers of `code`.
fn identifiers(code: &str) -> impl Iterator<Item = &str> {
    code.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with(|c: char| c.is_alphabetic() || c == '_'))
}

fn names(file: &Source, code: &str, place: &str, findings: &mut Vec<Finding>) {
    let mut push = |message: String| findings.push(Finding::new(Level::Error, "naming", place, message));
    let words: Vec<&str> = identifiers(code).collect();
    for (i, word) in words.iter().enumerate() {
        if word.starts_with("nb_") || word.starts_with("num_") {
            push(format!("`{word}`: a number of things ends in `_count`"));
        } else if word.ends_with("_idx") {
            push(format!("`{word}`: an index ends in `_index`"));
        } else if i > 0 && words[i - 1] == "fn" && word.starts_with("get_") && *word != "get_mut" {
            push(format!("`{word}`: an accessor is named after its field, with no `get_`"));
        } else if BANNED_NAMES.contains(word) && !code.contains(&format!("{word}::")) {
            push(format!("`{word}`: a name that says nothing"));
        } else if file.crate_name != "ordrumbox-ui" && word.split('_').any(|part| part == "bar" || part == "bars") {
            push(format!("`{word}`: a measure is a measure, never a bar (UI chrome only)"));
        }
    }
}

fn attributes(code: &str, place: &str, findings: &mut Vec<Finding>) {
    let mut push = |rule: &'static str, message: &str| findings.push(Finding::new(Level::Error, rule, place, message));
    if code.contains("#[allow(") || code.contains("#![allow(") {
        push("allow", "use `#[expect(lint, reason = \"…\")]`, never a bare `#[allow]`");
    }
    if code.contains("static mut ") || code.contains("thread_local!") {
        push("globals", "no `static mut` or `thread_local`: pass the parameter");
    }
}

/// Lines that open an `if`, a `match` or a loop deeper than [`MAX_NESTING`]
/// levels. Read from the indentation, which rustfmt fixes: a block ends at
/// the next line that is no deeper than the line that opened it.
fn nesting(file: &Source, findings: &mut Vec<Finding>) {
    let mut open: Vec<usize> = Vec::new();
    for (index, line) in file.code().iter().enumerate() {
        let code = strip(line);
        let text = code.trim();
        if text.is_empty() {
            continue;
        }
        let indent = code.len() - code.trim_start().len();
        while open.last().is_some_and(|&top| top >= indent) {
            open.pop();
        }
        if is_function(text) {
            open.clear();
        }
        let is_else = text.starts_with("} else");
        if is_else || opens_block(text) {
            if open.len() >= MAX_NESTING && !is_else {
                let message =
                    format!("`{}` is nested {} levels deep (limit {MAX_NESTING})", shorten(text), open.len() + 1);
                findings.push(Finding::new(Level::Warning, "nesting", format!("{}:{}", file.path, index + 1), message));
            }
            open.push(indent);
        }
    }
}

fn is_function(text: &str) -> bool {
    let text = text.strip_prefix("pub").map_or(text, |rest| rest.trim_start_matches(|c| c != ' ').trim_start());
    ["fn ", "const fn ", "async fn ", "unsafe fn "].iter().any(|p| text.starts_with(p))
}

fn opens_block(text: &str) -> bool {
    ["if ", "match ", "for ", "while ", "loop"].iter().any(|k| text.starts_with(k))
}

fn shorten(text: &str) -> String {
    text.chars().take(40).collect()
}
