//! Shared utility functions for racoon.

use chrono::{Duration, Utc};
use tracing::debug;

/// Sanitize an AI-generated query to make it valid for GCP Logging API.
///
/// Common issues fixed:
/// - Removes markdown code blocks
/// - Converts relative time (e.g., "-2h") to RFC3339 timestamps
/// - Removes newlines
pub fn sanitize_query(query: &str) -> String {
    let mut result = query.trim().to_string();

    // Remove markdown code blocks if present
    result = result
        .trim_start_matches("```")
        .trim_start_matches("gcp")
        .trim_start_matches("sql")
        .trim_end_matches("```")
        .trim()
        .to_string();

    // Remove newlines
    result = result.replace('\n', " ").replace('\r', " ");

    // Collapse multiple spaces
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    // Fix relative time strings
    result = fix_relative_timestamps(&result);

    result
}

/// Convert relative time patterns to RFC3339 timestamps.
///
/// Handles patterns like:
/// - "-2h" -> actual timestamp 2 hours ago
/// - "-1d" -> actual timestamp 1 day ago
/// - "-30m" -> actual timestamp 30 minutes ago
fn fix_relative_timestamps(query: &str) -> String {
    let mut result = query.to_string();

    let patterns = [
        (r#""-(\d+)h""#, "hours"),
        (r#""-(\d+)d""#, "days"),
        (r#""-(\d+)m""#, "minutes"),
        (r#""-(\d+)s""#, "seconds"),
    ];

    for (pattern, unit) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            let replacements: Vec<(String, String)> = re
                .captures_iter(&result.clone())
                .filter_map(|captures| {
                    let num_match = captures.get(1)?;
                    let num: i64 = num_match.as_str().parse().ok()?;

                    let duration = match unit {
                        "hours" => Duration::hours(num),
                        "days" => Duration::days(num),
                        "minutes" => Duration::minutes(num),
                        "seconds" => Duration::seconds(num),
                        _ => return None,
                    };

                    let timestamp = Utc::now() - duration;
                    let rfc3339 = timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string();

                    let old_value = captures.get(0)?.as_str().to_string();
                    let new_value = format!("\"{}\"", rfc3339);

                    Some((old_value, new_value))
                })
                .collect();

            for (old_value, new_value) in replacements {
                debug!(old = %old_value, new = %new_value, "Converted relative timestamp");
                result = result.replace(&old_value, &new_value);
            }
        }
    }

    result
}

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