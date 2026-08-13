# WP0 — 开源仓库骨架

Read `docs/design.md` and this brief. Do not implement iperf3 or Wi‑Fi.

## Where this fits

First work package of the C6 iperf3 server. Later packages add `iperf3-proto` behavior (WP1) and firmware (WP2). You only create the open-source skeleton so those packages have a Cargo workspace and legal files.

## Work from

`/Users/majoson/CodeSpace/esp32-c6-iperf3`

Existing files (keep, do not rewrite unless a tiny link is needed):

- `docs/design.md`
- `docs/superpowers/plans/2026-08-14-esp32-c6-iperf3.md`
- `.superpowers/sdd/` (controller files; do not delete)

The directory is not a git repo yet. `git init` on `main`, then commit.

## Create

- `.gitignore`: `target/`, `.env`, `*.pyc`, `.idea/`, `.vscode/` optional, `Thumbs.db`. Must ignore `.env`.
- `.env.example`: placeholder `SSID=your-2.4ghz-ssid` and `PASSWORD=your-wpa2-password` only. Never write a real password or the real lab SSID.
- `LICENSE-MIT` and `LICENSE-APACHE` (standard MIT + Apache-2.0 text). Copyright holder: `majoson-chen`.
- `rust-toolchain.toml`: stable channel plus target `riscv32imac-unknown-none-elf`.
- Workspace `Cargo.toml`:
  - workspace members: `iperf3-proto`, `firmware`
  - resolver `"2"`
  - workspace package metadata where it helps: edition `2024`, license `MIT OR Apache-2.0`, repository `https://github.com/majoson-chen/esp32-c6-iperf3`
- Stub crate `iperf3-proto`: library, `#![no_std]` in `src/lib.rs`, empty public surface except a compiling placeholder (e.g. `pub fn proto_version() -> &'static str` returning `"0.1.0"` is enough). `dev-dependencies` may use `std` tests later; a single host test that `proto_version()` is non-empty is OK.
- Stub crate `firmware`: do **not** pull `esp-hal` yet. Make it a `no_std` binary that will not be built in WP0 CI. Prefer `publish = false`. A `src/main.rs` that is clearly a stub (comment: Wi‑Fi/iperf land in WP2) is fine; if a host `cargo test` of the workspace would fail because firmware has no `std`/`main` for host, configure workspace so `cargo test -p iperf3-proto` works. Default-members can be `["iperf3-proto"]`.
- Files you create: top-of-file comment with author `Cursor Grok 4.6` and a one-line purpose (user rule). LICENSE files are standard legal text — do not add that header there.

## Do not

- Implement protocol or Wi‑Fi
- Add README (WP4)
- Add GitHub Actions (WP4)
- Commit secrets
- Push to GitHub
- Follow `esp-hal` git `main` APIs

## Verify

- `cargo test -p iperf3-proto` passes on host
- `.env` is gitignored (prove with `git check-ignore -v .env` after init, or equivalent)
- No real SSID/password in any file

## Commit

Conventional Commits, e.g. `chore: scaffold workspace, licenses, and crate stubs`.

You may make one or two commits, not a dump of unrelated concerns.

## Report

Write full report to `.superpowers/sdd/wp0-report.md` (TDD evidence only if you added a real test; config stubs may note TDD N/A). Return status DONE / DONE_WITH_CONCERNS / BLOCKED / NEEDS_CONTEXT, commits, one-line test summary, report path.
