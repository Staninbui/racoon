//! racoon - GCP Log Spy with AI superpowers 🦝
//!
//! This crate provides real-time GCP log streaming, AI-powered analysis,
//! and natural language query generation capabilities.
//!
//! # Modules
//!
//! - [`cli`]: Command-line interface definitions using clap.
//! - [`gcp`]: Google Cloud Platform integration (auth, logging).
//! - [`ai`]: AI-powered log analysis and query generation.
//! - [`storage`]: Log storage and export functionality.
//! - [`ui`]: User interface for log streaming and analysis.
//! - [`error`]: Application-level error types.
//! - [`utils`]: Utility functions.
//!
//! # Example
//!
//! ```rust,no_run
//! use racoon::gcp::GcpAuth;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Initialize GCP authentication
//!     let auth = GcpAuth::new().await?;
//!     auth.verify().await?;
//!     println!("Connected to GCP!");
//!     Ok(())
//! }
//! ```


pub mod cli;
pub mod error;
pub mod gcp;
pub mod ui;
pub mod ai;
pub mod storage;
pub mod utils;

// Re-exports
pub use error::{RacoonError, Result};
pub use gcp::{GcpAuth, LogEntry, LogFilter, LogsClient};
pub use ui::{App, run_app};
pub use ai::GeminiClient;
pub use storage::{export_logs, import_logs, ExportFormat, ExportOptions};
pub use utils::{parse_duration, parse_duration_chrono};

