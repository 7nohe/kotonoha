---
name: e2e-audio-check
description: End-to-end verification of kotonoha's transcription/translation pipeline and overlay without a human speaker. Use after changing the audio pipeline, whisper/VAD code, overlay window behavior, or before a release — plays macOS TTS as test audio and inspects logs, window state, and screenshots.
---

# E2E audio & overlay check

Verifies the full chain (capture → VAD → whisper → [Ollama] → overlay) using `say` as the audio source. The system-audio path picks TTS up directly (SCK/CATap tap other processes' audio); the mic path picks it up via the speakers.

## Setup

1. Launch the app so stderr is captured (release `.app` swallows logs when launched via `open`):
   ```sh
   src-tauri/target/release/bundle/macos/kotonoha.app/Contents/MacOS/kotonoha > /tmp/kotonoha_run.log 2>&1 &
   ```
   For dev, `npm run tauri dev` output serves the same purpose.
2. Sound check: `osascript -e 'get volume settings'` — **save the values and restore them after the test**. Unmute to ~20 for the test (`set volume output volume 20 without output muted`). Note: a muted output also silences what the capture taps.
3. Which system backend is active: `grep '\[audio\] system backend' /tmp/kotonoha_run.log` (CATap vs ScreenCaptureKit fallback).

## Japanese transcription check

```sh
say -v Flo "こんにちは。これはコトノハのテストです。リアルタイム文字起こしの動作を確認しています。"
sleep 6 && grep '\[stt\]' /tmp/kotonoha_run.log | tail -5
```

Expect: `partial` lines every ~1.6s while speaking, then a `final` shortly after the utterance ends. No `[stt]` lines during silence (hallucination check).

## English → Japanese translation check

Requires Ollama running with a model (`ollama pull qwen2.5:3b-instruct`) and config direction `en-ja` (config file: `~/Library/Application Support/<bundle-id>/config.json`).

```sh
say -v Samantha "Let's review the quarterly roadmap. The activation rate improved by twelve percent."
```

Expect an English `final` in the log, then the translation streaming on the overlay. Translation requests are visible in the Ollama server log; `curl -s localhost:11434/api/ps` shows the model loaded after the first request.

## Overlay visual / window-state check

- Screenshot mid-speech (the pill auto-fades after ~5s of silence, so capture while captions are active):
  ```sh
  say -v Samantha "overlay visibility test" & sleep 3 && screencapture -x /tmp/overlay_check.png
  ```
  Read the image and confirm pill rendering. Note: `screencapture` only captures the current Space; if the user is on a full-screen Space the overlay should appear *there* (that's the `fullScreenAuxiliary` guarantee).
- Window state without screenshots — layer, bounds, and whether it's on the current Space:
  ```swift
  // swift /tmp/winlist.swift
  import CoreGraphics; import Foundation
  for opts in [CGWindowListOption.optionOnScreenOnly, [.optionAll]] {
      print("---")
      for w in (CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] ?? [])
      where (w["kCGWindowOwnerName"] as? String ?? "").contains("kotonoha") {
          print(w["kCGWindowLayer"] ?? "?", w["kCGWindowBounds"] ?? "?")
      }
  }
  ```
  Expect the overlay at layer 25 (Status), within the primary display's bounds, and present in the onScreenOnly listing.

## Cleanup

Restore the saved volume settings and quit the app via the tray (or `pkill -x kotonoha`; note tray-quit exercises the graceful whisper shutdown path, pkill does not).

## Known limitations

- TTS-through-speakers degrades mic-path accuracy; judge the pipeline, not the word error rate.
- Permission dialogs cannot be clicked programmatically — if a TCC prompt appears, the user must approve it. `tccutil reset All <bundle-id>` clears stale denials.
