use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalOnlyDefaults {
    pub core_network_required: bool,
    pub raw_media_leaves_device: bool,
    pub storage: StorageDefaults,
    pub optional_ai: OptionalAiDefaults,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageDefaults {
    pub root: &'static str,
    pub database_file: &'static str,
    pub recordings_directory: &'static str,
    pub whisper_models_directory: &'static str,
    pub temp_directory: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionalAiDefaults {
    pub enabled: bool,
    pub payload_scope: &'static str,
}

pub fn local_only_defaults() -> LocalOnlyDefaults {
    LocalOnlyDefaults {
        core_network_required: false,
        raw_media_leaves_device: false,
        storage: StorageDefaults {
            root: "app_data_directory",
            database_file: "app.sqlite",
            recordings_directory: "recordings",
            whisper_models_directory: "models/whisper",
            temp_directory: "temp",
        },
        optional_ai: OptionalAiDefaults {
            enabled: false,
            payload_scope: "transcript_text_and_recording_metadata_only",
        },
    }
}
