//! Blocks of code that appear twice: AGENTS.md ("Reuse") wants a rule that
//! repeats extracted at its second copy.

use std::collections::{BTreeMap, HashMap};

use super::sources::Source;
use super::{Finding, Level};

/// Consecutive lines of code that make a block.
const BLOCK_LINES: usize = 6;
/// A block shorter than this (in characters) is boilerplate, not a rule.
const BLOCK_CHARS: usize = 150;
/// Lines that carry nothing by themselves.
const CLOSERS: [&str; 6] = ["}", "{", "});", "},", "),", "];"];

/// The pairs of files (or the one file) that share blocks of code.
pub(super) fn check(files: &[Source]) -> Vec<Finding> {
    let mut seen: HashMap<String, Vec<(&str, usize)>> = HashMap::new();
    for file in files {
        let lines: Vec<(usize, String)> = file
            .code()
            .iter()
            .enumerate()
            .map(|(i, l)| (i + 1, l.split_whitespace().collect::<Vec<_>>().join(" ")))
            .filter(|(_, l)| !l.is_empty() && !l.starts_with("//") && !CLOSERS.contains(&l.as_str()))
            .collect();
        for window in lines.windows(BLOCK_LINES) {
            let block: Vec<&str> = window.iter().map(|(_, l)| l.as_str()).collect();
            if block.iter().map(|l| l.len()).sum::<usize>() >= BLOCK_CHARS {
                seen.entry(block.join("\n")).or_default().push((&file.path, window[0].0));
            }
        }
    }
    let mut shared: BTreeMap<Vec<&str>, (usize, usize)> = BTreeMap::new();
    for places in seen.values().filter(|p| p.len() > 1) {
        let mut paths: Vec<&str> = places.iter().map(|(path, _)| *path).collect();
        paths.sort_unstable();
        paths.dedup();
        let entry = shared.entry(paths).or_insert((0, places[0].1));
        entry.0 += 1;
    }
    let mut ranked: Vec<_> = shared.into_iter().collect();
    ranked.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
    ranked
        .into_iter()
        .map(|(paths, (count, line))| {
            let message = format!("{count} shared block(s) of {BLOCK_LINES} lines (the first from line {line})");
            Finding::new(Level::Warning, "duplicates", paths.join(" + "), message)
        })
        .collect()
}
