//! Wi-Fi scanning on Android through `WifiManager`, which only Kotlin can
//! reach. The desktop builds compile this crate as an empty plugin.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager};

/// Handle to the Kotlin side, kept in the app state.
#[cfg(target_os = "android")]
pub struct WifiScan<R: Runtime>(PluginHandle<R>);

#[cfg(target_os = "android")]
impl<R: Runtime> WifiScan<R> {
    /// Raw scan from `WifiManager`: `{ networks: [...], connected: {...} }`.
    /// Blocking: call from a worker thread, never the main thread.
    pub fn scan(&self, trigger: bool) -> Result<serde_json::Value, String> {
        self.0
            .run_mobile_plugin::<serde_json::Value>(
                "scan",
                serde_json::json!({ "trigger": trigger }),
            )
            .map_err(|e| e.to_string())
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("wifiscan")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle =
                    _api.register_android_plugin("eu.m8.whylag.wifiscan", "WifiScanPlugin")?;
                _app.manage(WifiScan(handle));
            }
            Ok(())
        })
        .build()
}
