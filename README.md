# Wave

A lightweight Discord client in Rust and gpui (repository and data folders still use its first name, Rustcord).

A lightweight Discord desktop client in Rust and [gpui](https://www.gpui.rs): messaging, voice, video and Go Live.

Personal project. `assets/discord/` holds Discord's own icons and sounds, taken for personal use only: keep this
repository private and never ship a build that contains them (see `assets/discord/README.md`).

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

## Where things are

- `src/` app: `main.rs` state, `layout.rs` frame, `sidebar.rs`, `chat.rs`, `panel.rs`, `call.rs`, `settings.rs`
- `src/voice/` voice, video and Go Live engine
- `PARITY.md` what matches Discord and what doesn't yet
