//! WhyLag core: every diagnostic engine lives here, independent of the UI shell.
//!
//! The Tauri app and the development HTTP bridge both talk to this crate through a
//! single entry point, [`api::dispatch`], so the desktop app and browser preview
//! always run exactly the same code.

pub mod api;
pub mod error;
pub mod settings;
pub mod state;
pub mod util;

pub mod rf;
pub mod sim;
pub mod survey;
pub mod oui;
pub mod wifi;

pub use error::{ApiError, ApiResult};
pub use state::AppState;

/// Application version, kept in sync with the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
