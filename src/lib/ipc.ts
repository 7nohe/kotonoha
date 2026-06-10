import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Config, TranscriptEvent, TranslationEvent } from "./types";

export const setClickThrough = (enabled: boolean) =>
  invoke<void>("set_click_through", { enabled });

export const getClickThrough = () => invoke<boolean>("get_click_through");

export const showSettings = () => invoke<void>("show_settings");

export const startCapture = () => invoke<void>("start_capture");

export const stopCapture = () => invoke<void>("stop_capture");

export const isCapturing = () => invoke<boolean>("is_capturing");

export const getConfig = () => invoke<Config>("get_config");

export const setConfig = (config: Config) => invoke<void>("set_config", { config });

export const listOllamaModels = () => invoke<string[]>("list_ollama_models");

export const checkOllama = () => invoke<boolean>("check_ollama");

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

export const checkPermissions = () => invoke<PermissionStatus>("check_permissions");

export const requestMicrophonePermission = () =>
  invoke<void>("request_microphone_permission");

export const requestScreenRecordingPermission = () =>
  invoke<void>("request_screen_recording_permission");

export const listWhisperModels = () => invoke<WhisperModelInfo[]>("list_whisper_models");

export const downloadWhisperModel = (file: string) =>
  invoke<void>("download_whisper_model", { file });

export const isOnboardingNeeded = () => invoke<boolean>("is_onboarding_needed");

export const onDownloadProgress = (
  handler: (p: DownloadProgress) => void,
): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model-download-progress", (e) => handler(e.payload));

export const exportTranscript = (withSummary: boolean) =>
  invoke<string>("export_transcript", { withSummary });

export const clearHistory = () => invoke<void>("clear_history");

export const onCaptureState = (
  handler: (recording: boolean) => void,
): Promise<UnlistenFn> =>
  listen<boolean>("capture-state", (e) => handler(e.payload));

export const onTranscript = (
  handler: (event: TranscriptEvent) => void,
): Promise<UnlistenFn> =>
  listen<TranscriptEvent>("transcript", (e) => handler(e.payload));

export const onTranslation = (
  handler: (event: TranslationEvent) => void,
): Promise<UnlistenFn> =>
  listen<TranslationEvent>("translation", (e) => handler(e.payload));

export const onPipelineError = (
  handler: (message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ message: string }>("pipeline-error", (e) => handler(e.payload.message));
