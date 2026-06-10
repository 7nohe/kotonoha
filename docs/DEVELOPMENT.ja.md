# 開発者向けドキュメント

[English](./DEVELOPMENT.md) | [利用者向けドキュメント](../README.ja.md)

## スタック

- **アプリ**: Tauri 2 + React + TypeScript (Vite)
- **音声認識**: whisper.cpp ([whisper-rs](https://github.com/tazz4843/whisper-rs) 経由、Metal)、Silero VAD (whisper.cpp 内蔵)
- **翻訳・サマリ**: [Ollama](https://ollama.com) HTTP API (`localhost:11434`、NDJSON ストリーミング)
- **システム音声**: [cidre](https://github.com/yury/cidre) による Core Audio process taps (macOS 14.4+)、不可時は ScreenCaptureKit ([screencapturekit-rs](https://github.com/svtlabs/screencapturekit-rs)) に自動フォールバック
- **マイク**: cpal
- **オーバーレイ**: [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel) の非アクティベート・フローティングパネル

## セットアップ

必要なもの: Node.js 22+ / Rust 1.77.2+ / CMake (`brew install cmake`) / Xcode Command Line Tools

```sh
npm install
npm run tauri dev      # 開発 (Rust + フロントを watch)
npm run tauri build    # リリース .app → src-tauri/target/release/bundle/macos/
cargo test --manifest-path src-tauri/Cargo.toml
```

## アーキテクチャ

```
mic (cpal) ──────────┐  RTコールバック: ロックフリーの rtrb リングバッファのみ
system (CATap/SCK) ──┤
                     ▼
        パイプライン worker スレッド (ソースごと)
        ダウンミックス → 16kHz リサンプル (rubato) → Silero VAD セグメンタ
        (発話中 ~1.5s ごとに PARTIAL、無音 600ms か 12s 上限で FINAL)
                     ▼
        whisper スレッド ×1 (Metal、推論を直列化、
        古い partial はまとめて破棄)
                     ▼ "transcript" イベント
        ┌────────────┴────────────┐
        ▼                         ▼
   オーバーレイ webview      翻訳キュー (tokio)
   (React で字幕描画)        → Ollama ストリーミング → "translation" イベント
                            → 履歴 (エクスポート / サマリ)
```

`src-tauri/src/` の主要モジュール:

| モジュール | 役割 |
|---|---|
| `pipeline.rs` | キャプチャバックエンドの結線、worker スレッド、partial/final ジョブ制御 |
| `audio/` | mic (cpal)、system (SCK)、system_catap (Core Audio taps)、リサンプラ |
| `stt/` | whisper エンジンスレッド、VAD セグメンタ、モデルカタログ & ダウンロード |
| `translate/` | Ollama クライアント (chat / ストリーミング / サマリ)、翻訳キュー |
| `overlay.rs` | NSPanel 変換、ウィンドウレベル・collection behavior、配置 |
| `history.rs` | セッション発話履歴の蓄積、Markdown エクスポート |

## 開発時のコード署名

`tauri.conf.json` は `"signingIdentity": "-"`(ad-hoc)で、クローン直後からビルドできます。macOS のプライバシー権限 (TCC) はコード署名に紐づくため、ad-hoc のままだとリビルドのたびに権限が再要求されます。証明書がある場合:

```sh
export APPLE_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)"
```

これで権限の許可がリビルド後も保持されます。

## ハマりどころメモ(該当箇所を触る前に読むこと)

- **オーバーレイウィンドウ**: `tauri.conf.json` で `alwaysOnTop` / `visibleOnAllWorkspaces` を**設定しないこと** — Tauri が setup 後に collectionBehavior を上書きし、フルスクリーンアプリの上に表示するための `fullScreenAuxiliary` が消えます。パネル設定は `src-tauri/src/overlay.rs` に集約。ウィンドウ配置は必ず論理座標で行うこと(スケールの異なるマルチディスプレイで物理ピクセルを混ぜると画面外に飛びます)
- **whisper.cpp の VAD**: `WhisperVadContext` は呼び出しごとに処理時間が線形に伸びるため、セグメンタ内で定期的に再生成しています (`stt/segmenter.rs`)
- **終了パス**: プロセス終了前に whisper スレッドを join すること — ggml の Metal コンテキストが生きたまま落とすと static デストラクタで abort します (`commands::shutdown`)
- **Swift rpath**: screencapturekit crate は Swift ブリッジを含むため、バイナリに `/usr/lib/swift` への rpath が必要 (`build.rs`)
- **dev プロファイル**: whisper / DSP 系ネイティブクレートは dev でも `opt-level = 3` — デバッグ最適化ではリアルタイム音声処理に間に合いません (`Cargo.toml` の profile overrides)
- **CATap と SCK**: Core Audio tap バックエンドは画面収録権限(と macOS 15 の定期再確認)を完全に回避できますが、tap 作成に失敗すると ScreenCaptureKit に静かにフォールバックします。どちらが使われたかは `[audio] system backend: ...` ログで確認

## リリース

`v*` タグを push すると[リリースワークフロー](../.github/workflows/release.yml)が起動し、Apple Silicon ランナーで `.dmg` をビルドして**ドラフト**の GitHub Release に添付します:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Releases ページでドラフトを確認して公開してください。

secrets 未設定の場合は ad-hoc 署名になります。署名・公証済みビルドにする場合のリポジトリ secrets:

| Secret | 値 |
|---|---|
| `APPLE_CERTIFICATE` | "Developer ID Application" 証明書の `.p12` を base64 化したもの |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` のパスワード |
| `APPLE_SIGNING_IDENTITY` | 例: `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | 公証用の Apple ID・アプリ用パスワード・チーム ID |

## デザインアセット

`design/logo.svg`(アプリアイコンのソース)と `design/tray.svg`(メニューバー用テンプレートグリフ)。編集後:

```sh
# アプリアイコン: qlmanage でレンダリング (ImageMagick は SVG グラデーションを壊す) → squircle マスク
qlmanage -t -s 1024 -o /tmp design/logo.svg
magick -size 1024x1024 xc:none -fill white -draw "roundrectangle 0,0 1023,1023 230,230" PNG32:/tmp/mask.png
magick /tmp/logo.svg.png -background none -gravity center -extent 1024x1024 /tmp/mask.png -alpha off -compose CopyOpacity -composite PNG32:app-icon.png
npm run tauri icon app-icon.png   # src-tauri/icons/ を再生成 (tray.png は保持される)
```

メニューバーグリフ (`src-tauri/icons/tray.png`) は**黒+アルファのみ**であること(macOS のテンプレート画像)。SVG からではなく ImageMagick のネイティブ描画でレンダリングしてください。
