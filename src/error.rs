//! Application-level error types for racoon.
//!
//! This module defines a unified error enum that wraps all possible errors
//! that can occur in the application. Following the Rust convention:
//! - Libraries use `thiserror` for specific error types
//! - Applications use `anyhow` for ad-hoc errors with context
//!
//! We use `thiserror` here to provide structured errors that can be
//! pattern-matched by callers when needed.

use thiserror::Error;

/// Top-level application error type.
///
/// This enum captures all error categories that racoon can encounter.
/// Each variant warps a more specific error type from the relevant module.
#[derive(Error, Debug)]
pub enum RacoonError {
    /// GCP auth related errors
    #[error("Authentication error: {0}")]
    Auth(#[from] crate::gcp::auth::AuthError),

    /// GCP logging API errors
    #[error("Logging API error: {0}")]
    Logs(#[from] crate::gcp::logs::LogsError),

    /// AI/Gemini API errors
    #[error("AI error: {0}")]
    Ai(#[from] crate::ai::AiError),

    /// Storage/IO errors
    #[error("Storage error: {0}")]
    Storage(#[from] std::io::Error),

    /// Config errors
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Result type alias for racoon operations.
pub type Result<T> = std::result::Result<T, RacoonError>;
