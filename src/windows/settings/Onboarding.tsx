import { useEffect, useState } from "react";
import {
  checkOllama,
  checkPermissions,
  downloadWhisperModel,
  listWhisperModels,
  onDownloadProgress,
  requestMicrophonePermission,
  requestScreenRecordingPermission,
  startCapture,
  type PermissionStatus,
  type WhisperModelInfo,
} from "../../lib/ipc";

interface Props {
  onComplete: () => void;
}

export default function Onboarding({ onComplete }: Props) {
  const [perms, setPerms] = useState<PermissionStatus | null>(null);
  const [models, setModels] = useState<WhisperModelInfo[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [ollamaUp, setOllamaUp] = useState<boolean | null>(null);

  const refresh = () => {
    void checkPermissions().then(setPerms);
    void listWhisperModels().then(setModels);
    void checkOllama().then(setOllamaUp);
  };

  useEffect(() => {
    refresh();
    // Permissions can be granted in System Settings, so re-check periodically
    const timer = setInterval(() => void checkPermissions().then(setPerms), 2000);
    const unlisten = onDownloadProgress((p) => {
      if (p.total > 0) setProgress(p.downloaded / p.total);
    });
    return () => {
      clearInterval(timer);
      void unlisten.then((un) => un());
    };
  }, []);

  const download = async (file: string) => {
    setDownloading(file);
    setProgress(0);
    setError(null);
    try {
      await downloadWhisperModel(file);
      refresh();
      // Once the models are in place, start capturing immediately and finish onboarding
      await startCapture().catch(() => {});
      onComplete();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
    }
  };

  const permsOk = perms?.microphone && perms?.screenRecording;

  return (
    <div className="onboarding">
      <header className="settings-header">
        <h1>kotonoha へようこそ</h1>
        <p>3 ステップでセットアップが完了します。すべてローカルで動作します。</p>
      </header>

      <section className="group">
        <label className="group-label">1. 権限</label>
        <div className="row">
          <span>
            <StatusDot ok={!!perms?.microphone} />
            マイク
          </span>
          {!perms?.microphone && (
            <button
              className="action"
              onClick={() => void requestMicrophonePermission().then(refresh)}
            >
              許可する
            </button>
          )}
        </div>
        <div className="row">
          <span>
            <StatusDot ok={!!perms?.screenRecording} />
            画面収録 (相手の声の取り込みに必要)
          </span>
          {!perms?.screenRecording && (
            <button
              className="action"
              onClick={() => void requestScreenRecordingPermission()}
            >
              許可する
            </button>
          )}
        </div>
        {!permsOk && (
          <p className="hint">
            画面収録はシステム設定で許可後、アプリの再起動が必要な場合があります
          </p>
        )}
      </section>

      <section className="group">
        <label className="group-label">2. 音声認識モデル</label>
        {models.map((m) => (
          <div className="row" key={m.file}>
            <span>
              <StatusDot ok={m.downloaded} />
              {m.label}
              <span className="size"> {m.sizeMb}MB</span>
            </span>
            {!m.downloaded &&
              (downloading === m.file ? (
                <div className="progress">
                  <div className="progress-bar" style={{ width: `${progress * 100}%` }} />
                </div>
              ) : (
                <button
                  className="action"
                  disabled={downloading !== null}
                  onClick={() => void download(m.file)}
                >
                  ダウンロード
                </button>
              ))}
          </div>
        ))}
        {error && <p className="status-bad">{error}</p>}
      </section>

      <section className="group">
        <label className="group-label">3. Ollama (英語→日本語翻訳・任意)</label>
        <div className="row">
          <span>
            <StatusDot ok={!!ollamaUp} />
            {ollamaUp
              ? "接続済み"
              : "未検出 — `ollama serve` を起動すると翻訳が有効になります"}
          </span>
        </div>
      </section>
    </div>
  );
}

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot ok" : "dot"} />;
}
