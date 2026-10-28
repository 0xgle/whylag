use std::sync::Arc;
use tauri::Manager;
use whylag_core::{ApiError, AppState};

/// Single bridge command: every UI call goes through the shared core dispatcher.
#[tauri::command]
async fn api(
    state: tauri::State<'_, Arc<AppState>>,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    whylag_core::api::dispatch(state.inner(), &method, params).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("whylag"));
            app.manage(AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("error while running WhyLag");
}
