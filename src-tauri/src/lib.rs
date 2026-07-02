mod ai;
mod binaries;
mod capture;
mod commands;
mod config;
mod encoding;
mod jobs;
mod recorder;
mod storage;
mod transcription;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let storage = storage::initialize(app.handle())?;
            if let Err(error) = jobs::recover_on_startup(&storage) {
                eprintln!("processing job recovery failed: {error}");
            }
            jobs::spawn_pending_worker(storage.clone());
            app.manage(storage);
            app.manage(recorder::RecordingRuntime::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_bootstrap,
            commands::storage_overview,
            commands::create_recording,
            commands::list_recordings,
            commands::get_recording,
            commands::update_recording,
            commands::persist_transcript,
            commands::get_transcript_by_recording,
            commands::search_transcripts,
            commands::reindex_transcript_search,
            commands::whisper_model_status,
            commands::import_whisper_model,
            commands::transcribe_recording,
            commands::upsert_ai_summary,
            commands::get_ai_summary_by_recording,
            commands::get_ai_settings,
            commands::save_ai_settings,
            commands::summarize_recording,
            commands::create_processing_job,
            commands::list_processing_jobs,
            commands::update_processing_job,
            commands::recover_processing_jobs,
            commands::retry_processing_job,
            commands::cleanup_processing_files,
            commands::capture_status,
            commands::request_capture_permissions,
            commands::get_capture_selection,
            commands::save_capture_selection,
            commands::validate_capture_config,
            commands::start_recording_session,
            commands::stop_recording_session,
            commands::active_recording_session,
            commands::get_recording_session_by_recording,
            commands::encode_recording,
            commands::recording_asset_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Metafy Desktop");
}
