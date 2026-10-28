//! Log export (download) functionality.
//!
//! Provides functions to download logs from GCP and save them to local files.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::Utc;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, info};

use super::formats::ExportFormat;
use crate::ai::GeminiClient;
use crate::gcp::{LogEntry, LogFilter, LogsClient, LogsError};
use crate::utils::sanitize_query;

/// Errors that can occur during export operations.
#[derive(Error, Debug)]
pub enum ExportError {
    /// Failed to create or write to file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to serialize logs.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Failed to fetch logs from GCP.
    #[error("GCP error: {0}")]
    Gcp(#[from] LogsError),

    /// AI query generation failed.
    #[error("AI error: {0}")]
    Ai(#[from] crate::ai::AiError),
}

/// Options for log export.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Output format (JSON or NDJSON).
    pub format: ExportFormat,

    /// Pretty-print JSON output (only for JSON format).
    pub pretty: bool,

    /// Include export metadata in the file.
    pub include_metadata: bool,

    /// Maximum number of logs to export.
    pub limit: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Json,
            pretty: true,
            include_metadata: true,
            limit: 1000,
        }
    }
}

/// Result of an export operation.
#[derive(Debug)]
pub struct ExportResult {
    /// Number of logs exported.
    pub count: usize,

    /// Path to the output file.
    pub path: String,

    /// Format used for export.
    pub format: ExportFormat,

    /// Size of the output file in bytes.
    pub size_bytes: u64,
}

/// Export metadata included in JSON exports.
#[derive(Debug, Serialize)]
struct ExportMetadata {
    /// racoon version.
    version: String,
    /// Export timestamp.
    exported_at: String,
    /// GCP project ID.
    project: String,
    /// Filter used (if any).
    filter: Option<String>,
    /// Number of entries.
    count: usize,
}

/// Export wrapper for JSON with metadata.
#[derive(Debug, Serialize)]
struct ExportWrapper {
    /// Export metadata.
    #[serde(rename = "_metadata")]
    metadata: ExportMetadata,
    /// Log entries.
    entries: Vec<LogEntry>,
}

/// Export logs from GCP to a local file.
///
/// # Arguments
///
/// * `client` - The GCP logs client
/// * `filter` - Optional filter for the logs
/// * `output_path` - Path to write the output file
/// * `options` - Export options
///
/// # Returns
///
/// Export result with count and file info.
///
/// # Example
///
/// ```rust,no_run
/// use gcp_racoon::storage::{export_logs, ExportOptions};
/// use gcp_racoon::gcp::{LogsClient, LogFilter};
///
/// # async fn example(client: LogsClient) -> Result<(), Box<dyn std::error::Error>> {
/// let filter = LogFilter::new().severity_min("ERROR");
/// let options = ExportOptions::default();
/// let result = export_logs(&client, Some(&filter), "errors.json", options).await?;
/// println!("Exported {} logs", result.count);
/// # Ok(())
/// # }
/// ```
pub async fn export_logs(
    client: &LogsClient,
    filter: Option<&LogFilter>,
    output_path: impl AsRef<Path>,
    options: ExportOptions,
) -> Result<ExportResult, ExportError> {
    let output_path = output_path.as_ref();

    info!(
        path = %output_path.display(),
        format = ?options.format,
        limit = options.limit,
        "Exporting logs"
    );

    // Use explicitly configured format from ExportOptions
    let format = options.format;

    // Create output file
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);
    let mut count = 0usize;

    // Write based on format
    match format {
        ExportFormat::Json => {
            // Fetch logs across pages up to `options.limit`
            let entries = client.list_entries(filter, options.limit).await?;
            count = entries.len();
            debug!("Fetched {} log entries", count);

            if options.include_metadata {
                let wrapper = ExportWrapper {
                    metadata: ExportMetadata {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        exported_at: Utc::now().to_rfc3339(),
                        project: client.project().to_string(),
                        filter: filter.map(|f| f.build()),
                        count,
                    },
                    entries,
                };

                if options.pretty {
                    serde_json::to_writer_pretty(&mut writer, &wrapper)?;
                } else {
                    serde_json::to_writer(&mut writer, &wrapper)?;
                }
            } else if options.pretty {
                serde_json::to_writer_pretty(&mut writer, &entries)?;
            } else {
                serde_json::to_writer(&mut writer, &entries)?;
            }
        }
        ExportFormat::Ndjson => {
            // Stream pages directly to BufWriter for constant-memory export
            let mut page_token: Option<String> = None;
            while count < options.limit {
                let batch_size = (options.limit - count).min(1000);
                let (batch, next_token) = client
                    .list_entries_paged(filter, batch_size, page_token.as_deref())
                    .await?;

                let batch_empty = batch.is_empty();
                for entry in &batch {
                    if count >= options.limit {
                        break;
                    }
                    serde_json::to_writer(&mut writer, entry)?;
                    writeln!(&mut writer)?;
                    count += 1;
                }

                match next_token {
                    Some(token) if !token.is_empty() && !batch_empty && count < options.limit => {
                        page_token = Some(token);
                    }
                    _ => break,
                }
            }
            debug!("Streamed {} log entries to NDJSON", count);
        }
    }

    writer.flush()?;

    // Get file size
    let size_bytes = std::fs::metadata(output_path)?.len();

    info!(count = count, size_bytes = size_bytes, "Export complete");

    Ok(ExportResult {
        count,
        path: output_path.to_string_lossy().to_string(),
        format,
        size_bytes,
    })
}

/// Export logs using AI-generated query from natural language.
///
/// # Arguments
///
/// * `client` - The GCP logs client
/// * `gemini` - The Gemini AI client
/// * `description` - Natural language description of logs to download
/// * `output_path` - Path to write the output file
/// * `options` - Export options
///
/// # Returns
///
/// A tuple of (generated query, export result).
pub async fn export_logs_with_ai(
    client: &LogsClient,
    gemini: &GeminiClient,
    description: &str,
    output_path: impl AsRef<Path>,
    options: ExportOptions,
) -> Result<(String, ExportResult), ExportError> {
    info!(description = description, "Generating query with AI");

    // Generate query from natural language
    let raw_query = gemini.generate_query(description).await?;
    debug!(raw_query = %raw_query, "AI generated raw query");
    let query = sanitize_query(&raw_query);
    info!(query = %query, "Sanitized query");

    // Create filter from generated query
    let filter = LogFilter::new().raw(&query);

    // Export using the generated filter
    let result = export_logs(client, Some(&filter), output_path, options).await?;

    Ok((query, result))
}

/// Append logs to an existing NDJSON file.
///
/// This is useful for incremental exports.
///
/// # Arguments
///
/// * `entries` - Log entries to append
/// * `output_path` - Path to the NDJSON file
///
/// # Returns
///
/// Number of entries appended.
pub fn append_logs(
    entries: &[LogEntry],
    output_path: impl AsRef<Path>,
) -> Result<usize, ExportError> {
    let output_path = output_path.as_ref();

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path)?;

    let mut writer = BufWriter::new(file);

    for entry in entries {
        serde_json::to_writer(&mut writer, entry)?;
        writeln!(&mut writer)?;
    }

    writer.flush()?;

    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_export_options_default() {
        let options = ExportOptions::default();
        assert_eq!(options.format, ExportFormat::Json);
        assert!(options.pretty);
        assert!(options.include_metadata);
        assert_eq!(options.limit, 1000);
    }

    #[test]
    fn test_append_logs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.ndjson");

        let entries = vec![LogEntry::default(), LogEntry::default()];

        let count = append_logs(&entries, &path).unwrap();
        assert_eq!(count, 2);

        // Append more
        let count = append_logs(&entries, &path).unwrap();
        assert_eq!(count, 2);

        // Verify file content
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = content.lines().collect();
        assert_eq!(lines.len(), 4);
    }
}
