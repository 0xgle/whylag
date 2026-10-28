use crate::settings::Settings;
use crate::sim::SimWorld;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::Arc;

/// Process-wide state shared by every engine.
pub struct AppState {
    pub data_dir: PathBuf,
    settings: RwLock<Settings>,
    pub sim: Mutex<SimWorld>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let settings = Settings::load(&data_dir.join("settings.json"));
        Arc::new(Self { data_dir, settings: RwLock::new(settings), sim: Mutex::new(SimWorld::new()) })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    /// Merges a partial JSON object into the settings and persists them.
    pub fn update_settings(&self, patch: serde_json::Value) -> Result<Settings, crate::ApiError> {
        let mut guard = self.settings.write();
        let mut current = serde_json::to_value(&*guard).expect("settings serialise");
        match (current.as_object_mut(), patch.as_object()) {
            (Some(cur), Some(p)) => {
                for (k, v) in p {
                    cur.insert(k.clone(), v.clone());
                }
            }
            _ => return Err(crate::ApiError::bad_params("settings patch must be an object")),
        }
        let updated: Settings = serde_json::from_value(current)?;
        updated.save(&self.data_dir.join("settings.json"))?;
        *guard = updated.clone();
        Ok(updated)
    }
}
