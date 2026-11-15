# Wave

A lightweight Discord client in Rust and gpui (the crate and data folders still use its first name, Rustcord).

A lightweight Discord desktop client in Rust and [gpui](https://www.gpui.rs): messaging, voice, video and Go Live.

Personal project. `assets/discord/` holds Discord's own icons and fonts, kept for reference only: no build compiles
them in, and none may (see `assets/discord/README.md`).

## Build

Rust stable (pinned by `rust-toolchain.toml`).

```sh
cargo run
```

Sign in by scanning the QR code with the Discord mobile app.

### Windows

Not yet built or tested on Windows. Expected needs:

- [Rust](https://rustup.rs) with the MSVC toolchain
- Visual Studio Build Tools with "Desktop development with C++"
- [CMake](https://cmake.org/download/) on `PATH` (Opus is built from source)

Known gaps on Windows: Go Live (screen capture is macOS-only), sound cues (macOS-only), and debug sign-ins are
saved under `%APPDATA%\Rustcord`.

## How to release

Release builds update themselves at launch from this repository's GitHub Releases (debug builds and
`CHORD_DEMO` runs never do).

1. Bump `version` in `Cargo.toml` (e.g. `0.2.0`), run `cargo check` so `Cargo.lock` follows, and commit.
2. Tag that commit with the same version and push it:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

`.github/workflows/release.yml` checks the tag matches `Cargo.toml`, creates the release, and attaches, each with
a `.sha256`:

- installers: `wave-macos-aarch64.dmg`, `wave-macos-x86_64.dmg`, `wave-windows-x86_64-setup.exe` (per-user, no admin)
- what the updater downloads: `wave-macos-*.zip` (zipped `Wave.app`) and `wave-windows-x86_64.exe`

Running copies older than the tag download theirs on next launch and restart into it.

## Where things are

- `src/` app: `main.rs` state, `layout.rs` frame, `sidebar.rs`, `chat.rs`, `panel.rs`, `call.rs`, `settings.rs`
- `src/voice/` voice, video and Go Live engine
- `PARITY.md` what matches Discord and what doesn't yet
