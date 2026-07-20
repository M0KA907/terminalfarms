# Terminal Farms — Claude Code guide

Terminal Farms is a local-first Rust 2024 terminal farming game (Rust 1.88+). It has no
network, telemetry, or update path, and it must stay that way. This file is the
repository-level working contract for AI-assisted changes. The private vault in
`COMPONENTS/` may contain a newer, more detailed development guide; when both exist,
read both before editing, and treat the more recent instruction as authoritative.

## Architecture

Keep each concern in its home module. Moving logic across these boundaries is a
refactor that needs its own justification, not a side effect of a feature patch.

- `src/game.rs` — deterministic economy and state transitions: tiles, crops, seeds,
  produce, land expansion, upgrades, automation, rebirth, growth staging. Pure logic;
  no I/O, no clocks. Time always arrives as explicit elapsed seconds or timestamps.
- `src/storage.rs` — SQLite persistence: schema, append-only migrations with
  pre-migration backups, validation, transactional load/save, WAL setup.
- `src/data.rs` — typed, validated catalogs parsed from embedded TOML.
- `src/main.rs` — CLI, data-directory resolution, event loop, input routing, ticks,
  offline progress, autosave, logging, terminal restoration.
- `src/ui.rs` — Ratatui rendering, compatibility layout, color fallback, viewport
  math, and mouse hit testing. Hit-test functions must derive geometry from the same
  helpers the renderers use (`viewport_capacity`, `shop_crop_capacity`,
  `visible_shop_crops`) so clicks always land on what is drawn.
- `assets/crops.toml`, `assets/upgrades.toml` — compile-time balance data, embedded
  via `include_str!` and validated at startup.
- `src/lib.rs` — the minimal public surface the binary consumes. Do not grow it
  unless another target genuinely needs a symbol.

Data lives in the platform `io/M0KA907/TerminalFarms` local data directory;
`--portable` and `--data-dir DIR` override it. The app may create `terminalfarms.db`,
WAL files, timestamped migration backups, and an opt-in `terminalfarms.log`.

## Gameplay contract

Preserve these unless the user explicitly requests a product change, and cover any
requested change with new regression tests:

- A new farm: 20 coins, a 3x3 untilled field, three Radish seeds.
- The emergency Radish seed makes a zero-money softlock impossible.
- Crop and machine unlocks gate on run earnings, not current cash.
- The crop catalog holds at least 50 crops, ordered by non-decreasing
  `unlock_earnings`, every crop strongly profitable
  (`sell_price >= 3 * seed_price`), every art frame exactly 3 visible characters
  wide across all 6 growth stages. Growth times rise monotonically through the
  catalog and never exceed 9,000 seconds.
- Machines have no level cap; the next level always costs
  `base_price * (level + 1)`, timed machines run at `interval_seconds / level`,
  and each cycle performs one action per level (bounded by the per-tick action
  cap).
- Active growth uses the active-time multiplier; offline growth runs before offline
  machinery and must not inherit the active-only bonus.
- Land expansion preserves every existing tile.
- Rebirth resets run-scoped state and increases permanent rebirth progression.
- Reset stays a two-step confirmation; unrelated input disarms it.
- The shop crop list is a scrolling window: selection stays visible when cycling
  crops, the wheel scrolls it when the pointer is over the shop, and the machine,
  land, rebirth, and reset rows stay pinned below the window.
- State-changing input and automation trigger saves; autosave runs periodically and
  a final save runs on exit.
- Loaded saves validate against field dimensions and both embedded catalogs; saves
  are all-or-nothing SQLite transactions with bound parameters.
- Schema versions are append-only: newer schemas fail safely, older schemas are
  backed up before migration.
- Raw mode, the alternate screen, and mouse capture are restored on every exit path.
- `--compat`, small terminals, `--no-color`, `NO_COLOR`, and `TERM=dumb` stay usable.

## Security and safety ratchet

Guarantees only move forward; never weaken a guard to make a test or feature pass.

- No `unsafe`. No new dependencies, network access, subprocesses, telemetry, or
  dynamic code/data loading without explicit user approval.
- SQL stays parameterized; never interpolate values or identifiers into SQL.
- Treat saves, timestamps, terminal sizes, CLI paths, environment variables, and
  input events as untrusted: bound or reject them without panics, huge allocations,
  or wraparound. Keep checked conversions and saturating economy arithmetic.
- No new `unwrap`, `expect`, `panic!`, `unreachable!`, or raw indexing on data that
  can originate outside the binary.
- Floating-point growth and timer changes must consider zero, negative, non-finite,
  very large, and clock-rollback inputs.
- Never point tests or dev runs at a real user data directory; use the existing
  temporary-database pattern or a disposable `--data-dir`.
- Never delete or rewrite a user database as a recovery shortcut; prefer a clear
  error, and keep migrations transactional and backed up.
- Logging stays opt-in, local, append-only, and minimal.
- Keep terminal cleanup guards intact across every early return and error path.

## Testing rules

The suite is a ratchet: the branch state at task start is the floor. Currently 32
tests across library and binary targets.

- Never delete, ignore, or relax a passing test to accommodate a change. If an
  expectation intentionally changes, say why before editing it.
- Bug fixes start with a test that fails for the reported bug.
- Behavior changes add or update a test stating the new contract.
- Schema changes add migration coverage from the previous schema plus a round trip.
- Catalog changes keep the catalog validation tests green and add a gameplay
  assertion when balance, unlock order, timing, or automation changes.
- Input or layout changes cover keyboard and mouse routing plus compatibility
  rendering where relevant; keep hit tests aligned with render geometry.
- Tests are deterministic: explicit timestamps and elapsed seconds, `TestBackend`
  for UI, no sleeps, no wall clocks, no real home directories.

Test placement mirrors the module map: catalog invariants in `data.rs`, economy and
automation in `game.rs`, persistence in `storage.rs`, orchestration and routing in
`main.rs`, rendering and hit tests in `ui.rs`.

## Change workflow

Compress each request before editing: exact outcome, smallest files in scope,
explicit non-goals (no unrelated refactors, dependency changes, balance drift, or
save breakage), and the proof you will run. Then, per reviewable step:

1. Check `git status --short` and the relevant code and tests; preserve unrelated
   user changes.
2. Add one focused regression test and watch it fail for the intended reason.
3. Make the smallest implementation change; no drive-by cleanup in the same step.
4. Rerun the focused test, then the module tests, then the full suite. Do not stack
   new work on a red suite.

Required checks before claiming completion:

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

For CLI-facing changes also run `cargo run -- --help`. For interactive smoke tests
use a fresh disposable `--data-dir` and try both normal and `--compat --no-color`
modes. Never claim a check you did not run; if one cannot run, report the exact
command, the failure, and the remaining risk.

## Patch discipline

- Smallest safe change that satisfies the request; no broad formatting, module
  moves, mass renames, or balance edits alongside a fix.
- Embedded asset IDs are save-stable. Renaming or removing a crop or upgrade ID is
  a save migration, not a cosmetic edit; appending new entries is the safe path.
- Balance data changes belong in `assets/*.toml`; generator scripts and scratch
  work stay out of the repository.
- Never hand-edit `Cargo.lock`.
- Do not commit generated databases, logs, backups, `target/`, or anything under
  `COMPONENTS/`.
- Do not commit or push unless explicitly asked.

## COMPONENTS/ vault

`COMPONENTS/` is a private, git-ignored Obsidian vault holding the extended agent
guide and planning documents. Never stage or commit it — not even with
`git add -f` — and keep `/COMPONENTS/` root-anchored in `.gitignore`. Ignored is
not encrypted: no credentials, tokens, keys, or real user save data belong there.
New planning docs go in `COMPONENTS/Planning/` with a date-prefixed filename.

## Reporting

Finish every task by checking `git status --short`, `git diff --check`, and the
diff itself, confirming only intended files changed and `COMPONENTS/` is still
untracked. Report four sections: `Changed`, `Verified`, `Risks`, `Rollback`, with
non-destructive rollback instructions and a warning before any rollback that could
touch user data.
