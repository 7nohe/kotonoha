import { useEffect, useState } from "react";
import {
  checkOllama,
  deleteSession,
  exportSession,
  exportTranscript,
  getConfig,
  isOnboardingNeeded,
  listOllamaModels,
  listSessions,
  onCaptureState,
  setConfig,
} from "../../lib/ipc";
import type { AutoExport, Config, Direction, SessionInfo } from "../../lib/types";
import Onboarding from "./Onboarding";
import "./settings.css";

function sessionLabel(s: SessionInfo): string {
  const started = new Date(s.startedAtMs);
  const date = started.toLocaleDateString("ja-JP", {
    year: "numeric",
    month: "numeric",
    day: "numeric",
  });
  const time = started.toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" });
  const mins = Math.max(1, Math.round((s.endedAtMs - s.startedAtMs) / 60000));
  return `${date} ${time} (${mins}分・${s.utterances}発話)`;
}

export default function Settings() {
  const [config, setConfigState] = useState<Config | null>(null);
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [ollamaUp, setOllamaUp] = useState<boolean | null>(null);
  const [onboarding, setOnboarding] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [sessionStatus, setSessionStatus] = useState<string | null>(null);

  const refreshSessions = () => {
    void listSessions()
      .then(setSessions)
      .catch(() => setSessions([]));
  };

  useEffect(() => {
    void isOnboardingNeeded().then(setOnboarding);
    void getConfig().then(setConfigState);
    void checkOllama().then((up) => {
      setOllamaUp(up);
      if (up) void listOllamaModels().then(setOllamaModels);
    });
    refreshSessions();
    // A finished capture becomes a new stored session — but this window
    // outlives its visibility (close hides it), so only refresh when shown
    const unlisten = onCaptureState((capturing) => {
      if (!capturing && document.visibilityState === "visible") refreshSessions();
    });
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") refreshSessions();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      void unlisten.then((fn) => fn());
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  if (onboarding) {
    return (
      <div className="settings-root">
        <Onboarding onComplete={() => setOnboarding(false)} />
      </div>
    );
  }

  const update = (patch: Partial<Config>) => {
    if (!config) return;
    const next = { ...config, ...patch };
    setConfigState(next);
    void setConfig(next);
  };

  if (!config) return <div className="settings-root" />;

  /** Shared export choreography; `setStatus` picks which section shows it */
  const runExport = async (
    task: () => Promise<string>,
    withSummary: boolean,
    setStatus: (s: string | null) => void,
  ) => {
    setBusy(true);
    setStatus(withSummary ? "サマリを生成中..." : null);
    try {
      const path = await task();
      setStatus(`保存しました: ${path.split("/").pop()}`);
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doDeleteSession = async (id: string) => {
    setBusy(true);
    try {
      await deleteSession(id);
      refreshSessions();
      setSessionStatus(null);
    } catch (e) {
      setSessionStatus(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-root">
      <header className="settings-header">
        <h1>kotonoha</h1>
        <p>ローカル完結のリアルタイム字幕・翻訳</p>
      </header>

      <section className="group">
        <label className="group-label">言語</label>
        <div className="segmented">
          {(
            [
              ["ja", "日本語"],
              ["en-ja", "英語→日本語"],
            ] as [Direction, string][]
          ).map(([value, label]) => (
            <button
              key={value}
              className={config.direction === value ? "seg active" : "seg"}
              onClick={() => update({ direction: value })}
            >
              {label}
            </button>
          ))}
        </div>
      </section>

      <section className="group">
        <label className="group-label">音声ソース</label>
        <div className="row">
          <span>マイク (自分の声)</span>
          <button
            className={config.micEnabled ? "switch on" : "switch"}
            onClick={() => update({ micEnabled: !config.micEnabled })}
            aria-label="マイク"
          />
        </div>
        <div className="row">
          <span>システム音声 (相手の声)</span>
          <button
            className={config.systemEnabled ? "switch on" : "switch"}
            onClick={() => update({ systemEnabled: !config.systemEnabled })}
            aria-label="システム音声"
          />
        </div>
      </section>

      <section className="group">
        <label className="group-label">モデル</label>
        <div className="row">
          <span>文字起こし</span>
          <select disabled>
            <option>large-v3-turbo (推奨)</option>
          </select>
        </div>
        <div className="row">
          <span>翻訳 (Ollama)</span>
          {ollamaUp === false ? (
            <span className="status-bad">未起動 — `ollama serve` を実行</span>
          ) : (
            <select
              value={config.ollamaModel ?? ""}
              onChange={(e) => update({ ollamaModel: e.target.value || null })}
            >
              <option value="">自動 (先頭のモデル)</option>
              {ollamaModels.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          )}
        </div>
      </section>

      <section className="group">
        <label className="group-label">議事録</label>
        <div className="row">
          <span>停止時に自動で書き出す</span>
          <select
            value={config.autoExport}
            onChange={(e) => update({ autoExport: e.target.value as AutoExport })}
          >
            <option value="off">しない</option>
            <option value="transcript">トランスクリプトのみ</option>
            <option value="summary">サマリ付き (Ollama)</option>
          </select>
        </div>
        <div className="row">
          <button
            className="action"
            disabled={busy}
            onClick={() => void runExport(() => exportTranscript(false), false, setExportStatus)}
          >
            Markdown で書き出す
          </button>
          <button
            className="action"
            disabled={busy}
            onClick={() => void runExport(() => exportTranscript(true), true, setExportStatus)}
          >
            サマリ付きで書き出す
          </button>
        </div>
        {exportStatus && <p className="hint">{exportStatus}</p>}
        <p className="hint">~/Downloads に保存されます (メニューバーからも実行可能)</p>
      </section>

      <section className="group">
        <label className="group-label">過去のセッション</label>
        {sessions.length === 0 ? (
          <p className="hint">保存されたセッションはまだありません</p>
        ) : (
          <ul className="session-list">
            {sessions.map((s) => (
              <li key={s.id} className="session-item">
                <span className="session-title">{sessionLabel(s)}</span>
                <span className="session-actions">
                  <button
                    className="action"
                    disabled={busy}
                    onClick={() =>
                      void runExport(() => exportSession(s.id, false), false, setSessionStatus)
                    }
                  >
                    書き出す
                  </button>
                  <button
                    className="action"
                    disabled={busy}
                    onClick={() =>
                      void runExport(() => exportSession(s.id, true), true, setSessionStatus)
                    }
                  >
                    サマリ付き
                  </button>
                  <button
                    className="action danger"
                    disabled={busy}
                    onClick={() => void doDeleteSession(s.id)}
                  >
                    削除
                  </button>
                </span>
              </li>
            ))}
          </ul>
        )}
        {sessionStatus && <p className="hint">{sessionStatus}</p>}
      </section>
    </div>
  );
}
