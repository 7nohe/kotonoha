# CLAUDE.md

kotonoha — fully-local realtime transcription & translation overlay for macOS (Tauri 2 + React, whisper.cpp, Ollama). Architecture and module map: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Commands

```sh
npm run tauri dev                                # development (watches Rust + frontend)
npm run tauri build -- --bundles app             # release .app
cargo test --manifest-path src-tauri/Cargo.toml  # Rust unit tests
npm run build                                    # TypeScript type-check + Vite build
npm run tauri icon app-icon.png                  # regenerate icon set (keeps icons/tray.png)
```

- Builds are ad-hoc signed by default (`signingIdentity: "-"`). macOS ties privacy permissions (TCC) to the signature, so **export `APPLE_SIGNING_IDENTITY` before building** if a certificate is available — otherwise every rebuild re-prompts the user for mic/screen permissions.
- Release flow: push a `v*` tag → GitHub Actions builds a dmg into a draft Release.

## Critical invariants — do not break these

1. **Overlay window**: never set `alwaysOnTop` / `visibleOnAllWorkspaces` in `tauri.conf.json`. Tauri re-applies collectionBehavior after setup and wipes `fullScreenAuxiliary`, which is what keeps the panel above full-screen apps. All panel config lives in `src-tauri/src/overlay.rs` (tauri-nspanel, `PanelLevel::Status`, nonactivating).
2. **Window positioning**: logical coordinates only. Mixing physical pixels across monitors with different scale factors moves the window off-screen (real bug we shipped once).
3. **Quit path**: join the whisper thread before exiting (`commands::shutdown`). Process teardown while the ggml Metal context is alive aborts in static destructors → "unexpectedly quit" dialogs.
4. **VAD**: `WhisperVadContext` degrades linearly per call (2ms → seconds). The segmenter recreates it every `VAD_RECREATE_EVERY` calls; keep that workaround.
5. **RT audio callbacks** (cpal / SCK / CATap handlers): no locks, no allocation; push into rtrb ring buffers only.
6. **Swift rpath**: `build.rs` adds `-rpath /usr/lib/swift` for the screencapturekit crate's Swift bridge. Removing it makes the binary fail to launch (dyld).
7. **Dev profile**: whisper-rs(-sys) and rubato build with `opt-level = 3` even in dev. Debug-level native code cannot keep up with realtime audio.
8. **Tray icon** (`src-tauri/icons/tray.png`): must be black + alpha only (macOS template image). A PNG without an alpha channel renders as a white square in the menu bar.
9. **System audio backends**: CATap (macOS 14.4+, no Screen Recording permission) is tried first, SCK is the fallback; active backend is logged as `[audio] system backend: ...`. Keep SCK initialization lazy — touching `SCShareableContent` triggers the Screen Recording TCC prompt.

## Conventions

- Code comments, identifiers, commit messages: **English**.
- User-facing strings stay **Japanese**: UI labels, error messages surfaced via `pipeline-error`, exported Markdown headers/speaker labels, Ollama prompts. `Info.plist` is English with Japanese in `locales/ja.lproj/InfoPlist.strings`.
- Docs are dual-language and split by audience: `README(.ja).md` for users, `docs/DEVELOPMENT(.ja).md` for developers. Update both languages when editing either.
- Event names shared with the frontend live as consts in `src-tauri/src/events.rs`; IPC payload types are hand-mirrored in `src/lib/types.ts` — keep them in sync.
- Whisper/VAD models live in `~/Library/Application Support/<bundle-identifier>/models/`. Changing the bundle identifier orphans them (migrate or re-download).

## Verification

- `.claude/skills/e2e-audio-check` — end-to-end check without a human speaker: play `say` TTS, watch `[stt]` log lines, screenshot the overlay, dump window state via CGWindowList.
- `.claude/skills/regen-icons` — regenerate app icon and tray glyph from `design/*.svg` (the render pipeline has non-obvious tooling traps).
- An Ollama model is required for translation tests: `ollama pull qwen2.5:3b-instruct` and use the official Ollama.app (`brew install --cask ollama-app`); the brew *formula* ships without llama-server and returns HTTP 500 for every model.
