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
/// use gcp_racoon::storage::import_logs;
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

    if input_path == Path::new("-") {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut buf)?;
        return import_logs_from_str(&buf, "stdin");
    }

    info!(path = %input_path.display(), "Importing logs");

    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if matches!(ext.as_str(), "log" | "txt") {
        let content = std::fs::read_to_string(input_path)?;
        return import_logs_from_str(&content, &input_path.to_string_lossy());
    }

    // Detect format
    let format = ExportFormat::from_path(input_path);

    let (entries, errors) = match format {
        ExportFormat::Json => match import_json(input_path) {
            Ok(items) => (items, 0),
            Err(json_err) => {
                // Fallback to line-based NDJSON / plain-text parser if file isn't a single JSON document
                if let Ok(raw) = std::fs::read_to_string(input_path)
                    && let Ok(fallback) = import_logs_from_str(&raw, &input_path.to_string_lossy())
                {
                    return Ok(fallback);
                }
                return Err(json_err);
            }
        },
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

/// Import logs from an in-memory string (supports JSON array, wrapped JSON, NDJSON, and plain-text log lines).
///
/// Used by `racoon view -` (`stdin` pipe) and plain-text `.log` / `.txt` files.
pub fn import_logs_from_str(
    content: &str,
    source_label: &str,
) -> Result<ImportResult, ImportError> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(ImportError::Empty);
    }

    // 1. Try parsing as a complete JSON document (array, wrapped `{"entries": [...]}`, or single object)
    if (trimmed.starts_with('[') || trimmed.starts_with('{'))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        if let Some(entries_val) = value.get("entries") {
            let entries: Vec<LogEntry> =
                serde_json::from_value(entries_val.clone()).map_err(|e| ImportError::Parse {
                    line: 0,
                    message: format!("Failed to parse entries array: {}", e),
                })?;
            if entries.is_empty() {
                return Err(ImportError::Empty);
            }
            let count = entries.len();
            return Ok(ImportResult {
                entries,
                path: source_label.to_string(),
                format: ExportFormat::Json,
                count,
                errors: 0,
            });
        }
        if value.is_array() {
            let entries: Vec<LogEntry> =
                serde_json::from_value(value).map_err(|e| ImportError::Parse {
                    line: 0,
                    message: format!("Failed to parse JSON array: {}", e),
                })?;
            if entries.is_empty() {
                return Err(ImportError::Empty);
            }
            let count = entries.len();
            return Ok(ImportResult {
                entries,
                path: source_label.to_string(),
                format: ExportFormat::Json,
                count,
                errors: 0,
            });
        }
        if value.is_object() {
            let entry: LogEntry =
                serde_json::from_value(value).map_err(|e| ImportError::Parse {
                    line: 0,
                    message: format!("Failed to parse JSON log entry: {}", e),
                })?;
            return Ok(ImportResult {
                entries: vec![entry],
                path: source_label.to_string(),
                format: ExportFormat::Json,
                count: 1,
                errors: 0,
            });
        }
    }

    // 2. Line-by-line NDJSON or plain-text log stream (`kubectl logs`, `docker logs`, `gcloud`)
    let mut entries = Vec::new();
    for (idx, raw_line) in trimmed.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('{')
            && let Ok(entry) = serde_json::from_str::<LogEntry>(line)
        {
            entries.push(entry);
            continue;
        }
        entries.push(parse_plain_text_log_line(line, idx + 1));
    }

    if entries.is_empty() {
        return Err(ImportError::Empty);
    }

    let count = entries.len();
    Ok(ImportResult {
        entries,
        path: source_label.to_string(),
        format: ExportFormat::Ndjson,
        count,
        errors: 0,
    })
}

/// Parse a single unstructured / plain-text log line into a [`LogEntry`], extracting
/// leading RFC3339/ISO timestamps and standard severity tokens (`ERROR`, `WARN`, `INFO`, `DEBUG`, `CRITICAL`).
pub fn parse_plain_text_log_line(line: &str, line_num: usize) -> LogEntry {
    let mut rest = line.trim();
    let mut timestamp = None;

    // Check if the first token is an RFC3339 timestamp (e.g. `2026-10-28T12:00:01Z`)
    if let Some((first, after_first)) = rest.split_once(char::is_whitespace)
        && let Ok(dt) =
            chrono::DateTime::parse_from_rfc3339(first.trim_matches(|c| c == '[' || c == ']'))
    {
        timestamp = Some(dt.with_timezone(&chrono::Utc));
        rest = after_first.trim_start();
    }

    // Check if the next token is a severity keyword (`ERROR`, `[WARN]`, `INFO:`, etc.)
    let mut severity = "INFO".to_string();
    if let Some(first_word) = rest.split_whitespace().next() {
        let clean = first_word
            .trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | ':'))
            .to_uppercase();
        let matched_sev = match clean.as_str() {
            "DEBUG" | "TRACE" => Some("DEBUG"),
            "INFO" => Some("INFO"),
            "NOTICE" => Some("NOTICE"),
            "WARN" | "WARNING" => Some("WARNING"),
            "ERR" | "ERROR" => Some("ERROR"),
            "CRIT" | "CRITICAL" | "FATAL" | "PANIC" => Some("CRITICAL"),
            "ALERT" => Some("ALERT"),
            "EMERG" | "EMERGENCY" => Some("EMERGENCY"),
            _ => None,
        };
        if let Some(sev) = matched_sev {
            severity = sev.to_string();
            rest = rest[first_word.len()..].trim_start();
        }
    }

    let message_text = if rest.is_empty() {
        line.trim().to_string()
    } else {
        rest.to_string()
    };

    LogEntry {
        insert_id: Some(format!("line-{}", line_num)),
        timestamp,
        severity: Some(severity),
        text_payload: Some(message_text),
        ..Default::default()
    }
}

/// Import from JSON format.
fn import_json(path: &Path) -> Result<Vec<LogEntry>, ImportError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    // Try to parse as wrapped format first (with _metadata)
    let value: serde_json::Value =
        serde_json::from_reader(reader).map_err(|e| ImportError::Parse {
            line: e.line(),
            message: e.to_string(),
        })?;

    // Check if it's wrapped format
    if let Some(entries) = value.get("entries") {
        debug!("Detected wrapped JSON format with metadata");
        let entries: Vec<LogEntry> =
            serde_json::from_value(entries.clone()).map_err(|e| ImportError::Parse {
                line: 0,
                message: format!("Failed to parse entries array: {}", e),
            })?;
        return Ok(entries);
    }

    // Try as plain array
    if value.is_array() {
        debug!("Detected plain JSON array format");
        let entries: Vec<LogEntry> =
            serde_json::from_value(value).map_err(|e| ImportError::Parse {
                line: 0,
                message: format!("Failed to parse as array: {}", e),
            })?;
        return Ok(entries);
    }

    // Single entry
    if value.is_object() {
        debug!("Detected single JSON object");
        let entry: LogEntry = serde_json::from_value(value).map_err(|e| ImportError::Parse {
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
    use std::io::Write;
    use tempfile::tempdir;

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
            writeln!(
                file,
                "{}",
                serde_json::to_string(&LogEntry::default()).unwrap()
            )
            .unwrap();
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

    #[test]
    fn test_import_logs_from_str_json_ndjson_and_plain_text() {
        let plain_logs = "\
2026-10-28T12:00:01Z ERROR [payment-svc] Connection refused to 10.0.0.1:5432
2026-10-28T12:00:02Z WARN Retrying request 1/3
Just a plain stdout line without timestamp";

        let res = import_logs_from_str(plain_logs, "stdin").unwrap();
        assert_eq!(res.count, 3);
        assert_eq!(res.entries[0].severity_or_default(), "ERROR");
        assert!(
            res.entries[0]
                .message()
                .contains("Connection refused to 10.0.0.1:5432")
        );
        assert!(res.entries[0].timestamp.is_some());
        assert_eq!(res.entries[1].severity_or_default(), "WARNING");
        assert_eq!(res.entries[2].severity_or_default(), "INFO");
    }
}
