//! A single serializable error type for Tauri commands.
//!
//! We wrap `anyhow::Error` and hand-implement `Serialize` (emitting the display
//! string) so any `?`-propagated error surfaces to the frontend as a plain message.
//! NOTE: intentionally NOT a `thiserror::Error` — that would make `AppError`
//! implement `std::error::Error`, which collides the blanket `From` impl below
//! with core's reflexive `impl From<T> for T`.

use serde::{Serialize, Serializer};

#[derive(Debug)]
pub struct AppError(pub anyhow::Error);

pub type AppResult<T> = Result<T, AppError>;

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Any concrete error that anyhow accepts converts into AppError, so `?` just works.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        AppError(err.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

/// Helper to build an `AppError` from a message.
pub fn err<T>(msg: impl Into<String>) -> AppResult<T> {
    Err(AppError(anyhow::anyhow!(msg.into())))
}
