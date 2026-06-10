export type Source = "mic" | "system";

export interface Caption {
  utteranceId: string;
  source: Source;
  /** Original transcript (Japanese in ja mode, English in en→ja mode) */
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

export interface Config {
  direction: Direction;
  ollamaModel: string | null;
  micEnabled: boolean;
  systemEnabled: boolean;
}
