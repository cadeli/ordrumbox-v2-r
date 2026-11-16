//! `cargo xtask audit [--strict] [--all] [--no-clippy]`: the rules of
//! AGENTS.md that a tool can check, over every crate.
//!
//! An *error* is a rule that holds today and must keep holding (the crate
//! graph, the names, `#[allow]`); it fails the audit. A *warning* is a
//! measure with known exceptions (a file or function that is too long, deep
//! nesting, duplicated code); it fails only with `--strict`. A file that
//! is exempt from a rule on purpose when it carries the line
//! `// audit: allow(<rule>): <why>` (the rule is the name the report gives).

mod clippy;
mod duplicates;
mod graph;
mod rules;
mod sources;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Error,
    Warning,
}

/// One thing the audit found, where, and what to do about it.
#[derive(Debug)]
struct Finding {
    level: Level,
    rule: &'static str,
    place: String,
    message: String,
}

impl Finding {
    fn new(level: Level, rule: &'static str, place: impl Into<String>, message: impl Into<String>) -> Self {
        Self { level, rule, place: place.into(), message: message.into() }
    }
}

/// How many findings of a rule the report lists, without `--all`.
const LISTED: usize = 8;

/// Run the audit with the arguments after `audit`.
pub fn run(args: &[String]) -> io::Result<()> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let (strict, all) = (has("--strict"), has("--all"));
    let root = workspace_root()?;
    let files = sources::load(&root)?;

    let mut findings = graph::check(&root)?;
    findings.extend(rules::check(&files));
    findings.extend(sources::size_findings(&files));
    findings.extend(duplicates::check(&files));
    if !has("--no-clippy") {
        findings.extend(clippy::check(&root)?);
    }

    findings.retain(|f| !exempt(&files, f));
    report(&findings, all);
    let failed = findings.iter().any(|f| f.level == Level::Error || (strict && f.level == Level::Warning));
    if failed { Err(io::Error::other("the audit found problems")) } else { Ok(()) }
}

/// Whether the file of `finding` carries an `audit: allow(<rule>)` line.
fn exempt(files: &[sources::Source], finding: &Finding) -> bool {
    let path = finding.place.split(':').next().unwrap_or_default();
    let marker = format!("audit: allow({})", finding.rule);
    files
        .iter()
        .any(|f| f.path == path && f.lines.iter().any(|l| l.trim_start().starts_with("//") && l.contains(&marker)))
}

/// The folder that holds the workspace's `Cargo.toml`, from the current one up.
fn workspace_root() -> io::Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|manifest| manifest.contains("[workspace]")) {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(io::Error::other("no workspace found above the current folder"));
        }
    }
}

/// `path` as the report shows it: from the workspace root.
fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// Print the findings by rule, then the totals.
fn report(findings: &[Finding], all: bool) {
    let mut by_rule: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in findings {
        by_rule.entry(finding.rule).or_default().push(finding);
    }
    for (rule, list) in &by_rule {
        let label = if list.iter().any(|f| f.level == Level::Error) { "ERROR" } else { "warn " };
        println!("\n[{label}] {rule}: {}", list.len());
        for finding in list.iter().take(if all { usize::MAX } else { LISTED }) {
            println!("  {}  {}", finding.place, finding.message);
        }
        if !all && list.len() > LISTED {
            println!("  … {} more (--all lists them)", list.len() - LISTED);
        }
    }
    let count = |level| findings.iter().filter(|f| f.level == level).count();
    println!("\naudit: {} error(s), {} warning(s)", count(Level::Error), count(Level::Warning));
}
