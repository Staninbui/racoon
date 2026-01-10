//! Offline storage module for racoon.
//!
//! Provides functionality to save and load logs locally for offline analysis.
//!
//! # Features
//!
//! - **Export**: Download logs to JSON or NDJSON files
//! - **Import**: Load previously saved logs for offline viewing
//! - **AI-Assisted Download**: Use natural language to specify what to download
//!
//! # Supported Formats
//!
//! - JSON (`.json`) - Pretty-printed array of log entries
//! - NDJSON (`.ndjson`) - Newline-delimited JSON, one entry per line
//!
//! # Example
//!
//! ```rust,ignore
//! use gcp_racoon::storage::{export_logs, import_logs, ExportOptions, ExportFormat};
//! use gcp_racoon::gcp::{LogsClient, LogFilter};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Export logs
//!     let options = ExportOptions {
//!         format: ExportFormat::Json,
//!         pretty: true,
//!         include_metadata: true,
//!     };
//!     let count = export_logs(&client, filter, "logs.json", options).await?;
//!
//!     // Import logs later
//!     let logs = import_logs("logs.json")?;
//!
//!     Ok(())
//! }
//! ```
pub mod export;
pub mod formats;
pub mod import;

pub use export::{ExportOptions, ExportResult, export_logs, export_logs_with_ai};
pub use formats::ExportFormat;
pub use import::{ImportResult, import_logs};
