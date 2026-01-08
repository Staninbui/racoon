//! racoon - GCP Log Spy with AI
//!
//! it provides a simple interface for checking gcp logs with AI

use tracing_subscriber::filter::filter_fn;

pub mod cli;

/// todo:
/// - pub mod gcp;
/// - pub mod ai;
/// - pub mod ui;
/// - pub mod storage;
///
///
///
///   parse time range syntax like `1h`, `5m`, `30s`
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
pub fn parse_duration(s: &str) -> Result<u64, String> {
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
}