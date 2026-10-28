use serde::Serialize;

/// Error returned to the UI. `code` is stable and translated by the UI; `message`
/// carries technical detail for logs and the "details" disclosure.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code}: {message}")]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn bad_params(message: impl Into<String>) -> Self {
        Self::new("badParams", message)
    }

    pub fn unknown_method(method: &str) -> Self {
        Self::new("unknownMethod", format!("unknown method `{method}`"))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        // Engines may attach an ApiError as the root cause to keep a stable code.
        if let Some(api) = err.downcast_ref::<ApiError>() {
            return api.clone();
        }
        Self::internal(format!("{err:#}"))
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        Self::bad_params(err.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::new("io", err.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
