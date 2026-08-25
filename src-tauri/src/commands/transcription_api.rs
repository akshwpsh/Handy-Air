//! Frontend commands for the Transcription API path.
//!
//! Kept in its own file so the feature adds to `commands/mod.rs` rather than
//! editing it — see `docs/adr/0002-transcription-api-as-a-parallel-path.md`.

use crate::managers::transcription::TranscriptionManager;
use crate::settings::{get_settings, write_settings, SecretString};
use crate::transcription_api;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Resolve the key to probe `base_url` with: an explicitly typed one wins,
/// otherwise a stored key - but only one belonging to that same endpoint. The
/// URL here is whatever the user has typed into the form, which is not
/// necessarily the one in settings.
fn key_for_probe(app: &AppHandle, base_url: &str, typed: &str) -> String {
    let typed = typed.trim();
    if !typed.is_empty() {
        return typed.to_string();
    }
    get_settings(app).transcription_api_key_for(base_url)
}

/// List the models an endpoint offers.
///
/// This doubles as the key check in onboarding and settings: a wrong key answers
/// 401 here, which costs nothing, instead of failing the user's first dictation.
#[tauri::command]
#[specta::specta]
pub async fn list_transcription_api_models(
    app: AppHandle,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let key = key_for_probe(&app, &base_url, &api_key);
    transcription_api::list_models(&base_url, &key).await
}

/// Finish onboarding with the Transcription API instead of a downloaded model.
///
/// The regular `select_model` path cannot do this: it requires a model that is
/// already on disk, which is exactly what this flow avoids.
#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding_with_transcription_api(
    app: AppHandle,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<(), String> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let api_key = api_key.trim().to_string();
    let model = model.trim().to_string();

    if base_url.is_empty() {
        return Err("Transcription API endpoint is not set".to_string());
    }
    if model.is_empty() {
        return Err("Transcription API model is not set".to_string());
    }

    // Verify before committing: completing onboarding on a key that does not
    // work would drop the user into an app that cannot transcribe anything.
    // An endpoint that serves no model catalogue still passes - see
    // `verify_endpoint`.
    let probe_key = key_for_probe(&app, &base_url, &api_key);
    transcription_api::verify_endpoint(&base_url, &probe_key).await?;

    let mut settings = get_settings(&app);
    settings.transcription_api_enabled = true;
    settings.transcription_api_base_url = base_url;
    settings.transcription_api_model = model;
    if !api_key.is_empty() {
        settings.transcription_api_key = SecretString::from(api_key);
    }
    settings.onboarding_completed = true;
    write_settings(&app, settings);

    log::info!("Onboarding completed with the Transcription API (no local model)");
    Ok(())
}

/// Turn the Transcription API on or off.
///
/// Enabling it unloads the Local Engine straight away: not spending its
/// 1-3 GB of RAM is the point of the feature, and waiting for the idle
/// unload timeout would leave that memory held for nothing. Disabling it
/// puts the local model back on its normal load path.
#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_enabled_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_api_enabled = enabled;
    write_settings(&app, settings);

    let transcription_manager = app.state::<Arc<TranscriptionManager>>();
    if enabled {
        if let Err(e) = transcription_manager.unload_model() {
            log::warn!("Failed to unload the local model after enabling the API: {e}");
        }
    } else {
        transcription_manager.initiate_model_load();
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_base_url_setting(
    app: AppHandle,
    base_url: String,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_api_base_url = base_url.trim().trim_end_matches('/').to_string();
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_key_setting(app: AppHandle, api_key: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_api_key = SecretString::from(api_key.trim().to_string());
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_model_setting(app: AppHandle, model: String) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_api_model = model.trim().to_string();
    write_settings(&app, settings);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_transcription_api_local_fallback_setting(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.transcription_api_local_fallback = enabled;
    write_settings(&app, settings);
    Ok(())
}
