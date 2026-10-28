//! GCP Cloud Logging API client, domain model, and filter engine.
//!
//! Organized into focused submodules:
//! - [`client`]: REST API v2 querying, multi-page pagination, and overlap-window streaming.
//! - [`entry`]: [`LogEntry`], [`MonitoredResource`], [`HttpRequest`], trace correlation, and Console URLs.
//! - [`filter`]: [`LogFilter`] builder and quote/boolean-aware local filter evaluator.

pub mod client;
pub mod entry;
pub mod filter;

pub use client::{
    LogsClient, LogsError, StreamHandle, extract_first_gcp_project_id, parse_resource_names,
};
pub use entry::{HttpRequest, LogEntry, MonitoredResource};
pub use filter::{
    LogFilter, is_local_only_filter, matches_local_filter, severity_rank, tokenize_filter_expr,
};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, Utc};
    use tokio::sync::mpsc;

    use super::client::StreamDeduplicator;
    use super::*;

    #[test]
    fn test_log_filter_builder() {
        let filter = LogFilter::new()
            .severity_min("ERROR")
            .resource_type("cloud_run_revision");

        let built = filter.build();
        assert!(built.contains("severity>=ERROR"));
        assert!(built.contains("resource.type=\"cloud_run_revision\""));
    }

    #[test]
    fn test_log_filter_text_contains() {
        let filter = LogFilter::new().text_contains("connection timeout");
        assert_eq!(filter.build(), "\"connection timeout\"");
    }

    #[test]
    fn test_log_entry_message() {
        let entry = LogEntry {
            text_payload: Some("Hello world".to_string()),
            ..Default::default()
        };
        assert_eq!(entry.message(), "Hello world");

        let entry = LogEntry {
            json_payload: Some(serde_json::json!({"message": "From JSON"})),
            ..Default::default()
        };
        assert_eq!(entry.message(), "From JSON");

        let entry = LogEntry::default();
        assert_eq!(entry.message(), "[no message]");
    }

    #[test]
    fn test_smart_log_entry_message_fallbacks() {
        // 1. Go zap/zerolog style `msg`
        let zap_entry = LogEntry {
            json_payload: Some(serde_json::json!({"level": "error", "msg": "grpc dial timeout"})),
            ..Default::default()
        };
        assert_eq!(zap_entry.message(), "grpc dial timeout");

        // 2. Python structlog style `event`
        let structlog_entry = LogEntry {
            json_payload: Some(serde_json::json!({"event": "user_login_failed", "user_id": 42})),
            ..Default::default()
        };
        assert_eq!(structlog_entry.message(), "user_login_failed");

        // 3. Nested `error.message` object (GCP Error Reporting / Envoy / Node.js)
        let nested_err_entry = LogEntry {
            json_payload: Some(serde_json::json!({
                "error": { "code": 504, "message": "upstream request timeout" }
            })),
            ..Default::default()
        };
        assert_eq!(nested_err_entry.message(), "upstream request timeout");

        // 4. Pure HTTP Load Balancer / Cloud Run request log without textPayload or jsonPayload
        let http_only_entry = LogEntry {
            http_request: Some(HttpRequest {
                request_method: Some("POST".to_string()),
                request_url: Some("https://api.acme.com/v2/checkout".to_string()),
                status: Some(503),
                latency: Some("5.004s".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            http_only_entry.message(),
            "POST 503 https://api.acme.com/v2/checkout (5.004s)"
        );
    }

    #[test]
    fn test_negated_fingerprint_local_filter() {
        let noisy_entry = LogEntry {
            severity: Some("WARNING".to_string()),
            text_payload: Some("High memory usage 85%".to_string()),
            ..Default::default()
        };
        let real_err_entry = LogEntry {
            severity: Some("ERROR".to_string()),
            text_payload: Some("Redis connection failed on port 6379".to_string()),
            ..Default::default()
        };

        let filter_expr = "severity>=WARNING -fingerprint:\"High memory usage <N>%\"";
        assert!(is_local_only_filter(filter_expr));
        assert!(!matches_local_filter(&noisy_entry, filter_expr));
        assert!(matches_local_filter(&real_err_entry, filter_expr));
    }

    #[test]
    fn test_log_entry_to_line() {
        let entry = LogEntry {
            timestamp: Some(Utc::now()),
            severity: Some("ERROR".to_string()),
            text_payload: Some("Something went wrong".to_string()),
            ..Default::default()
        };

        let line = entry.to_line();
        assert!(line.contains("[ERROR]"));
        assert!(line.contains("Something went wrong"));
    }

    #[test]
    fn test_log_entry_default() {
        let entry = LogEntry::default();
        assert_eq!(entry.severity_or_default(), "DEFAULT");
        assert_eq!(entry.timestamp_str(), "--:--:--");
    }

    #[test]
    fn test_log_entry_correlation_id() {
        let entry = LogEntry {
            trace: Some("projects/demo-proj/traces/105445aa7843bc8bf206b12000100000".to_string()),
            ..Default::default()
        };
        let (filter, short_id) = entry.correlation_id().unwrap();
        assert_eq!(
            filter,
            "trace=\"projects/demo-proj/traces/105445aa7843bc8bf206b12000100000\""
        );
        assert_eq!(short_id, "105445aa");
        assert_eq!(entry.short_trace_badge().as_deref(), Some("🔗105445aa"));

        let json_entry = LogEntry {
            json_payload: Some(serde_json::json!({
                "message": "Order failed",
                "request_id": "req-9988776655"
            })),
            ..Default::default()
        };
        let (j_filter, j_short) = json_entry.correlation_id().unwrap();
        assert_eq!(j_filter, "jsonPayload.request_id=\"req-9988776655\"");
        assert_eq!(j_short, "req-9988");
    }

    #[test]
    fn test_message_preview_utf8_multibyte_no_panic() {
        let cjk_60 = "数据库连接超时错误".repeat(7); // 63 chars, 189 bytes
        let entry = LogEntry {
            text_payload: Some(cjk_60.clone()),
            ..Default::default()
        };
        assert_eq!(entry.message_preview(), cjk_60);

        let long_cjk = format!("🚨生产故障告警：{}", "微服务调用链超时重试失败".repeat(15));
        let long_entry = LogEntry {
            text_payload: Some(long_cjk),
            ..Default::default()
        };
        let preview = long_entry.message_preview();
        assert!(preview.ends_with("..."));
        assert_eq!(preview.chars().count(), 153);
    }

    #[test]
    fn test_stream_deduplicator_same_timestamp_and_eviction() {
        let mut dedup = StreamDeduplicator::new(16);
        let ts = Utc::now();

        let e1 = LogEntry {
            insert_id: Some("id-1".to_string()),
            timestamp: Some(ts),
            text_payload: Some("First log at ts".to_string()),
            ..Default::default()
        };
        let e2 = LogEntry {
            insert_id: Some("id-2".to_string()),
            timestamp: Some(ts),
            text_payload: Some("Second log at exact same ts".to_string()),
            ..Default::default()
        };

        assert!(dedup.insert(&e1));
        assert!(!dedup.insert(&e1));
        assert!(dedup.insert(&e2));
        assert!(dedup.contains(&e1));
        assert!(dedup.contains(&e2));
    }

    #[tokio::test]
    async fn test_stream_handle_drop_signals_stop_channel() {
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let handle = StreamHandle::new(stop_tx);
        drop(handle);
        // Dropping StreamHandle immediately sends () and closes the channel
        assert!(stop_rx.recv().await.is_some());
    }

    #[test]
    fn test_log_entry_matches_query_lower_without_serialization() {
        let entry = LogEntry {
            severity: Some("ERROR".to_string()),
            text_payload: Some("Connection reset by peer".to_string()),
            json_payload: Some(serde_json::json!({
                "service": "checkout-api",
                "nested": { "error_code": "E_CONN_RESET", "retryable": true }
            })),
            ..Default::default()
        };

        assert!(entry.matches_query_lower("connection reset"));
        assert!(entry.matches_query_lower("checkout-api"));
        assert!(entry.matches_query_lower("e_conn_reset"));
        assert!(!entry.matches_query_lower("nonexistent_token"));
    }

    #[test]
    fn test_log_entry_surrounding_context() {
        let ts = DateTime::parse_from_rfc3339("2026-10-27T13:00:10Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut rlabels = HashMap::new();
        rlabels.insert("pod_name".to_string(), "order-svc-7d9f8b-x2k9p".to_string());

        let entry = LogEntry {
            timestamp: Some(ts),
            severity: Some("ERROR".to_string()),
            resource: Some(MonitoredResource {
                r#type: "k8s_container".to_string(),
                labels: Some(rlabels),
            }),
            text_payload: Some("Connection reset by peer".to_string()),
            ..Default::default()
        };

        let (filter, badge) = entry.surrounding_context(5).unwrap();
        assert!(filter.contains("resource.type=\"k8s_container\""));
        assert!(filter.contains("resource.labels.pod_name=\"order-svc-7d9f8b-x2k9p\""));
        assert!(filter.contains("timestamp>=\"2026-10-27T13:00:05+00:00\""));
        assert!(filter.contains("timestamp<=\"2026-10-27T13:00:15+00:00\""));
        assert!(!filter.contains("severity"));
        assert_eq!(badge, "order-svc-7d9f8b-x2k ±5s @ 13:00:10");
    }

    #[test]
    fn test_log_entry_gcp_console_url_and_project_resolution() {
        let ts = DateTime::parse_from_rfc3339("2026-10-27T13:00:10Z")
            .unwrap()
            .with_timezone(&Utc);
        let entry = LogEntry {
            insert_id: Some("ins-98765".to_string()),
            timestamp: Some(ts),
            log_name: Some(
                "projects/prod-payments-42/logs/run.googleapis.com%2Fstderr".to_string(),
            ),
            resource: Some(MonitoredResource {
                r#type: "cloud_run_revision".to_string(),
                labels: None,
            }),
            ..Default::default()
        };

        let url = entry.gcp_console_url("file:incident.json").unwrap();
        assert!(url.starts_with("https://console.cloud.google.com/logs/query;query="));
        assert!(url.contains("insertId%3D%22ins-98765%22"));
        assert!(url.contains("cursorTimestamp=2026-10-27T13%3A00%3A10%2B00%3A00"));
        assert!(url.ends_with("?project=prod-payments-42"));

        let empty = LogEntry::default();
        assert!(empty.gcp_console_url("file:incident.json").is_none());
        assert_eq!(
            empty.gcp_console_url("my-live-project").as_deref(),
            Some("https://console.cloud.google.com/logs/query?project=my-live-project")
        );
    }

    #[test]
    fn test_parse_resource_names_multi_project_and_log_views() {
        assert_eq!(parse_resource_names("my-proj"), vec!["projects/my-proj"]);
        assert_eq!(
            parse_resource_names("prod-apigw, prod-order , prod-payment"),
            vec![
                "projects/prod-apigw",
                "projects/prod-order",
                "projects/prod-payment"
            ]
        );
        assert_eq!(
            parse_resource_names(
                "projects/host-proj/locations/global/buckets/central/views/all-logs,organizations/123456"
            ),
            vec![
                "projects/host-proj/locations/global/buckets/central/views/all-logs",
                "organizations/123456"
            ]
        );
    }
}
