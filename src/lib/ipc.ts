import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Config,
  DownloadProgress,
  PermissionStatus,
  TranscriptEvent,
  TranslationEvent,
  WhisperModelInfo,
} from "./types";

// ---- Commands ----

export const setClickThrough = (enabled: boolean) =>
  invoke<void>("set_click_through", { enabled });

export const showSettings = () => invoke<void>("show_settings");

export const startCapture = () => invoke<void>("start_capture");

export const stopCapture = () => invoke<void>("stop_capture");

export const getConfig = () => invoke<Config>("get_config");

export const setConfig = (config: Config) => invoke<void>("set_config", { config });

export const listOllamaModels = () => invoke<string[]>("list_ollama_models");

export const checkOllama = () => invoke<boolean>("check_ollama");

export const checkPermissions = () => invoke<PermissionStatus>("check_permissions");

export const requestMicrophonePermission = () =>
  invoke<void>("request_microphone_permission");

export const requestScreenRecordingPermission = () =>
  invoke<void>("request_screen_recording_permission");

export const listWhisperModels = () => invoke<WhisperModelInfo[]>("list_whisper_models");

export const downloadWhisperModel = (file: string) =>
  invoke<void>("download_whisper_model", { file });

export const isOnboardingNeeded = () => invoke<boolean>("is_onboarding_needed");

export const exportTranscript = (withSummary: boolean) =>
  invoke<string>("export_transcript", { withSummary });

// ---- Events ----

const onEvent =
  <T>(name: string) =>
  (handler: (payload: T) => void): Promise<UnlistenFn> =>
    listen<T>(name, (e) => handler(e.payload));

export const onTranscript = onEvent<TranscriptEvent>("transcript");
export const onTranslation = onEvent<TranslationEvent>("translation");
export const onPipelineError = onEvent<string>("pipeline-error");
export const onDownloadProgress = onEvent<DownloadProgress>("model-download-progress");
export const onCaptureState = onEvent<boolean>("capture-state");
