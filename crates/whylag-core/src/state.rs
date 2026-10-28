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
    pub discovery: Mutex<crate::discovery::DiscoveryRun>,
    pub discovery_handle: crate::discovery::DiscoveryHandle,
    pub perf: Mutex<crate::perf::PerfRun>,
    pub perf_handle: crate::perf::PerfHandle,
    pub path: Mutex<crate::path::PathRun>,
    pub path_handle: crate::path::PathHandle,
    pub autotest: Mutex<crate::autotest::AutoTestRun>,
    pub autotest_handle: crate::autotest::AutoTestHandle,
    pub roam: Mutex<crate::roaming::RoamRun>,
    pub roam_handle: crate::roaming::RoamHandle,
    pub peer_server: Mutex<Option<crate::perf::peer::PeerServer>>,
    pub monitor: crate::monitor::MonitorHandle,
    pub security: Mutex<Option<crate::security::SecurityReport>>,
    /// Set when the last Wi-Fi scan fell back to the simulator.
    pub sim_fallback: RwLock<bool>,
    /// When the adapter was last asked to scan (ms), to throttle requests:
    /// every active scan briefly takes the radio off channel.
    pub last_scan_trigger_ms: std::sync::atomic::AtomicU64,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let settings = Settings::load(&data_dir.join("settings.json"));
        Arc::new(Self {
            data_dir,
            settings: RwLock::new(settings),
            sim: Mutex::new(SimWorld::new()),
            discovery: Mutex::new(Default::default()),
            discovery_handle: Default::default(),
            perf: Mutex::new(Default::default()),
            perf_handle: Default::default(),
            path: Mutex::new(Default::default()),
            path_handle: Default::default(),
            autotest: Mutex::new(Default::default()),
            autotest_handle: Default::default(),
            roam: Mutex::new(Default::default()),
            roam_handle: Default::default(),
            peer_server: Mutex::new(None),
            monitor: Default::default(),
            security: Mutex::new(None),
            sim_fallback: RwLock::new(false),
            last_scan_trigger_ms: Default::default(),
        })
    }

    /// Network engines (discovery, performance, path) use the simulated office
    /// only when the user picked it explicitly; Wi-Fi scanning additionally
    /// falls back to it when no adapter data is available.
    pub fn use_simulator(&self) -> bool {
        self.settings.read().data_source == crate::settings::DataSource::Simulator
    }

    /// True when Wi-Fi data currently comes from the simulator (explicitly or
    /// as the Auto fallback), so survey helpers should simulate as well.
    pub fn sim_active(&self) -> bool {
        self.use_simulator() || *self.sim_fallback.read()
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
            _ => {
                return Err(crate::ApiError::bad_params(
                    "settings patch must be an object",
                ))
            }
        }
        let updated: Settings = serde_json::from_value(current)?;
        updated.save(&self.data_dir.join("settings.json"))?;
        *guard = updated.clone();
        Ok(updated)
    }
}
