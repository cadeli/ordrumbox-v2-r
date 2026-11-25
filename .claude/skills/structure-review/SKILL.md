---
name: structure-review
description: Review of structure against AGENTS.md (decoupling, reuse, naming, errors and idioms, file size), of the whole workspace, of some crates, or of what changed since a commit, written up as docs/revue-AAAA-MM.md in French with findings and proposed lots, then fixed lot by lot on the user's go-ahead. Use when the user asks for a structure, architecture or clean-code review of the whole repository (« revue », « revue de structure »), or to carry out a lot of an existing review. Not for a diff or a PR: that is /code-review.
disable-model-invocation: true
---

# Structure review

A review of the workspace (or of the scope below) against the rules of `AGENTS.md`, done the
way `docs/revue-2026-11.md` was done. Reply to the user in French; the report
is in French; code, comments and names stay in English.

## Rules of the job

- **Change no code while reviewing.** The review produces a report. Fixes
  come after, one lot at a time, each on the user's go-ahead.
- **Never commit** unless the user asks.
- **No finding without evidence**: each one names files with `path:line`,
  counts (how many copies, how many callers) and the rule of `AGENTS.md` it
  breaks, quoted by its section. Read the code at the place before writing
  the finding; an inventory hit alone is not a finding.
- **What the tool already says, say once.** `cargo xtask audit --all` checks
  the crate graph, names, `#[allow]`, globals, re-exports, file sizes,
  nesting, duplicates and clippy's measures. Report its counts in the
  header; spend the review on what it cannot judge.
- **Decisions stand.** An item an earlier review marked done, decided or
  « laissé tel quel, voulu » is not raised again unless the code changed
  since in a way that reopens it; then say what changed.

## Scope

The argument picks what is reviewed; the header of the report names it.

| Argument | Scope |
|---|---|
| none | the whole workspace |
| `depuis <commit>` | the files changed since that commit |
| `depuis la dernière revue` | the same, from the base commit of the latest review in `docs/revue-*.md` |
| one or more crate names (`ordrumbox-engine`, `engine`) | those crates only |
| `lot <n>` | carry out that lot of the latest review (section 4), no review |

The two can combine (`engine depuis a1b2c3d`). Anything else: ask.

In a partial scope (by commit or by crate):

- The files in scope are the ones read and inventoried: for a commit,
  `git diff --name-only <commit>..HEAD -- crates/` without the test files
  (`tests/`, `*_tests*`, `tests.rs`); for a crate, its `src/`.
- A rule that spans files is still checked against the whole workspace, from
  what is in scope: a name or a function defined in scope is looked for
  everywhere (`rg`), a duplicate may have its other copy outside, a field
  made wider in scope is checked against all its readers. A finding may
  then name a file out of scope.
- A finding of an earlier review that is still open and lies in scope is
  listed again only by its number and status (« toujours ouvert »,
  « aggravé : … »), not rewritten.
- The **Limite** of the header says what was not looked at: the other
  crates, or the files not changed.

## 1. Start

1. `git rev-parse --short HEAD` and `git status --short`: the base of the
   review, and whether the tree is clean (if not, say so and ask whether to
   review it as it is). For `depuis …`, check that the commit exists and
   list the files in scope with their changed line counts
   (`git diff --stat`); if none, say so and stop.
2. Read `AGENTS.md`, sections "Layout", "Structure: do and do not", "Code
   conventions", "Errors", "Modern Rust".
3. Read every earlier `docs/revue-*.md`: its findings, its decisions, its
   lots. Note what is still open.
4. `cargo xtask audit --all`, and `wc -l` over `crates/*/src`: lines per
   crate, the largest files.

## 2. Mechanical inventory

Leads, not findings. The inventory is split into parts, one Explore
subagent per part, all started in the same message so they run in
parallel. For the whole workspace, these parts (at `940a706`,
`ordrumbox-ui` alone is about 46 000 lines, too much for one):

| Part | Files |
|---|---|
| core + dsp | `crates/ordrumbox-core/src`, `crates/ordrumbox-dsp/src` |
| logic | `crates/ordrumbox-logic/src` |
| engine | `crates/ordrumbox-engine/src` |
| ui — session | `crates/ordrumbox-ui/src/session*.rs`, `session_mcp/` |
| ui — views | `view/`, `editors/`, `menu/`, `panel/`, `piano_roll/`, `song_view/`, `ui/`, `kits/` of `crates/ordrumbox-ui/src` |
| ui — rest | the other files of `crates/ordrumbox-ui/src` (`store.rs`, `mcp*`, `update.rs`…) |
| app + tools | `crates/ordrumbox-app/src`, `crates/ordrumbox-probe/src`, `crates/xtask/src` |

Test folders (`assign_tests/`, `session_scenarios/`, `tests/`) are not
inventoried. A part past about 15 000 lines of code is split again along
its folders. If the layout changed (a new crate, a moved folder), adapt
the table and say so in the header. In a partial scope, keep only the parts
with files in scope; fewer than about 3 000 lines in all, do the inventory
in this session without subagents.

Each subagent gets: its files, the list below, the sections of `AGENTS.md`
it applies, and the earlier findings already decided (not to raise again).
It reads code only where a lead needs it, edits nothing, and returns its
leads one per line, in this form, and nothing else:

```
<family A|B|N|E|T> | <path:line> | <rule of AGENTS.md, by section> | <what is there, with counts> | <cross-part: names to look for elsewhere, or ->
```

The cross-part column carries what one part cannot judge alone (a function
name also defined elsewhere, a field read from another crate, a synonym):
this session checks those across the workspace with `rg`, then reads the
code at every lead it keeps before writing it as a finding.

What to look for:

- **Decoupling**
  - For each `impl Session` file: which fields of `Session` it touches.
    Files that touch many, methods that read only two or three fields
    ("depend on the narrowest thing").
  - Fields `pub`, `pub(crate)`, `pub(super)` read from another module
    ("do not reach into another module's fields").
  - Domain logic in `ordrumbox-ui` that needs no window, sound or file (it
    belongs in `core`, `logic` or `engine`).
  - A view that mutates the model instead of returning a command.
- **Reuse**
  - Function names defined in more than one crate or module.
  - Conversions and constants recoded: dB ↔ gain, beats per measure, step
    ↔ tick, clamps, hit tests, layout rects (`rg` the literal: `20.0 *`,
    `log10`, `powf`, `/ 4`, `* 4`).
  - A helper that exists but is bypassed by inline copies.
  - A `bool` parameter where an `enum` exists or two functions would do.
- **Naming**
  - The domain vocabulary across crates: one concept, one name (`rg -i`
    each term of "Code conventions" and its synonyms: `song`/`arrangement`,
    `step`/`slot`, `volume`/`gain`/`level`, `chance`/`probability`,
    `bar`/`measure`, `style`/`genre`).
  - Lengths and durations without their unit; counts without `_count`.
- **Errors and idioms** ("Errors", "Modern Rust")
  - A failure swallowed: `let _ =` on a `Result`, `.ok()` or
    `unwrap_or_default()` on I/O or parse, an error sent to `eprintln!`
    instead of an `Issue`, `status` or `Session::message`.
  - A clamp or fallback that silently changes what the user asked for.
  - Sentinels (`-1`, `u8::MAX`, `NaN`, `""`) standing for "none" instead
    of an `Option`; magic integers or `bool` pairs instead of an `enum`.
  - `as` where `From` or `try_from` says the intent; index loops where
    `zip`, `chunks_exact` or `windows` fit.
- **Size and seams**
  - Files near or past 500 lines of code; for each, whether it holds two
    ideas (its first doc line needs "and", or its functions fall into two
    groups that do not call each other).

## 3. The report

Write `docs/revue-<YYYY>-<MM>.md` (the current month). If it exists, add a
new part to it, headed `# Revue du <YYYY-MM-DD>`, and **continue its
numbering**: findings go on from the highest number of their family in the
file (after N9 comes N10), lots from the highest lot (after lot 9 comes lot
10). A number is never reused within a file, so « voir B5 » or « Lot 3 »
always points to one place. Same shape as `docs/revue-2026-11.md`:

1. **Title and header**: scope (whole, `depuis <commit>`, crates), base
   commit, size of the code in scope, the audit's counts,
   the method (what was inventoried, what was read), and the limit (what
   was not read line by line).
2. **Ce qui est sain**: what holds and should stay so, briefly.
3. **Findings**, numbered within each family (A1, B1, N1… in a new file;
   continued in an existing one, see above):
   - **A — Découplage**, **B — Réutilisation**, **N — Nommage**,
     **E — Erreurs et idiomes**, **T — Taille et coutures** (a file to
     split: where the seam is and what each part would hold);
   - a family with no finding is left out, not written empty;
   - each with a gravity: **(A)** structural, **(B)** a rule of `AGENTS.md`
     broken, **(C)** minor;
   - each says: what is there (with `path:line` and counts), the rule
     broken, the proposed fix, and its cost (files touched, risk to
     behaviour or snapshots).
4. **Ordre proposé**: lots, small and independent first; each lot one
   coherent change that keeps the tests green on its own.
5. **Règles d'audit possibles**: findings a tool could keep from coming
   back (a pattern on one line, a name), for `crates/xtask/src/audit/`.

Then stop and give the user a short summary in French: the number of
findings per family and gravity, the two or three that matter most, and the
first lot proposed. Offer the report as an artifact only if they ask.

## 4. Carrying out a lot

Only on the user's go-ahead (`lot <n>` as the argument is one), one lot at
a time:

1. Make the change. Follow `AGENTS.md` for everything touched (no internal
   `pub use`, visibility as narrow as works, file doc lines kept true).
2. Check, in this order (formatting first, so what is checked is what
   will be committed), and report the results as they are:
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets
   cargo xtask audit
   cargo test --workspace
   ```
   If the lot touches a render path (`ordrumbox-dsp`, `Engine::render`, or
   anything reached from the audio callback), also `cargo test --release
   --workspace`: the audio snapshots must stay within 2e-5, and the time is
   judged in release ("Audio thread" in `AGENTS.md`).
   A snapshot that changes is a change of behaviour: stop and ask, unless
   the lot meant it (then `DEVELOPMENT.md`, "Changes of behaviour").
3. Append to the report a section `## Lot <n> — fait (<what>)`: what
   changed, what was left on purpose and why, behaviour, checks. Mark the
   finding done in its own section and in the table of lots.
4. Do not commit. Tell the user the lot is ready, and what comes next.
