import { useEffect, useState } from "react";
import {
  checkOllama,
  exportTranscript,
  getConfig,
  isOnboardingNeeded,
  listOllamaModels,
  setConfig,
} from "../../lib/ipc";
import type { Config, Direction } from "../../lib/types";
import Onboarding from "./Onboarding";
import "./settings.css";

export default function Settings() {
  const [config, setConfigState] = useState<Config | null>(null);
  const [ollamaModels, setOllamaModels] = useState<string[]>([]);
  const [ollamaUp, setOllamaUp] = useState<boolean | null>(null);
  const [onboarding, setOnboarding] = useState<boolean | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportStatus, setExportStatus] = useState<string | null>(null);

  useEffect(() => {
    void isOnboardingNeeded().then(setOnboarding);
    void getConfig().then(setConfigState);
    void checkOllama().then((up) => {
      setOllamaUp(up);
      if (up) void listOllamaModels().then(setOllamaModels);
    });
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

  const doExport = async (withSummary: boolean) => {
    setExporting(true);
    setExportStatus(withSummary ? "サマリを生成中..." : null);
    try {
      const path = await exportTranscript(withSummary);
      setExportStatus(`保存しました: ${path.split("/").pop()}`);
    } catch (e) {
      setExportStatus(String(e));
    } finally {
      setExporting(false);
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
          <button className="action" disabled={exporting} onClick={() => void doExport(false)}>
            Markdown で書き出す
          </button>
          <button className="action" disabled={exporting} onClick={() => void doExport(true)}>
            サマリ付きで書き出す
          </button>
        </div>
        {exportStatus && <p className="hint">{exportStatus}</p>}
        <p className="hint">~/Downloads に保存されます (メニューバーからも実行可能)</p>
      </section>
    </div>
  );
}
