//! Shared utility functions for racoon.
//!
//! Includes duration parsing, AI query sanitization, HTTP client construction
//! with safe default timeouts, log message fingerprinting, RFC 3986 URL encoding,
//! and cross-platform clipboard / browser helpers.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use regex::{Captures, Regex};
use reqwest::Client;
use tracing::debug;

/// Default TCP connect timeout for outbound HTTP requests (10 seconds).
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default total request timeout for outbound HTTP requests (30 seconds).
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Construct a shared `reqwest::Client` configured with safe production timeouts
/// (`connect_timeout = 10s`, `timeout = 30s`) and a standard User-Agent header.
pub fn default_http_client() -> Client {
    Client::builder()
        .user_agent(concat!("gcp-racoon/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(StdDuration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
        .timeout(StdDuration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Normalize a log message into a canonical fingerprint by replacing variable tokens
/// (UUIDs, IPv4 addresses, timestamps, hex strings, numbers) with placeholders.
///
/// Placed in `utils` so both the `ai` layer (compaction & LRU cache) and the `ui`
/// layer (Error Spike Radar) can share it without reverse module dependencies.
pub fn fingerprint_message(msg: &str) -> String {
    static RE_UUID: OnceLock<Regex> = OnceLock::new();
    static RE_IP: OnceLock<Regex> = OnceLock::new();
    static RE_ISO_TS: OnceLock<Regex> = OnceLock::new();
    static RE_HEX: OnceLock<Regex> = OnceLock::new();
    static RE_NUM: OnceLock<Regex> = OnceLock::new();

    let re_uuid = RE_UUID.get_or_init(|| {
        Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
    });
    let re_ip = RE_IP.get_or_init(|| Regex::new(r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b").unwrap());
    let re_iso_ts = RE_ISO_TS.get_or_init(|| {
        Regex::new(r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?\b").unwrap()
    });
    let re_hex =
        RE_HEX.get_or_init(|| Regex::new(r"(?i)\b(?:0x[0-9a-f]+|[0-9a-f]{16,})\b").unwrap());
    let re_num = RE_NUM.get_or_init(|| Regex::new(r"\b\d+").unwrap());

    let s = msg.replace(['\n', '\r'], " ");
    let s = re_uuid.replace_all(&s, "<UUID>");
    let s = re_iso_ts.replace_all(&s, "<TS>");
    let s = re_ip.replace_all(&s, "<IP>");
    let s = re_hex.replace_all(&s, "<HEX>");
    let s = re_num.replace_all(&s, "<N>");

    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 110 {
        let truncated: String = collapsed.chars().take(107).collect();
        format!("{}...", truncated)
    } else {
        collapsed
    }
}

/// Sanitize an AI-generated query to make it valid for GCP Logging API.
///
/// Common issues fixed:
/// - Removes markdown code blocks (` ```gcp `, ` ```sql `, ` ```text `, etc.)
/// - Converts relative time (e.g., `"-2h"`) to RFC3339 timestamps
/// - Removes newlines and collapses redundant whitespace
pub fn sanitize_query(query: &str) -> String {
    let mut result = query.trim().to_string();

    if let Some(stripped) = result.strip_prefix("```") {
        let stripped = stripped
            .strip_prefix("gcp")
            .or_else(|| stripped.strip_prefix("sql"))
            .or_else(|| stripped.strip_prefix("text"))
            .or_else(|| stripped.strip_prefix("json"))
            .unwrap_or(stripped);
        result = stripped.trim_end_matches("```").trim().to_string();
    } else if result.ends_with("```") {
        result = result.trim_end_matches("```").trim().to_string();
    }

    // Collapse newlines and multiple spaces into single spaces
    result = result.split_whitespace().collect::<Vec<_>>().join(" ");

    // Fix relative time strings
    fix_relative_timestamps(&result)
}

/// Convert relative time patterns (`"-2h"`, `"-1d"`, `"-30m"`, `"-45s"`) to RFC3339 timestamps
/// using a single pre-compiled `OnceLock<Regex>`.
fn fix_relative_timestamps(query: &str) -> String {
    static RE_REL_TIME: OnceLock<Regex> = OnceLock::new();
    let re = RE_REL_TIME.get_or_init(|| Regex::new(r#""-(\d+)([hdms])""#).unwrap());

    let now = Utc::now();
    re.replace_all(query, |caps: &Captures| {
        let Ok(num) = caps[1].parse::<i64>() else {
            return caps[0].to_string();
        };
        let duration = match &caps[2] {
            "h" => Duration::hours(num),
            "d" => Duration::days(num),
            "m" => Duration::minutes(num),
            "s" => Duration::seconds(num),
            _ => return caps[0].to_string(),
        };
        let timestamp = now - duration;
        let rfc3339 = timestamp.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let new_value = format!("\"{}\"", rfc3339);
        debug!(old = %&caps[0], new = %new_value, "Converted relative timestamp");
        new_value
    })
    .into_owned()
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
    fn test_sanitize_query_strips_fences_and_resolves_relative_timestamps() {
        let raw = "```gcp\nseverity>=ERROR\n  timestamp>=\"-2h\"\n```";
        let sanitized = sanitize_query(raw);
        assert!(sanitized.starts_with("severity>=ERROR timestamp>=\"20"));
        assert!(sanitized.ends_with("Z\""));
        assert!(!sanitized.contains("```"));
        assert!(!sanitized.contains("-2h"));
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
