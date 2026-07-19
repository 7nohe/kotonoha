---
name: debug-capture-permissions
description: Diagnose kotonoha's audio-capture permission problems on macOS — CATap falling back to ScreenCaptureKit, unexpected Screen Recording prompts, silent system-audio capture, or TCC denials. Use when the system backend log shows "CATap unavailable", when permission dialogs reappear on every launch, or after macOS updates change capture behavior.
---

# Debugging capture permissions (TCC / CATap / SCK)

kotonoha captures system audio via a Core Audio process tap (CATap) and only falls
back to ScreenCaptureKit (SCK) when tap setup fails. CATap needs the lightweight
"System Audio Recording" permission; SCK needs full "Screen Recording" — so an
unexpected Screen Recording dialog almost always means **CATap failed first**.
Find out why CATap failed before touching permission settings.

## 1. Which backend is actually running?

Launch with stderr captured (Finder-launched apps swallow it):

```sh
src-tauri/target/release/bundle/macos/kotonoha.app/Contents/MacOS/kotonoha > /tmp/k.log 2>&1 &
sleep 5 && grep '\[audio\]' /tmp/k.log
```

- `system backend: Core Audio tap (...)` — CATap active, permissions are fine.
- `CATap unavailable (<error>), falling back to ScreenCaptureKit` — read the error:
  the failure stage is embedded in the Japanese message (tap 作成 / 集約デバイス /
  キャプチャ開始). `fcc: "nope"` on キャプチャ開始 with permission granted is a
  config problem, not TCC (see §4).

## 2. Watch TCC decisions live

The TCC database needs Full Disk Access to read; the unified log does not.
Use the absolute path — user shells may shadow `log` with a function:

```sh
/usr/bin/log stream --predicate 'subsystem == "com.apple.TCC"' --style compact > /tmp/tcc.log 2>&1 &
open -a kotonoha   # Finder-equivalent launch = correct TCC attribution
sleep 10; kill %1
grep -E 'AUTHREQ_CTX|AUTHREQ_RESULT' /tmp/tcc.log | grep -iE 'kotonoha|AudioCapture|ScreenCapture'
```

Pair each `AUTHREQ_CTX` (service name) with the following `AUTHREQ_RESULT`:
`authValue` 0 = denied, 1 = unknown/would prompt, 2 = allowed.
The interesting services: `kTCCServiceAudioCapture` (CATap),
`kTCCServiceScreenCapture` (SCK), `kTCCServiceMicrophone`.

Terminal-launched processes get the terminal host as responsible process, which
can skew prompting — trust `open -a` runs for TCC questions, terminal runs for logs.

## 3. Reset stale grants/denials

```sh
tccutil reset All com.7nohe.kotonoha
```

Then relaunch from Finder and approve the prompts that appear. Ad-hoc-signed
builds get a fresh TCC identity on every rebuild — always build with
`APPLE_SIGNING_IDENTITY` set (see CLAUDE.md) or permissions will not stick.

## 4. Known failure: 'nope' at capture start despite granted permission

macOS 26 rejects `AudioDeviceStart` on a tap-only aggregate device with
`kAudioHardwareIllegalOperationError` ('nope') even when
`kTCCServiceAudioCapture` is allowed. The aggregate must include the output
device in `kAudioAggregateDeviceSubDeviceListKey` (Apple's AudioCap sample does
this) — fixed in `src-tauri/src/audio/system_catap.rs`; keep the sub-device
list if that code is ever rewritten.

## 5. Verify audio actually flows

A tap can start yet deliver all-zero buffers (observed on macOS 26 after long
uptime — Apple Developer Forums thread 825780). Don't stop at "backend: Core
Audio tap"; play TTS and confirm `[stt] System ...` lines appear (see the
e2e-audio-check skill).
