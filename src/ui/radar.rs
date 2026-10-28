//! Error Fingerprinting & Spike Radar engine (`c` key).
//!
//! Normalizes dynamic tokens (UUIDs, IPs, timestamps, hex strings, numbers)
//! in `WARNING` / `ERROR` / `CRITICAL` log messages to cluster repetitive error storms
//! into canonical patterns and render a real-time Unicode velocity sparkline.

use serde::{Deserialize, Serialize};

use crate::gcp::LogEntry;
pub use crate::utils::fingerprint_message;

/// Comparison metrics for a single normalized error/warning fingerprint between two windows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffPatternEntry {
    /// Canonical normalized fingerprint (`<UUID>`, `<IP>`, `<N>`, etc.).
    pub fingerprint: String,
    /// Highest severity observed (`WARNING`, `ERROR`, `CRITICAL`, etc.).
    pub severity: String,
    /// Occurrence count in the baseline window.
    pub baseline_count: usize,
    /// Occurrence count in the current/candidate window.
    pub current_count: usize,
    /// Representative sample log message.
    pub sample_message: String,
}

/// Structured diff report comparing error/warning fingerprints between a baseline and current dataset (`racoon diff`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogDiffReport {
    /// Total log entries inspected in the baseline window.
    pub baseline_total_logs: usize,
    /// Total log entries inspected in the current window.
    pub current_total_logs: usize,
    /// Total warning/error/critical entries in the baseline window.
    pub baseline_error_logs: usize,
    /// Total warning/error/critical entries in the current window.
    pub current_error_logs: usize,
    /// Brand-new error patterns (`0` in baseline -> `>= 1` in current).
    pub new_patterns: Vec<DiffPatternEntry>,
    /// Existing patterns whose frequency spiked (`current > baseline` by `>= 50%` and `+2` occurrences).
    pub regressed_patterns: Vec<DiffPatternEntry>,
    /// Patterns present in baseline that disappeared in current (`>= 1` in baseline -> `0` in current).
    pub resolved_patterns: Vec<DiffPatternEntry>,
    /// Patterns present in both without a significant spike.
    pub unchanged_patterns: Vec<DiffPatternEntry>,
}

impl LogDiffReport {
    /// Return `true` if any brand-new error patterns or frequency regressions were detected.
    pub fn has_regressions(&self) -> bool {
        !self.new_patterns.is_empty() || !self.regressed_patterns.is_empty()
    }

    /// Format a human-readable Markdown / terminal report of the fingerprint diff.
    pub fn format_report(&self, baseline_label: &str, current_label: &str) -> String {
        let mut out = String::with_capacity(2048);
        let verdict = if self.has_regressions() {
            "🚨 REGRESSIONS DETECTED"
        } else {
            "✅ NO NEW REGRESSIONS"
        };

        out.push_str(&format!(
            "# 🦝 Racoon Release & Window Diff — {}\n\n",
            verdict
        ));
        out.push_str(&format!(
            "- **Baseline (`{}`)**: {} logs ({} errors/warnings)\n",
            baseline_label, self.baseline_total_logs, self.baseline_error_logs
        ));
        out.push_str(&format!(
            "- **Current  (`{}`)**: {} logs ({} errors/warnings)\n\n",
            current_label, self.current_total_logs, self.current_error_logs
        ));

        out.push_str(&format!(
            "## 🆕 New Error Patterns ({})\n",
            self.new_patterns.len()
        ));
        if self.new_patterns.is_empty() {
            out.push_str("  (none)\n");
        } else {
            for p in &self.new_patterns {
                out.push_str(&format!(
                    "  - 🚨 [0 → {}] [{}] {}\n",
                    p.current_count, p.severity, p.fingerprint
                ));
            }
        }
        out.push('\n');

        out.push_str(&format!(
            "## 📈 Regressed / Spiking Patterns ({})\n",
            self.regressed_patterns.len()
        ));
        if self.regressed_patterns.is_empty() {
            out.push_str("  (none)\n");
        } else {
            for p in &self.regressed_patterns {
                out.push_str(&format!(
                    "  - 🔥 [{} → {}] [{}] {}\n",
                    p.baseline_count, p.current_count, p.severity, p.fingerprint
                ));
            }
        }
        out.push('\n');

        out.push_str(&format!(
            "## ✅ Resolved Patterns ({})\n",
            self.resolved_patterns.len()
        ));
        if self.resolved_patterns.is_empty() {
            out.push_str("  (none)\n");
        } else {
            for p in &self.resolved_patterns {
                out.push_str(&format!(
                    "  - ✅ [{} → 0] [{}] {}\n",
                    p.baseline_count, p.severity, p.fingerprint
                ));
            }
        }

        out
    }
}

/// Compare two slices of `LogEntry` items (`baseline` vs `current`) by their normalized
/// error/warning fingerprints to detect new regressions, frequency spikes, and resolved errors.
pub fn compute_log_diff(baseline: &[LogEntry], current: &[LogEntry]) -> LogDiffReport {
    let mut base_radar = SpikeRadar::new();
    base_radar.rebuild_from_logs(baseline);

    let mut curr_radar = SpikeRadar::new();
    curr_radar.rebuild_from_logs(current);

    let mut new_patterns = Vec::new();
    let mut regressed_patterns = Vec::new();
    let mut resolved_patterns = Vec::new();
    let mut unchanged_patterns = Vec::new();

    for c_cluster in &curr_radar.clusters {
        let base_match = base_radar
            .clusters
            .iter()
            .find(|b| b.fingerprint == c_cluster.fingerprint);

        match base_match {
            None => {
                new_patterns.push(DiffPatternEntry {
                    fingerprint: c_cluster.fingerprint.clone(),
                    severity: c_cluster.severity.clone(),
                    baseline_count: 0,
                    current_count: c_cluster.count,
                    sample_message: c_cluster.sample_entry.message_preview(),
                });
            }
            Some(b_cluster) => {
                let sev = if severity_weight(&c_cluster.severity)
                    >= severity_weight(&b_cluster.severity)
                {
                    c_cluster.severity.clone()
                } else {
                    b_cluster.severity.clone()
                };
                let entry = DiffPatternEntry {
                    fingerprint: c_cluster.fingerprint.clone(),
                    severity: sev,
                    baseline_count: b_cluster.count,
                    current_count: c_cluster.count,
                    sample_message: c_cluster.sample_entry.message_preview(),
                };
                // Flag as regressed if count grew by >= 50% and at least +2 occurrences
                if c_cluster.count >= b_cluster.count + 2
                    && c_cluster.count * 2 >= b_cluster.count * 3
                {
                    regressed_patterns.push(entry);
                } else {
                    unchanged_patterns.push(entry);
                }
            }
        }
    }

    for b_cluster in &base_radar.clusters {
        if !curr_radar
            .clusters
            .iter()
            .any(|c| c.fingerprint == b_cluster.fingerprint)
        {
            resolved_patterns.push(DiffPatternEntry {
                fingerprint: b_cluster.fingerprint.clone(),
                severity: b_cluster.severity.clone(),
                baseline_count: b_cluster.count,
                current_count: 0,
                sample_message: b_cluster.sample_entry.message_preview(),
            });
        }
    }

    LogDiffReport {
        baseline_total_logs: baseline.len(),
        current_total_logs: current.len(),
        baseline_error_logs: base_radar.total_error_logs,
        current_error_logs: curr_radar.total_error_logs,
        new_patterns,
        regressed_patterns,
        resolved_patterns,
        unchanged_patterns,
    }
}

/// Alert emitted by `SpikeRadar::record_entry` when a brand-new `ERROR`/`CRITICAL` pattern
/// appears or an existing pattern crosses the spike threshold (`count == 3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpikeAlert {
    /// A brand-new `ERROR` / `CRITICAL` / `ALERT` / `EMERGENCY` fingerprint was observed for the first time.
    NewErrorPattern {
        severity: String,
        fingerprint: String,
    },
    /// An existing cluster crossed the spike threshold (`count == 3`).
    SpikeThresholdCrossed {
        severity: String,
        fingerprint: String,
        count: usize,
    },
}

impl SpikeAlert {
    /// Format a compact alert banner for the TUI header or status bar.
    pub fn summary(&self) -> String {
        match self {
            SpikeAlert::NewErrorPattern {
                severity,
                fingerprint,
            } => format!("🚨 NEW {}: {}", severity, fingerprint),
            SpikeAlert::SpikeThresholdCrossed {
                severity,
                fingerprint,
                count,
            } => format!("🚨 SPIKE (x{}) [{}]: {}", count, severity, fingerprint),
        }
    }
}

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
    ///
    /// Returns `Some(SpikeAlert)` when a brand-new `ERROR`/`CRITICAL` pattern appears or when an
    /// existing cluster crosses the spike threshold (`count == 3`).
    pub fn record_entry(&mut self, entry: &LogEntry) -> Option<SpikeAlert> {
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
            return None;
        }

        let fp = fingerprint_message(&entry.message_preview());
        let ts = entry.timestamp_str();
        let mut alert = None;

        if let Some(existing) = self.clusters.iter_mut().find(|c| c.fingerprint == fp) {
            let was_spike = existing.is_spike;
            existing.count += 1;
            existing.last_seen = ts;
            if severity_weight(&sev) > severity_weight(&existing.severity) {
                existing.severity = sev.clone();
            }
            existing.sample_entry = entry.clone();
            existing.is_spike = existing.count >= 3 || existing.severity == "CRITICAL";
            if !was_spike && existing.is_spike {
                alert = Some(SpikeAlert::SpikeThresholdCrossed {
                    severity: existing.severity.clone(),
                    fingerprint: existing.fingerprint.clone(),
                    count: existing.count,
                });
            }
        } else {
            let is_high_sev = matches!(sev.as_str(), "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY");
            if is_high_sev {
                alert = Some(SpikeAlert::NewErrorPattern {
                    severity: sev.clone(),
                    fingerprint: fp.clone(),
                });
            }
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

        alert
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

    #[test]
    fn test_compute_log_diff_detects_new_regressed_and_resolved_patterns() {
        let baseline = vec![
            LogEntry {
                severity: Some("WARNING".to_string()),
                text_payload: Some("Deprecated API v1 called by client 10".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Redis timeout after 100ms".to_string()),
                ..Default::default()
            },
            LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some("Redis timeout after 120ms".to_string()),
                ..Default::default()
            },
        ];

        let mut current = Vec::new();
        for ms in [200, 210, 220, 230, 240, 250] {
            current.push(LogEntry {
                severity: Some("ERROR".to_string()),
                text_payload: Some(format!("Redis timeout after {}ms", ms)),
                ..Default::default()
            });
        }
        for req in 1..=3 {
            current.push(LogEntry {
                severity: Some("CRITICAL".to_string()),
                text_payload: Some(format!(
                    "NullPointerException in CheckoutController req {}",
                    req
                )),
                ..Default::default()
            });
        }

        let report = compute_log_diff(&baseline, &current);
        assert!(report.has_regressions());
        assert_eq!(report.new_patterns.len(), 1);
        assert_eq!(
            report.new_patterns[0].fingerprint,
            "NullPointerException in CheckoutController req <N>"
        );
        assert_eq!(report.new_patterns[0].current_count, 3);

        assert_eq!(report.regressed_patterns.len(), 1);
        assert_eq!(
            report.regressed_patterns[0].fingerprint,
            "Redis timeout after <N>ms"
        );
        assert_eq!(report.regressed_patterns[0].baseline_count, 2);
        assert_eq!(report.regressed_patterns[0].current_count, 6);

        assert_eq!(report.resolved_patterns.len(), 1);
        assert_eq!(
            report.resolved_patterns[0].fingerprint,
            "Deprecated API v1 called by client <N>"
        );

        let text = report.format_report("v1.2.0 (before)", "v1.3.0 (after)");
        assert!(text.contains("New Error Patterns"));
        assert!(text.contains("NullPointerException"));
    }

    #[test]
    fn test_spike_radar_emits_alert_on_new_error_and_spike_threshold() {
        let mut radar = SpikeRadar::new();

        // 1. First WARNING does NOT emit NewErrorPattern alert, but once it hits 3 occurrences it emits SpikeThresholdCrossed
        let warn1 = radar.record_entry(&LogEntry {
            severity: Some("WARNING".to_string()),
            text_payload: Some("Slow query 120ms".to_string()),
            ..Default::default()
        });
        assert!(warn1.is_none());

        let warn2 = radar.record_entry(&LogEntry {
            severity: Some("WARNING".to_string()),
            text_payload: Some("Slow query 140ms".to_string()),
            ..Default::default()
        });
        assert!(warn2.is_none());

        let warn3 = radar.record_entry(&LogEntry {
            severity: Some("WARNING".to_string()),
            text_payload: Some("Slow query 180ms".to_string()),
            ..Default::default()
        });
        assert_eq!(
            warn3,
            Some(SpikeAlert::SpikeThresholdCrossed {
                severity: "WARNING".to_string(),
                fingerprint: "Slow query <N>ms".to_string(),
                count: 3,
            })
        );

        // 2. First ERROR immediately emits NewErrorPattern alert
        let err1 = radar.record_entry(&LogEntry {
            severity: Some("ERROR".to_string()),
            text_payload: Some("Connection refused to 10.0.0.5:5432".to_string()),
            ..Default::default()
        });
        assert_eq!(
            err1,
            Some(SpikeAlert::NewErrorPattern {
                severity: "ERROR".to_string(),
                fingerprint: "Connection refused to <IP>".to_string(),
            })
        );
    }
}
