## What and why

## Checklist

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` pass
- [ ] `cargo xtask audit` passes
- [ ] A change of behaviour (a snapshot rewritten) is meant and noted in `DEVELOPMENT.md`, "Changes of behaviour"
- [ ] Nothing reached from the audio callback allocates, takes a blocking lock or does I/O
- [ ] No layout shift in the UI; `README.md` and `CHANGELOG.md` updated if the user sees the change
