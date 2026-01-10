//! AI integration module for racoon.
//!
//! Provides AI-powered log analysis and natural language query generation
//! using Google's Gemini API.
//!
//! # Features
//!
//! - **Log Explanation**: Analyze a log entry and provide human-readable insights
//! - **Query Generation**: Convert natural language to GCP Logging query syntax
//! - **Root Cause Analysis**: Suggest possible causes and remediation steps
//!
//! # Configuration
//!
//! The Gemini API key can be provided via:
//! 1. `GEMINI_API_KEY` environment variable (recommended)
//! 2. Config file at `~/.config/racoon/config.toml`
//!
//! # Example
//!
//! ```rust,no_run
//! use gcp_racoon::ai::GeminiClient;
//! use gcp_racoon::gcp::LogEntry;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = GeminiClient::from_env()?;
//!
//!     // Explain a log entry
//!     let entry = LogEntry::default();
//!     let explanation = client.explain_log(&entry).await?;
//!     println!("{}", explanation);
//!
//!     // Generate a query from natural language
//!     let query = client.generate_query("errors from Cloud Run in the last hour").await?;
//!     println!("Query: {}", query);
//!
//!     Ok(())
//! }
//! ```
pub mod gemini;
pub mod prompts;

pub use gemini::{AiError, GeminiClient, UserIntent};
