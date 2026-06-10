# kotonoha 🍃

[English](./README.md)

ローカル完結のリアルタイム文字起こし・翻訳オーバーレイ (macOS)。

どのミーティングツールでも、手元で kotonoha を起動しておくとフローティング字幕が表示されます。日本語ミーティングではリアルタイム文字起こし、英語ミーティングではリアルタイム日本語訳。音声は外部に一切送信されず、すべてローカルで処理されます。

## 特徴

- **フローティング字幕オーバーレイ** — フォーカスを奪わない非アクティベートパネルで、フルスクリーンの会議アプリの上にも表示。ドラッグ移動・クリックスルー・無音時の自動フェード対応
- **リアルタイム文字起こし** — whisper.cpp (large-v3-turbo) + Metal アクセラレーション、Silero VAD による発話区間検出、約 2 秒以内に途中結果を表示
- **ライブ翻訳** — 英語の発話をローカルの Ollama モデルで日本語化し、原文の下にトークン単位でストリーミング表示
- **2 系統の音声ソース** — システム音声(相手の声)とマイク(自分の声)を並行して文字起こし
- **議事録エクスポート** — セッションの発話履歴を Markdown で `~/Downloads` に書き出し。Ollama による要点・決定事項・TODO のサマリ付きも可能
- **完全ローカル** — クラウドなし、API キーなし、テレメトリなし

## 構成

- **アプリ**: Tauri 2 + React + TypeScript
- **音声認識**: whisper.cpp ([whisper-rs](https://github.com/tazz4843/whisper-rs) 経由、Metal)、Silero VAD (whisper.cpp 内蔵)
- **翻訳・サマリ**: [Ollama](https://ollama.com) (`localhost:11434`、ストリーミング)
- **システム音声**: Core Audio process taps (macOS 14.4+、画面収録権限不要) → 使えない場合は ScreenCaptureKit に自動フォールバック
- **マイク**: cpal
- **オーバーレイ**: [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel) による非アクティベートのフローティングパネル

## はじめる

### 必要なもの

- macOS 13.3+(Apple Silicon 推奨。Core Audio tap バックエンドは 14.4+)
- Node.js / Rust 1.77.2+ / CMake (`brew install cmake`) / Xcode Command Line Tools

### ビルドと起動

```sh
npm install
npm run tauri dev      # 開発
npm run tauri build    # リリース .app (src-tauri/target/release/bundle/macos/)
```

初回起動時にオンボーディングが開き、権限の付与と音声認識モデルのダウンロード(アプリ内)を案内します。

### Ollama(任意。翻訳・サマリに必要)

```sh
brew install --cask ollama-app   # 公式アプリ。formula 版 (brew install ollama) は
                                 # llama-server が同梱されず動かないことがある
ollama serve                     # または Ollama.app を起動
ollama pull qwen2.5:3b-instruct  # 軽量で高速な翻訳向けモデル
```

設定画面の「言語」を「英語→日本語」に切り替えると、英語の発話が確定するたびに日本語訳がストリーミング表示されます。Ollama が起動していなくても文字起こしは動き続けます。

## 使い方

1. メニューバーのアイコンから操作(オーバーレイ表示/非表示、クリックスルー、議事録の書き出し、設定、終了)
2. オーバーレイはドラッグで任意の位置へ移動。ホバーで操作ボタン(キャプチャ開始/停止・クリックスルー・設定)が出現
3. クリックスルーを有効にすると字幕越しに下のアプリを操作可能(解除はメニューバーから)
4. **ヘッドホン推奨**: スピーカー再生だと相手の声をマイクが二重に拾います

## システム音声のキャプチャ方式

1. **Core Audio process tap** (macOS 14.4+) — 「画面収録」ではなく軽い「システムオーディオ録音」権限で動作。起動時にまずこちらを試します
2. **ScreenCaptureKit** — tap が使えない環境 (macOS 13.x、権限拒否など) では自動フォールバック

どちらが使われたかはログ (`[audio] system backend: ...`) で確認できます。

## コード署名(開発時)

`tauri.conf.json` は `"signingIdentity": "-"`(ad-hoc)になっており、クローン直後からそのままビルドできます。ただし macOS のプライバシー権限 (TCC) はコード署名に紐づくため、ad-hoc のままだとリビルドのたびに権限が再要求されます。署名証明書を持っている場合はシェルで一度 export してからビルドしてください:

```sh
export APPLE_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)"
```

これで権限の許可がリビルド後も保持されます。

## 開発メモ

- オーバーレイウィンドウの `alwaysOnTop` / `visibleOnAllWorkspaces` を `tauri.conf.json` で**設定してはいけません**。Tauri が setup 後に collectionBehavior を上書きし、フルスクリーンアプリの上に表示するためのフラグ `fullScreenAuxiliary` が消えます。パネル設定は `src-tauri/src/overlay.rs` に集約しています
- whisper.cpp の `WhisperVadContext` は呼び出しを重ねると処理時間が線形に伸びるため、セグメンタ内で定期的に再生成しています (`stt/segmenter.rs`)
- screencapturekit crate は Swift ブリッジを含むため、バイナリに `/usr/lib/swift` への rpath が必要です (`build.rs`)
- dev ビルドでも whisper / DSP 系クレートは `opt-level = 3` でコンパイルします(デバッグ最適化ではリアルタイム音声処理に間に合いません)

## ライセンス

[MIT](./LICENSE)
