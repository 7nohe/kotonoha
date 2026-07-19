import { useCallback, useEffect, useRef, useState } from "react";
import {
  onCaptureState,
  onOverlayPassthrough,
  onPipelineError,
  onTranscript,
  onTranslation,
  setClickThrough,
  setInteractiveRegion,
  showSettings,
  startCapture,
  stopCapture,
} from "../../lib/ipc";
import type { Caption, TranscriptEvent, TranslationEvent } from "../../lib/types";
import CaptionRow from "./CaptionRow";
import "./overlay.css";

const MAX_ROWS = 3;
const IDLE_AFTER_MS = 5000;

export default function Overlay() {
  const [captions, setCaptions] = useState<Caption[]>([]);
  const [idle, setIdle] = useState(true);
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  // Hover state driven by JS (not CSS :hover): once the window starts
  // ignoring mouse events the webview never receives mouseleave, so the
  // backend tells us via overlay-passthrough when to drop it
  const [hot, setHot] = useState(false);
  const lastActivityRef = useRef(0);
  const pillRef = useRef<HTMLDivElement | null>(null);

  const touch = useCallback(() => {
    lastActivityRef.current = Date.now();
    setIdle(false);
  }, []);

  const applyTranscript = useCallback(
    (ev: TranscriptEvent) => {
      touch();
      setError(null);
      setCaptions((prev) => {
        const i = prev.findIndex((c) => c.utteranceId === ev.utteranceId);
        if (i >= 0) {
          const next = [...prev];
          next[i] = { ...next[i], original: ev.text, isFinal: ev.isFinal };
          return next;
        }
        const caption: Caption = {
          utteranceId: ev.utteranceId,
          source: ev.source,
          original: ev.text,
          isFinal: ev.isFinal,
        };
        return [...prev, caption].slice(-MAX_ROWS);
      });
    },
    [touch],
  );

  const applyTranslation = useCallback(
    (ev: TranslationEvent) => {
      touch();
      setCaptions((prev) => {
        // Return the same array when nothing matches to skip the re-render
        if (!prev.some((c) => c.utteranceId === ev.utteranceId)) return prev;
        return prev.map((c) =>
          c.utteranceId === ev.utteranceId
            ? {
                ...c,
                translation: (c.translation ?? "") + ev.delta,
                translationDone: ev.done,
              }
            : c,
        );
      });
    },
    [touch],
  );

  // Start capturing on launch (works with zero interaction)
  useEffect(() => {
    startCapture()
      .then(() => setRecording(true))
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    const unlisteners = [
      onTranscript(applyTranscript),
      onTranslation(applyTranslation),
      onPipelineError((message) => setError(message)),
      onCaptureState(setRecording),
      onOverlayPassthrough((ignored) => {
        if (ignored) setHot(false);
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, [applyTranscript, applyTranslation]);

  // Report the pill's bounds so the backend can pass clicks outside it
  // through to the apps underneath (cursor hit-testing in overlay.rs)
  useEffect(() => {
    const el = pillRef.current;
    if (!el) return;
    // offset* ignores the idle scale() transform, unlike getBoundingClientRect
    const report = () => {
      void setInteractiveRegion({
        x: el.offsetLeft,
        y: el.offsetTop,
        width: el.offsetWidth,
        height: el.offsetHeight,
      });
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const timer = setInterval(() => {
      if (Date.now() - lastActivityRef.current > IDLE_AFTER_MS) setIdle(true);
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  const toggleRecording = async () => {
    try {
      if (recording) {
        await stopCapture();
      } else {
        await startCapture();
        setError(null);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const enableClickThrough = () => {
    // Disabling is done from the menu bar tray (the window stops receiving mouse events)
    void setClickThrough(true);
  };

  return (
    <div
      className={`overlay-root ${hot ? "hot" : ""}`}
      onMouseEnter={() => setHot(true)}
      onMouseLeave={() => setHot(false)}
    >
      <div
        ref={pillRef}
        className={`pill ${idle ? "idle" : ""} ${error ? "has-error" : ""} ${collapsed ? "collapsed" : ""}`}
        data-tauri-drag-region
      >
        {collapsed ? (
          <div className="chip-row">
            <span className="tool grip" title="ドラッグで移動" data-tauri-drag-region>
              ⠿
            </span>
            <button
              className="tool"
              title="元のサイズに戻す"
              onClick={() => setCollapsed(false)}
            >
              <span className={`rec-dot ${recording ? "on" : ""}`} />
            </button>
          </div>
        ) : (
          <>
            <div className="toolbar">
              <button
                className={`tool ${recording ? "tool-active" : ""}`}
                title={recording ? "キャプチャ停止" : "キャプチャ開始"}
                onClick={toggleRecording}
              >
                <span className={`rec-dot ${recording ? "on" : ""}`} />
              </button>
              <button
                className="tool"
                title="クリックスルー (解除はメニューバーの kotonoha から)"
                onClick={enableClickThrough}
              >
                ◎
              </button>
              <button className="tool" title="設定" onClick={() => void showSettings()}>
                ⚙
              </button>
              <button className="tool" title="縮小" onClick={() => setCollapsed(true)}>
                −
              </button>
              <span className="tool grip" title="ドラッグで移動" data-tauri-drag-region>
                ⠿
              </span>
            </div>

            {captions.length === 0 ? (
              <div className="empty-hint" title={error ?? undefined}>
                <span className={`rec-dot ${recording ? "on" : ""}`} />
                {error
                  ? "エラー — ホバーで詳細を表示"
                  : recording
                    ? "待機中 — 音声を検出すると字幕が表示されます"
                    : "停止中 — ホバーして ⏺ で開始"}
              </div>
            ) : (
              <div className="captions" title={error ?? undefined}>
                {captions.map((c) => (
                  <CaptionRow key={c.utteranceId} caption={c} />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
