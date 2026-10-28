//! Shared utility functions for racoon.

use std::io::Write;
use std::process::{Command, Stdio};

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
    result = result.replace(['\n', '\r'], " ");

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
/// use gcp_racoon::parse_duration;
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
    let mut chars = s.chars();
    let unit = chars
        .next_back()
        .ok_or_else(|| "Duration cannot be empty".to_string())?;
    let num_str = chars.as_str();
    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("Invalid number: {}", num_str))?;

    let multiplier = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
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

/// Percent-encode a string according to RFC 3986 unreserved character rules.
pub fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

/// Pipe `text` to an external clipboard command's `stdin`.
fn pipe_to_command(program: &str, args: &[&str], text: &str) -> std::io::Result<bool> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    Ok(status.success())
}

/// Copy text to the system clipboard using native OS utilities (`pbcopy`, `wl-copy`/`xclip`/`xsel`, or `clip`).
pub fn copy_to_clipboard(text: &str) -> std::result::Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if pipe_to_command("pbcopy", &[], text).unwrap_or(false) {
            return Ok(());
        }
        Err("Failed to copy to macOS clipboard via 'pbcopy'.".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        if pipe_to_command("clip", &[], text).unwrap_or(false) {
            return Ok(());
        }
        Err("Failed to copy to Windows clipboard via 'clip'.".to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let candidates: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("clip.exe", &[]),
        ];

        for (prog, args) in candidates {
            if pipe_to_command(prog, args, text).unwrap_or(false) {
                return Ok(());
            }
        }

        Err(
            "No clipboard utility found (install 'wl-copy', 'xclip', or 'xsel' on Linux)."
                .to_string(),
        )
    }
}

/// Open a URL in the default system browser (`open`, `xdg-open`, or `cmd /C start`).
pub fn open_in_browser(url: &str) -> std::result::Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("Refusing to open non-HTTP(S) URL: {}", url));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to launch browser via 'open': {}", e))
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to launch browser via 'cmd /C start': {}", e))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        for prog in ["xdg-open", "wslview", "sensible-browser"] {
            if Command::new(prog)
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
        Err("Failed to launch browser (install 'xdg-open' or 'wslview').".to_string())
    }
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
        // Multi-byte UTF-8 unit must return Err cleanly without panicking on byte slice
        assert!(parse_duration("1小时").is_err());
        assert!(parse_duration("30秒").is_err());
    }

    #[test]
    fn test_parse_duration_chrono() {
        let dur = parse_duration_chrono("1h").unwrap();
        assert_eq!(dur.num_seconds(), 3600);
    }

    #[test]
    fn test_url_encode_rfc3986() {
        assert_eq!(
            url_encode("insertId=\"abc-123\""),
            "insertId%3D%22abc-123%22"
        );
        assert_eq!(
            url_encode("2026-10-27T13:00:20+00:00"),
            "2026-10-27T13%3A00%3A20%2B00%3A00"
        );
    }

    #[test]
    fn test_open_in_browser_rejects_non_http_urls() {
        assert!(open_in_browser("file:///etc/passwd").is_err());
        assert!(open_in_browser("javascript:alert(1)").is_err());
    }
}
