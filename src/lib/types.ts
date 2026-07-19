export type Source = "mic" | "system";

export interface Caption {
  utteranceId: string;
  source: Source;
  /** Transcribed original text (Japanese in ja mode, English in en→ja mode) */
  original: string;
  /** Streaming translation from Ollama (en→ja mode only) */
  translation?: string;
  isFinal: boolean;
  translationDone?: boolean;
}

export interface TranscriptEvent {
  utteranceId: string;
  source: Source;
  text: string;
  isFinal: boolean;
}

export interface TranslationEvent {
  utteranceId: string;
  delta: string;
  done: boolean;
}

export type Direction = "ja" | "en-ja";

/** What to export automatically when capture stops */
export type AutoExport = "off" | "transcript" | "summary";

export interface Config {
  direction: Direction;
  ollamaModel: string | null;
  micEnabled: boolean;
  systemEnabled: boolean;
  autoExport: AutoExport;
}

export interface SessionInfo {
  /** File stem of the stored session, e.g. "20260719-140302" */
  id: string;
  startedAtMs: number;
  endedAtMs: number;
  utterances: number;
}

/** Overlay pill bounds (logical px, webview top-left origin) for cursor hit-testing */
export interface InteractiveRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PermissionStatus {
  microphone: boolean;
  screenRecording: boolean;
}

export interface WhisperModelInfo {
  label: string;
  file: string;
  sizeMb: number;
  downloaded: boolean;
}

export interface DownloadProgress {
  file: string;
  downloaded: number;
  total: number;
}
