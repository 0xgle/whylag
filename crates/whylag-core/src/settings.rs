use serde::{Deserialize, Serialize};
use std::path::Path;

/// Where measurements come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DataSource {
    /// Real hardware when available, simulator otherwise.
    #[default]
    Auto,
    /// Real hardware only; errors are surfaced instead of falling back.
    Live,
    /// Always the built-in simulated office (demos, training, development).
    Simulator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub data_source: DataSource,
    /// UI language: "en" or "pl".
    pub language: String,
    /// SNMP v2c community used for switch/router discovery.
    pub snmp_community: String,
    /// Company shown on reports (integrators brand their own reports).
    pub report_company: String,
    pub report_author: String,
    /// Contracted ISP speed, used to judge speed test results.
    pub contracted_down_mbps: Option<f64>,
    pub contracted_up_mbps: Option<f64>,
    /// User accepted that M-Lab speed tests publish results as open data.
    pub mlab_consent: bool,
    /// RSSI (dBm) considered "good coverage" in survey and advisor.
    pub coverage_threshold_dbm: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            data_source: DataSource::Auto,
            language: "en".into(),
            snmp_community: "public".into(),
            report_company: String::new(),
            report_author: String::new(),
            contracted_down_mbps: None,
            contracted_up_mbps: None,
            mlab_consent: false,
            coverage_threshold_dbm: -67,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).expect("settings serialise"),
        )
    }
}
