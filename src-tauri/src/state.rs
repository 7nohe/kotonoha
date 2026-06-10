use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use tauri::menu::CheckMenuItem;
use tauri::Wry;

use crate::config::Config;
use crate::history::History;
use crate::pipeline::PipelineHandle;
use crate::stt::engine::SttEngine;
use crate::translate::queue::TranslationQueue;

#[derive(Default)]
pub struct AppState {
    pub history: History,
    pub click_through: AtomicBool,
    pub tray_click_through_item: Mutex<Option<CheckMenuItem<Wry>>>,
    pub config: Mutex<Config>,
    /// whisper inference thread (created on first capture start, reused afterwards)
    pub stt_engine: Mutex<Option<SttEngine>>,
    /// Join handle for the whisper thread; joined on quit so the ggml Metal
    /// context is dropped before process teardown (otherwise ggml aborts)
    pub stt_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    pub translation_queue: Mutex<Option<TranslationQueue>>,
    pub mic_pipeline: Mutex<Option<PipelineHandle>>,
    pub system_pipeline: Mutex<Option<PipelineHandle>>,
}
