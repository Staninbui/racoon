//! racoon - GCP Log Spy with AI superpowers 🦝
//!
//! This crate provides real-time GCP log streaming, AI-powered analysis,
//! and natural language query generation capabilities.
//!
//! # Modules
//!
//! - [`cli`]: Command-line interface definitions using clap.
//! - [`gcp`]: Google Cloud Platform integration (auth, logging).
//! - [`error`]: Application-level error types.
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
/// todo:
/// - pub mod ai;
/// - pub mod ui;
/// - pub mod storage;

// Re-export commonly used types at crate root
pub use error::{RacoonError, Result};
pub use gcp::{GcpAuth, LogEntry, LogFilter, LogsClient};

/// parse time range syntax like `1h`, `5m`, `30s`
///
/// # Examples
///
/// ```
/// use racoon::parse_duration;
///
/// assert_eq!(parse_duration("30s"), Ok(30));
/// assert_eq!(parse_duration("5m"), Ok(300));
/// assert_eq!(parse_duration("1h"), Ok(3600));
/// assert_eq!(parse_duration("2d"), Ok(172800));
/// ```
///
/// # Errors
///
/// Returns an error string if the input is empty, contains an invalid number,
/// or uses an unknown unit.
///
pub fn parse_duration(s: &str) -> std::result::Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("Duration cannot be empty".to_string())
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str.parse().map_err(|_| format!("Invalid number: {}", num_str))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(format!("Unknown unit: {}. Use s/m/h/d", unit)),
    };

    Ok(num * multiplier)
}

/// Parse a duration string (e.g., "1h", "30m") into a chrono::Duration.
///
/// This version returns a chrono::Duration for use with the LogFilter.
pub fn parse_duration_chrono(s: &str) -> std::result::Result<chrono::Duration, String> {
    let seconds = parse_duration(s)?;
    Ok(chrono::Duration::seconds(seconds as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30s"), Ok(30));
        assert_eq!(parse_duration("5m"), Ok(300));
        assert_eq!(parse_duration("1h"), Ok(3600));
        assert_eq!(parse_duration("2d"), Ok(172800));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("100x").is_err());
    }

    #[test]
    fn test_parse_duration_chrono() {
        let dur = parse_duration_chrono("1h").unwrap();
        assert_eq!(dur.num_seconds(), 3600);
    }
}