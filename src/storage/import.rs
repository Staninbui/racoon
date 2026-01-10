//! Log import (load) functionality.
//!
//! Provides functions to load previously saved logs from local files.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use thiserror::Error;
use tracing::{debug, info};

use super::formats::ExportFormat;
use crate::gcp::LogEntry;

/// Errors that can occur during import operations.
#[derive(Error, Debug)]
pub enum ImportError {
    /// Failed to read file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse log entries.
    #[error("Parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    /// File is empty.
    #[error("File is empty or contains no valid entries")]
    Empty,

    /// Unsupported format.
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),
}

/// Result of an import operation.
#[derive(Debug)]
pub struct ImportResult {
    /// Loaded log entries.
    pub entries: Vec<LogEntry>,

    /// Source file path.
    pub path: String,

    /// Format detected.
    pub format: ExportFormat,

    /// Number of entries loaded.
    pub count: usize,

    /// Number of entries that failed to parse (for NDJSON).
    pub errors: usize,
}

/// Import logs from a local file.
///
/// Automatically detects format based on file extension and content.
///
/// # Arguments
///
/// * `input_path` - Path to the log file
///
/// # Returns
///
/// Import result with loaded entries.
///
/// # Example
///
/// ```rust,no_run
/// use racoon::storage::import_logs;
///
/// let result = import_logs("logs.json")?;
/// println!("Loaded {} logs", result.count);
/// for entry in result.entries {
///     println!("{}", entry.message_preview());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn import_logs(input_path: impl AsRef<Path>) -> Result<ImportResult, ImportError> {
    let input_path = input_path.as_ref();

    info!(path = %input_path.display(), "Importing logs");

    // Detect format
    let format = ExportFormat::from_path(input_path);

    let (entries, errors) = match format {
        ExportFormat::Json => (import_json(input_path)?, 0),
        ExportFormat::Ndjson => import_ndjson(input_path)?,
    };

    let count = entries.len();

    if count == 0 && errors == 0 {
        return Err(ImportError::Empty);
    }

    info!(
        count = count,
        errors = errors,
        format = ?format,
        "Import complete"
    );

    Ok(ImportResult {
        entries,
        path: input_path.to_string_lossy().to_string(),
        format,
        count,
        errors,
    })
}

/// Import from JSON format.
fn import_json(path: &Path) -> Result<Vec<LogEntry>, ImportError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Try to parse as wrapped format first (with _metadata)
    let value: serde_json::Value = serde_json::from_reader(reader)
        .map_err(|e| ImportError::Parse {
            line: e.line(),
            message: e.to_string(),
        })?;

    // Check if it's wrapped format
    if let Some(entries) = value.get("entries") {
        debug!("Detected wrapped JSON format with metadata");
        let entries: Vec<LogEntry> = serde_json::from_value(entries.clone())
            .map_err(|e| ImportError::Parse {
                line: 0,
                message: format!("Failed to parse entries array: {}", e),
            })?;
        return Ok(entries);
    }

    // Try as plain array
    if value.is_array() {
        debug!("Detected plain JSON array format");
        let entries: Vec<LogEntry> = serde_json::from_value(value)
            .map_err(|e| ImportError::Parse {
                line: 0,
                message: format!("Failed to parse as array: {}", e),
            })?;
        return Ok(entries);
    }

    // Single entry
    if value.is_object() {
        debug!("Detected single JSON object");
        let entry: LogEntry = serde_json::from_value(value)
            .map_err(|e| ImportError::Parse {
                line: 0,
                message: format!("Failed to parse as log entry: {}", e),
            })?;
        return Ok(vec![entry]);
    }

    Err(ImportError::Parse {
        line: 0,
        message: "Unable to detect JSON structure".to_string(),
    })
}

/// Import from NDJSON format.
fn import_ndjson(path: &Path) -> Result<(Vec<LogEntry>, usize), ImportError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut entries = Vec::new();
    let mut errors = 0;

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<LogEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                debug!(line = line_num + 1, error = %e, "Failed to parse line");
                errors += 1;
            }
        }
    }

    Ok((entries, errors))
}

/// Load logs with limit.
///
/// Useful for previewing large files.
///
/// # Arguments
///
/// * `input_path` - Path to the log file
/// * `limit` - Maximum number of entries to load
pub fn import_logs_limited(
    input_path: impl AsRef<Path>,
    limit: usize,
) -> Result<ImportResult, ImportError> {
    let mut result = import_logs(input_path)?;

    if result.entries.len() > limit {
        result.entries.truncate(limit);
        result.count = limit;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::io::Write;

    #[test]
    fn test_import_json_array() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");

        let entries = vec![LogEntry::default(), LogEntry::default()];
        let json = serde_json::to_string(&entries).unwrap();
        std::fs::write(&path, json).unwrap();

        let result = import_logs(&path).unwrap();
        assert_eq!(result.count, 2);
        assert_eq!(result.format, ExportFormat::Json);
    }

    #[test]
    fn test_import_ndjson() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.ndjson");

        let mut file = File::create(&path).unwrap();
        for _ in 0..3 {
            writeln!(file, "{}", serde_json::to_string(&LogEntry::default()).unwrap()).unwrap();
        }

        let result = import_logs(&path).unwrap();
        assert_eq!(result.count, 3);
        assert_eq!(result.format, ExportFormat::Ndjson);
    }

    #[test]
    fn test_import_empty_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.json");
        std::fs::write(&path, "[]").unwrap();

        let result = import_logs(&path);
        assert!(matches!(result, Err(ImportError::Empty)));
    }

    #[test]
    fn test_import_limited() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");

        let entries: Vec<LogEntry> = (0..100).map(|_| LogEntry::default()).collect();
        let json = serde_json::to_string(&entries).unwrap();
        std::fs::write(&path, json).unwrap();

        let result = import_logs_limited(&path, 10).unwrap();
        assert_eq!(result.count, 10);
    }
}