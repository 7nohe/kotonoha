# Developer documentation

[日本語](./DEVELOPMENT.ja.md) | [User docs](../README.md)

## Stack

- **App**: Tauri 2 + React + TypeScript (Vite)
- **STT**: whisper.cpp via [whisper-rs](https://github.com/tazz4843/whisper-rs) (Metal), Silero VAD (whisper.cpp built-in)
- **Translation / summary**: [Ollama](https://ollama.com) HTTP API (`localhost:11434`, NDJSON streaming)
- **System audio**: Core Audio process taps via [cidre](https://github.com/yury/cidre) (macOS 14.4+) with automatic fallback to ScreenCaptureKit ([screencapturekit-rs](https://github.com/svtlabs/screencapturekit-rs))
- **Microphone**: cpal
- **Overlay**: [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel) non-activating floating panel

## Setup

Requirements: Node.js 22+, Rust 1.77.2+, CMake (`brew install cmake`), Xcode Command Line Tools.

```sh
npm install
npm run tauri dev      # development (watches Rust + frontend)
npm run tauri build    # release .app → src-tauri/target/release/bundle/macos/
cargo test --manifest-path src-tauri/Cargo.toml
```

## Architecture

```
mic (cpal) ──────────┐  RT callbacks: lock-free rtrb ring buffers only
system (CATap/SCK) ──┤
                     ▼
        pipeline worker thread (per source)
        downmix → 16kHz resample (rubato) → Silero VAD segmenter
        (~1.5s PARTIAL jobs while speaking; FINAL on 600ms silence or 12s cap)
                     ▼
        single whisper thread (Metal, serialized inference,
        stale partials coalesced away)
                     ▼ "transcript" events
        ┌────────────┴────────────┐
        ▼                         ▼
   overlay webview        translation queue (tokio)
   (React captions)       → Ollama streaming → "translation" events
                          → history (export / summary)
```

Key modules under `src-tauri/src/`:

| Module | Role |
|---|---|
| `pipeline.rs` | Capture-backend wiring, worker threads, partial/final job scheduling |
| `audio/` | mic (cpal), system (SCK), system_catap (Core Audio taps), resampler |
| `stt/` | whisper engine thread, VAD segmenter, model catalog & downloads |
| `translate/` | Ollama client (chat/streaming/summary), translation queue |
| `overlay.rs` | NSPanel conversion, window level/collection behavior, positioning |
| `history.rs` | Session transcript accumulation, Markdown export |

## Code signing during development

`tauri.conf.json` ships with `"signingIdentity": "-"` (ad-hoc) so the project builds out of the box. macOS ties privacy permissions (TCC) to the code signature, so ad-hoc builds re-prompt for permissions after every rebuild. With a signing certificate:

```sh
export APPLE_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)"
```

Permissions then persist across rebuilds.

## Hard-won notes (read before touching these areas)

- **Overlay window**: do **not** set `alwaysOnTop` / `visibleOnAllWorkspaces` in `tauri.conf.json` — Tauri re-applies the collection behavior after setup and wipes `fullScreenAuxiliary` (the flag that keeps the panel above full-screen apps). All panel configuration lives in `src-tauri/src/overlay.rs`. Window positioning must be done in logical coordinates; mixing physical pixels across monitors with different scale factors places the window off-screen.
- **whisper.cpp VAD**: `WhisperVadContext` slows down linearly with each call, so the segmenter recreates it periodically (`stt/segmenter.rs`).
- **Quit path**: the whisper thread must be joined before process exit — tearing down while the ggml Metal context is alive aborts in static destructors (`commands::shutdown`).
- **Swift rpath**: the screencapturekit crate embeds a Swift bridge; the binary needs an rpath to `/usr/lib/swift` (`build.rs`).
- **Dev profile**: native whisper/DSP crates are compiled with `opt-level = 3` even in dev — debug-level optimization is too slow for realtime audio (`Cargo.toml` profile overrides).
- **CATap vs SCK**: the Core Audio tap backend avoids the Screen Recording permission entirely (and macOS 15's periodic re-confirmation), but silently falls back to ScreenCaptureKit when tap creation fails. Which backend is active is logged as `[audio] system backend: ...`.

## Releasing

Pushing a `v*` tag triggers the [release workflow](../.github/workflows/release.yml), which builds a `.dmg` on an Apple Silicon runner and attaches it to a **draft** GitHub Release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Review the draft on the Releases page and publish it.

Without secrets the artifact is ad-hoc signed. For signed & notarized builds, set these repository secrets:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | base64-encoded `.p12` of a "Developer ID Application" certificate |
| `APPLE_CERTIFICATE_PASSWORD` | password of the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | Apple ID, app-specific password, and team ID for notarization |

## Design assets

`design/logo.svg` (app icon source) and `design/tray.svg` (menu-bar template glyph). After editing:

```sh
# App icon: render via qlmanage (ImageMagick breaks SVG gradients), mask to squircle
qlmanage -t -s 1024 -o /tmp design/logo.svg
magick -size 1024x1024 xc:none -fill white -draw "roundrectangle 0,0 1023,1023 230,230" PNG32:/tmp/mask.png
magick /tmp/logo.svg.png -background none -gravity center -extent 1024x1024 /tmp/mask.png -alpha off -compose CopyOpacity -composite PNG32:app-icon.png
npm run tauri icon app-icon.png   # regenerates src-tauri/icons/ (tray.png is kept)
```

The menu-bar glyph (`src-tauri/icons/tray.png`) must be **black + alpha only** (macOS template image); render it with ImageMagick native drawing, not from SVG.
