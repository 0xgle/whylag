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

#[derive(serde::Deserialize)]
struct Filter {
    name: String,
    extensions: Vec<String>,
}

/// Asks where to save with a native dialog and writes the file. The path never
/// comes from the web view, so page script cannot write anywhere on its own.
#[tauri::command]
async fn save_file(
    app: tauri::AppHandle,
    default_name: String,
    filters: Option<Vec<Filter>>,
    content: String,
    base64: bool,
) -> Result<Option<String>, ApiError> {
    use tauri_plugin_dialog::DialogExt;
    let bytes = if base64 {
        whylag_core::util::base64_decode(&content)
            .ok_or_else(|| ApiError::bad_params("invalid base64"))?
    } else {
        content.into_bytes()
    };
    let picked = tokio::task::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_file_name(&default_name);
        for f in filters.unwrap_or_default() {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(&f.name, &exts);
        }
        dialog.blocking_save_file()
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Opens (or focuses) the small always-on-top gamer overlay window.
#[tauri::command]
async fn open_overlay(app: tauri::AppHandle) -> Result<(), ApiError> {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "overlay",
        tauri::WebviewUrl::App("index.html#overlay".into()),
    )
    .title("WhyLag")
    .inner_size(300.0, 150.0)
    .min_inner_size(240.0, 120.0)
    .always_on_top(true)
    .decorations(false)
    .skip_taskbar(true)
    .resizable(true)
    .build()
    .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("whylag"));
            app.manage(AppState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![api, save_file, open_overlay])
        .run(tauri::generate_context!())
        .expect("error while running WhyLag");
}
