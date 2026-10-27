//! Error Fingerprinting & Spike Radar engine (`c` key).
//!
//! Normalizes dynamic tokens (UUIDs, IPs, timestamps, hex strings, numbers)
//! in `WARNING` / `ERROR` / `CRITICAL` log messages to cluster repetitive error storms
//! into canonical patterns and render a real-time Unicode velocity sparkline.

use regex::Regex;
use std::sync::OnceLock;

use crate::gcp::LogEntry;

/// A clustered group of logs sharing the same normalized error fingerprint.
#[derive(Debug, Clone)]
pub struct ErrorCluster {
    /// Canonical normalized message template (e.g., `"Connection timeout to <IP>:<N> after <N>ms"`).
    pub fingerprint: String,
    /// Highest severity observed in this cluster (`WARNING`, `ERROR`, `CRITICAL`, etc.).
    pub severity: String,
    /// Number of occurrences matching this fingerprint.
    pub count: usize,
    /// Timestamp (`HH:MM:SS`) when this pattern was first seen.
    pub first_seen: String,
    /// Timestamp (`HH:MM:SS`) when this pattern was most recently seen.
    pub last_seen: String,
    /// Representative log entry (most recent) for inspection and AI root-cause explanation.
    pub sample_entry: LogEntry,
    /// Whether this cluster is flagged as a high-frequency spike (`count >= 3` or `CRITICAL`).
    pub is_spike: bool,
}

/// Real-time error clustering and velocity radar state.
#[derive(Debug, Clone, Default)]
pub struct SpikeRadar {
    /// Ranked error clusters (ordered by `count` descending).
    pub clusters: Vec<ErrorCluster>,
    /// Total count of warning/error/critical entries processed.
    pub total_error_logs: usize,
    /// Rolling window of error counts across recent log batches (up to 8 buckets) for sparkline rendering.
    pub velocity_buckets: Vec<usize>,
    /// Counter of entries seen in the current batch window.
    entries_in_current_window: usize,
    /// Counter of error entries in the current batch window.
    errors_in_current_window: usize,
}

const WINDOW_SIZE_ENTRIES: usize = 10;
const MAX_SPARKLINE_BUCKETS: usize = 8;

impl SpikeRadar {
    /// Create a new empty `SpikeRadar`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all clusters and velocity buckets.
    pub fn clear(&mut self) {
        self.clusters.clear();
        self.total_error_logs = 0;
        self.velocity_buckets.clear();
        self.entries_in_current_window = 0;
        self.errors_in_current_window = 0;
    }

    /// Rebuild clusters and velocity buckets from a collection of log entries.
    pub fn rebuild_from_logs<'a>(&mut self, logs: impl IntoIterator<Item = &'a LogEntry>) {
        self.clear();
        for entry in logs {
            self.record_entry(entry);
        }
    }

    /// Ingest a single `LogEntry`, updating velocity buckets and error clusters if severity >= WARNING.
    pub fn record_entry(&mut self, entry: &LogEntry) {
        let sev = entry.severity_or_default().to_uppercase();
        let is_err_or_warn = matches!(
            sev.as_str(),
            "WARNING" | "WARN" | "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY"
        );

        self.entries_in_current_window += 1;
        if is_err_or_warn {
            self.errors_in_current_window += 1;
            self.total_error_logs += 1;
        }

        if self.entries_in_current_window >= WINDOW_SIZE_ENTRIES {
            self.velocity_buckets.push(self.errors_in_current_window);
            if self.velocity_buckets.len() > MAX_SPARKLINE_BUCKETS {
                self.velocity_buckets.remove(0);
            }
            self.entries_in_current_window = 0;
            self.errors_in_current_window = 0;
        }

        if !is_err_or_warn {
            return;
        }

        let fp = fingerprint_message(&entry.message_preview());
        let ts = entry.timestamp_str();

        if let Some(existing) = self.clusters.iter_mut().find(|c| c.fingerprint == fp) {
            existing.count += 1;
            existing.last_seen = ts;
            if severity_weight(&sev) > severity_weight(&existing.severity) {
                existing.severity = sev.clone();
            }
            existing.sample_entry = entry.clone();
            existing.is_spike = existing.count >= 3 || existing.severity == "CRITICAL";
        } else {
            let is_spike = sev == "CRITICAL" || sev == "ALERT" || sev == "EMERGENCY";
            self.clusters.push(ErrorCluster {
                fingerprint: fp,
                severity: sev,
                count: 1,
                first_seen: ts.clone(),
                last_seen: ts,
                sample_entry: entry.clone(),
                is_spike,
            });
        }

        // Keep clusters sorted by count descending, then severity weight descending
        self.clusters.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| severity_weight(&b.severity).cmp(&severity_weight(&a.severity)))
        });
    }

    /// Render a compact Unicode sparkline (`▁▂▃▄▅▆▇█`) of recent error velocity.
    pub fn sparkline(&self) -> String {
        const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let mut buckets = self.velocity_buckets.clone();
        if self.entries_in_current_window > 0 || buckets.is_empty() {
            buckets.push(self.errors_in_current_window);
        }
        if buckets.len() > MAX_SPARKLINE_BUCKETS {
            buckets.remove(0);
        }

        let max_val = buckets.iter().copied().max().unwrap_or(0);
        if max_val == 0 {
            return "▁".repeat(buckets.len().max(4));
        }

        buckets
            .into_iter()
            .map(|v| {
                if v == 0 {
                    '▁'
                } else {
                    let idx = ((v * 7) / max_val).min(7);
                    BARS[idx]
                }
            })
            .collect()
    }

    /// Return true if any error cluster is currently in a spike state.
    pub fn has_active_spike(&self) -> bool {
        self.clusters.iter().any(|c| c.is_spike)
    }

    /// Compact summary string for the TUI header bar.
    pub fn header_summary(&self) -> String {
        if self.clusters.is_empty() {
            "🟢 0 errs".to_string()
        } else {
            let icon = if self.has_active_spike() {
                "🚨"
            } else {
                "🔥"
            };
            format!(
                "{} {} patterns ({} errs) {}",
                icon,
                self.clusters.len(),
                self.total_error_logs,
                self.sparkline()
            )
        }
    }
}

/// Normalize a log message into a canonical fingerprint by replacing variable tokens
/// (UUIDs, IPv4 addresses, timestamps, hex strings, numbers) with placeholders.
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

fn severity_weight(sev: &str) -> u8 {
    match sev.to_uppercase().as_str() {
        "WARNING" | "WARN" => 1,
        "ERROR" => 2,
        "CRITICAL" => 3,
        "ALERT" => 4,
        "EMERGENCY" => 5,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_normalization() {
        let msg1 = "Connection timeout to 10.24.1.99:5432 after 3000ms (req 550e8400-e29b-41d4-a716-446655440000)";
        let msg2 = "Connection timeout to 192.168.0.12:5432 after 4500ms (req 123e4567-e89b-12d3-a456-426614174000)";

        let fp1 = fingerprint_message(msg1);
        let fp2 = fingerprint_message(msg2);
        assert_eq!(fp1, fp2);
        assert_eq!(fp1, "Connection timeout to <IP> after <N>ms (req <UUID>)");
    }

    #[test]
    fn test_spike_radar_clustering_and_ranking() {
        let mut radar = SpikeRadar::new();

        for i in 0..4 {
            radar.record_entry(&LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some(format!("DB connection pool exhausted on shard {}", i)),
                ..Default::default()
            });
        }

        radar.record_entry(&LogEntry {
            severity: Some("WARNING".to_string()),
            text_payload: Some("High memory usage 88%".to_string()),
            ..Default::default()
        });

        assert_eq!(radar.clusters.len(), 2);
        assert_eq!(radar.total_error_logs, 5);
        assert_eq!(radar.clusters[0].count, 4);
        assert!(radar.clusters[0].is_spike);
        assert_eq!(
            radar.clusters[0].fingerprint,
            "DB connection pool exhausted on shard <N>"
        );
        assert!(radar.header_summary().contains("2 patterns (5 errs)"));
    }
}
