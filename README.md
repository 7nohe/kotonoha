# kotonoha 🍃

[日本語](./README.ja.md)

A fully-local realtime transcription & translation overlay for macOS.

Launch kotonoha alongside any meeting tool and it shows floating captions: realtime Japanese transcription for Japanese meetings, and live Japanese translation for English meetings. No audio ever leaves your machine — everything runs locally.

## Features

- **Floating caption overlay** — a non-activating panel that stays above every app (including full-screen meeting apps) without stealing focus. Draggable, click-through mode, auto-fades when nobody is speaking
- **Realtime transcription** — whisper.cpp (large-v3-turbo) with Metal acceleration, Silero VAD utterance segmentation, partial results within ~2s
- **Live translation** — English speech is translated to Japanese via a local Ollama model, streamed token-by-token under the original text
- **Dual audio sources** — system audio (the other participants) and your microphone, transcribed in parallel
- **Meeting minutes export** — dump the session transcript to Markdown in `~/Downloads`, optionally with an Ollama-generated summary (key points / decisions / TODOs)
- **Fully local** — no cloud, no API keys, no telemetry

## Architecture

- **App**: Tauri 2 + React + TypeScript
- **STT**: whisper.cpp via [whisper-rs](https://github.com/tazz4843/whisper-rs) (Metal), Silero VAD (whisper.cpp built-in)
- **Translation / summary**: [Ollama](https://ollama.com) (`localhost:11434`, streaming)
- **System audio**: Core Audio process taps (macOS 14.4+, no screen-recording permission) with automatic fallback to ScreenCaptureKit
- **Microphone**: cpal
- **Overlay**: [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel) non-activating floating panel

## Getting started

### Requirements

- macOS 13.3+ (Apple Silicon recommended; Core Audio tap backend needs 14.4+)
- Node.js, Rust 1.77.2+, CMake (`brew install cmake`), Xcode Command Line Tools

### Build & run

```sh
npm install
npm run tauri dev      # development
npm run tauri build    # release .app (src-tauri/target/release/bundle/macos/)
```

On first launch an onboarding flow walks you through granting permissions and downloading the speech model in-app.

### Ollama (optional, required for translation & summaries)

```sh
brew install --cask ollama-app   # official app — the brew formula build may
                                 # ship without llama-server and fail to run
ollama serve                     # or launch Ollama.app
ollama pull qwen2.5:3b-instruct  # small, fast model that translates well
```

Switch the language direction to "英語→日本語" (English → Japanese) in Settings and each finalized English utterance is translated in a stream. Transcription keeps working even when Ollama is down.

## Usage

1. Control everything from the menu-bar icon: show/hide overlay, click-through, export minutes, settings, quit
2. Drag the overlay anywhere; hover to reveal controls (capture toggle, click-through, settings)
3. Click-through lets you interact with apps beneath the captions (turn it off from the menu bar)
4. **Headphones recommended** — with speakers your mic also picks up the other participants

## System audio capture

1. **Core Audio process tap** (macOS 14.4+) — uses the lightweight "System Audio Recording" permission instead of Screen Recording; tried first
2. **ScreenCaptureKit** — automatic fallback when the tap is unavailable (macOS 13.x, permission denied, …)

The chosen backend is logged as `[audio] system backend: ...`.

## Code signing (development)

`tauri.conf.json` ships with `"signingIdentity": "-"` (ad-hoc) so the project builds out of the box. Note that macOS ties privacy permissions (TCC) to the code signature, so with ad-hoc signing every rebuild re-prompts for permissions. If you have a signing certificate, export it once in your shell and rebuild:

```sh
export APPLE_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)"
```

Permissions then persist across rebuilds.

## Development notes

- Do **not** set `alwaysOnTop` / `visibleOnAllWorkspaces` in `tauri.conf.json` for the overlay window — Tauri re-applies the collection behavior after setup and wipes `fullScreenAuxiliary` (the flag that keeps the panel above full-screen apps). All panel configuration lives in `src-tauri/src/overlay.rs`
- whisper.cpp's `WhisperVadContext` slows down linearly with each call, so the segmenter recreates it periodically (`stt/segmenter.rs`)
- The screencapturekit crate embeds a Swift bridge, so the binary needs an rpath to `/usr/lib/swift` (`build.rs`)
- In dev builds the native whisper/DSP crates are compiled with `opt-level = 3` — debug-level optimization is too slow for realtime audio

## License

[MIT](./LICENSE)
