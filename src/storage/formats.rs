//! File format handling for log storage.

use std::path::Path;

/// Supported export formats for log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    /// JSON array format - all logs in a single array.
    /// Human-readable and easy to parse.
    #[default]
    Json,

    /// Newline-delimited JSON - one log per line.
    /// Better for streaming and large files.
    Ndjson,
}

impl ExportFormat {
    /// Detect format from file extension.
    ///
    /// # Arguments
    ///
    /// * `path` - File path to check
    ///
    /// # Returns
    ///
    /// The detected format, or `Json` as default.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("ndjson") | Some("jsonl") => Self::Ndjson,
            _ => Self::Json,
        }
    }

    /// Get the typical file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
        }
    }

    /// Get a human-readable name for this format.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Ndjson => "NDJSON (Newline Delimited)",
        }
    }
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "ndjson" | "jsonl" => Ok(Self::Ndjson),
            _ => Err(format!("Unknown format: {}. Use 'json' or 'ndjson'", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_from_path() {
        assert_eq!(
            ExportFormat::from_path(Path::new("logs.json")),
            ExportFormat::Json
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("logs.ndjson")),
            ExportFormat::Ndjson
        );
        assert_eq!(
            ExportFormat::from_path(Path::new("logs.jsonl")),
            ExportFormat::Ndjson
        );
        // Default to JSON
        assert_eq!(
            ExportFormat::from_path(Path::new("logs.txt")),
            ExportFormat::Json
        );
    }

    #[test]
    fn test_format_from_str() {
        assert_eq!("json".parse::<ExportFormat>().unwrap(), ExportFormat::Json);
        assert_eq!("ndjson".parse::<ExportFormat>().unwrap(), ExportFormat::Ndjson);
        assert!("invalid".parse::<ExportFormat>().is_err());
    }
}