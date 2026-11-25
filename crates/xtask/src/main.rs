//! `cargo xtask audit`: the checks of AGENTS.md (`audit.rs`).

use std::io;

mod audit;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, rest)) if command == "audit" => audit::run(rest),
        _ => Err(io::Error::other("usage: cargo xtask audit [options]")),
    }
}
