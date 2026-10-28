//! Pre-AI PII & Secret Redaction Engine (`🛡️ PII Guard`).
//!
//! Scrubs sensitive credentials, tokens, connection passwords, and personally
//! identifiable information (PII) locally before any log payload or prompt is
//! transmitted to an external LLM provider.
//!
//! # Redacted Categories
//!
//! - **JWT & Bearer Tokens**: `Authorization: Bearer ...`, standalone `eyJ...` JWTs, GCP `ya29....` OAuth tokens
//! - **API Keys & Cloud Secrets**: Google `AIza...`, OpenAI `sk-...`, Anthropic `sk-ant-...`, GitHub `ghp_...`, AWS `AKIA...`, PEM private keys
//! - **Connection & Key-Value Secrets**: URI passwords (`postgres://user:pass@host`), JSON/log fields (`password`, `secret`, `api_key`, `access_token`, `cookie`, etc.)
//! - **Customer PII**: Email addresses (while preserving GCP `*.gserviceaccount.com` IAM identifiers), Luhn-validated credit card numbers, SSNs

use regex::{Captures, Regex};
use std::sync::OnceLock;

/// Summary statistics from a redaction pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedactionStats {
    /// Total number of sensitive tokens replaced across all categories.
    pub total_redactions: usize,
    /// Number of auth tokens / JWTs redacted.
    pub tokens_redacted: usize,
    /// Number of API keys or private keys redacted.
    pub api_keys_redacted: usize,
    /// Number of passwords / key-value secrets redacted.
    pub secrets_redacted: usize,
    /// Number of email addresses redacted.
    pub emails_redacted: usize,
    /// Number of payment cards or SSNs redacted.
    pub pii_numbers_redacted: usize,
}

impl RedactionStats {
    /// Merge another `RedactionStats` into `self`.
    pub fn merge(&mut self, other: &RedactionStats) {
        self.total_redactions += other.total_redactions;
        self.tokens_redacted += other.tokens_redacted;
        self.api_keys_redacted += other.api_keys_redacted;
        self.secrets_redacted += other.secrets_redacted;
        self.emails_redacted += other.emails_redacted;
        self.pii_numbers_redacted += other.pii_numbers_redacted;
    }

    /// Return a list of human-readable category labels that were triggered.
    pub fn active_categories(&self) -> Vec<&'static str> {
        let mut cats = Vec::new();
        if self.tokens_redacted > 0 {
            cats.push("JWT/Bearer");
        }
        if self.api_keys_redacted > 0 {
            cats.push("API/Private Key");
        }
        if self.secrets_redacted > 0 {
            cats.push("Password/Secret");
        }
        if self.emails_redacted > 0 {
            cats.push("Email");
        }
        if self.pii_numbers_redacted > 0 {
            cats.push("Card/SSN");
        }
        cats
    }

    /// Format a compact security banner when sensitive items were scrubbed.
    pub fn banner(&self) -> Option<String> {
        if self.total_redactions == 0 {
            return None;
        }
        let cats = self.active_categories().join(", ");
        Some(format!(
            "🛡️ PII & Secret Guard: auto-redacted {} sensitive item(s) [{}] before sending to AI",
            self.total_redactions, cats
        ))
    }
}

/// Redact sensitive credentials, secrets, and PII from a text or serialized JSON string.
///
/// Returns `(redacted_string, stats)`.
pub fn redact_sensitive_text(input: &str) -> (String, RedactionStats) {
    static RE_PEM_KEY: OnceLock<Regex> = OnceLock::new();
    static RE_BEARER: OnceLock<Regex> = OnceLock::new();
    static RE_JWT: OnceLock<Regex> = OnceLock::new();
    static RE_GCP_OAUTH: OnceLock<Regex> = OnceLock::new();
    static RE_API_KEY: OnceLock<Regex> = OnceLock::new();
    static RE_URI_PASS: OnceLock<Regex> = OnceLock::new();
    static RE_JSON_SECRET: OnceLock<Regex> = OnceLock::new();
    static RE_KV_SECRET: OnceLock<Regex> = OnceLock::new();
    static RE_EMAIL: OnceLock<Regex> = OnceLock::new();
    static RE_SSN: OnceLock<Regex> = OnceLock::new();
    static RE_CARD_CANDIDATE: OnceLock<Regex> = OnceLock::new();

    let re_pem = RE_PEM_KEY.get_or_init(|| {
        Regex::new(
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
        )
        .unwrap()
    });
    let re_bearer = RE_BEARER.get_or_init(|| {
        Regex::new(r"(?i)(Authorization\s*[:=]\s*(?:\\?[\x22'])?Bearer\s+)[A-Za-z0-9._~+/-]+=*")
            .unwrap()
    });
    let re_jwt = RE_JWT.get_or_init(|| {
        Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b").unwrap()
    });
    let re_gcp_oauth =
        RE_GCP_OAUTH.get_or_init(|| Regex::new(r"\bya29\.[0-9A-Za-z._-]{16,}\b").unwrap());
    let re_api_key = RE_API_KEY.get_or_init(|| {
        Regex::new(
            r"\b(?:AIza[0-9A-Za-z_-]{30,45}|sk-ant-[0-9A-Za-z_-]{20,}|sk-(?:proj-)?[0-9A-Za-z_-]{20,}|gh[pousr]_[0-9A-Za-z]{20,}|AKIA[0-9A-Z]{16})\b",
        )
        .unwrap()
    });
    let re_uri_pass = RE_URI_PASS.get_or_init(|| {
        Regex::new(
            r"(?i)((?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis(?:s)?|amqp(?:s)?)://[^:\s/\x22']+:)(?:[^\s/\x22']+)(@)",
        )
        .unwrap()
    });
    let re_json_secret = RE_JSON_SECRET.get_or_init(|| {
        Regex::new(
            r#"(?i)("(?:password|passwd|pwd|secret|client_secret|api_key|apikey|access_token|refresh_token|id_token|private_key|x-api-key|cookie|set-cookie)"\s*:\s*)"([^"\\]*(?:\\.[^"\\]*)*)""#,
        )
        .unwrap()
    });
    let re_kv_secret = RE_KV_SECRET.get_or_init(|| {
        Regex::new(
            r"(?i)\b(password|passwd|pwd|secret|client_secret|api_key|apikey|access_token|refresh_token|x-api-key)\s*([=:])\s*([^\s,;\x22'}\]]+)",
        )
        .unwrap()
    });
    let re_email = RE_EMAIL.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})\b").unwrap()
    });
    let re_ssn = RE_SSN.get_or_init(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap());
    let re_card =
        RE_CARD_CANDIDATE.get_or_init(|| Regex::new(r"\b(?:\d{4}[ -]?){3}\d{1,4}\b").unwrap());

    let mut stats = RedactionStats::default();
    let mut text = input.to_string();

    // 1. PEM Private Keys
    text = re_pem
        .replace_all(&text, |_: &Captures| {
            stats.api_keys_redacted += 1;
            stats.total_redactions += 1;
            "<REDACTED_PRIVATE_KEY>"
        })
        .into_owned();

    // 2. Authorization: Bearer <token>
    text = re_bearer
        .replace_all(&text, |caps: &Captures| {
            stats.tokens_redacted += 1;
            stats.total_redactions += 1;
            format!("{}<REDACTED_BEARER_TOKEN>", &caps[1])
        })
        .into_owned();

    // 3. Standalone JWTs
    text = re_jwt
        .replace_all(&text, |_: &Captures| {
            stats.tokens_redacted += 1;
            stats.total_redactions += 1;
            "<REDACTED_JWT>"
        })
        .into_owned();

    // 4. GCP OAuth2 access tokens (ya29.*)
    text = re_gcp_oauth
        .replace_all(&text, |_: &Captures| {
            stats.tokens_redacted += 1;
            stats.total_redactions += 1;
            "<REDACTED_GCP_TOKEN>"
        })
        .into_owned();

    // 5. Well-known API key prefixes (GCP AIza, OpenAI sk-, Anthropic sk-ant-, GitHub ghp_, AWS AKIA)
    text = re_api_key
        .replace_all(&text, |_: &Captures| {
            stats.api_keys_redacted += 1;
            stats.total_redactions += 1;
            "<REDACTED_API_KEY>"
        })
        .into_owned();

    // 6. Database / Broker URI credentials (postgres://user:pass@host)
    text = re_uri_pass
        .replace_all(&text, |caps: &Captures| {
            stats.secrets_redacted += 1;
            stats.total_redactions += 1;
            format!("{}<REDACTED_SECRET>{}", &caps[1], &caps[2])
        })
        .into_owned();

    // 7. JSON structured secret fields ("password": "...")
    text = re_json_secret
        .replace_all(&text, |caps: &Captures| {
            let val = &caps[2];
            if val.starts_with("<REDACTED") || val.is_empty() {
                return caps[0].to_string();
            }
            stats.secrets_redacted += 1;
            stats.total_redactions += 1;
            format!("{}\"<REDACTED_SECRET>\"", &caps[1])
        })
        .into_owned();

    // 8. Plaintext key=value or key: value secrets
    text = re_kv_secret
        .replace_all(&text, |caps: &Captures| {
            let val = &caps[3];
            if val.starts_with("<REDACTED") {
                return caps[0].to_string();
            }
            stats.secrets_redacted += 1;
            stats.total_redactions += 1;
            format!("{}{}<REDACTED_SECRET>", &caps[1], &caps[2])
        })
        .into_owned();

    // 9. Email addresses (preserve GCP service account emails `*.gserviceaccount.com`)
    text = re_email
        .replace_all(&text, |caps: &Captures| {
            let domain = caps[1].to_lowercase();
            if domain.ends_with("gserviceaccount.com") || domain.ends_with("googleapis.com") {
                caps[0].to_string()
            } else {
                stats.emails_redacted += 1;
                stats.total_redactions += 1;
                "<REDACTED_EMAIL>".to_string()
            }
        })
        .into_owned();

    // 10. US Social Security Numbers
    text = re_ssn
        .replace_all(&text, |_: &Captures| {
            stats.pii_numbers_redacted += 1;
            stats.total_redactions += 1;
            "<REDACTED_SSN>"
        })
        .into_owned();

    // 11. Credit card numbers (13-16 digits passing Luhn checksum)
    text = re_card
        .replace_all(&text, |caps: &Captures| {
            let matched = &caps[0];
            if is_valid_luhn_card(matched) {
                stats.pii_numbers_redacted += 1;
                stats.total_redactions += 1;
                "<REDACTED_CARD>".to_string()
            } else {
                matched.to_string()
            }
        })
        .into_owned();

    (text, stats)
}

/// Validate whether a candidate numeric string (with optional spaces or hyphens)
/// is a 13–19 digit payment card number passing the Luhn checksum algorithm.
fn is_valid_luhn_card(candidate: &str) -> bool {
    let digits: Vec<u32> = candidate.chars().filter_map(|c| c.to_digit(10)).collect();

    if !(13..=19).contains(&digits.len()) {
        return false;
    }

    // Reject trivial all-zero or single-repeated-digit sequences
    if digits.iter().all(|&d| d == digits[0]) {
        return false;
    }

    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut n = d;
        if double {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
        double = !double;
    }

    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_bearer_and_jwt_and_gcp_token() {
        let raw = "Header Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c and token ya29.a0AfH6SMBx998877665544332211";
        let (scrubbed, stats) = redact_sensitive_text(raw);

        assert!(scrubbed.contains("Authorization: Bearer <REDACTED_BEARER_TOKEN>"));
        assert!(scrubbed.contains("<REDACTED_GCP_TOKEN>"));
        assert!(!scrubbed.contains("eyJhbGci"));
        assert!(!scrubbed.contains("ya29."));
        assert_eq!(stats.tokens_redacted, 2);
    }

    #[test]
    fn test_redact_api_keys_and_passwords_and_uris() {
        let raw = r#"{"api_key": "AIzaSyD-99887766554433221100aabbccddeeff", "password": "MySuperSecretPassword123!", "db": "postgres://admin:p@ssw0rd99@10.0.0.5:5432/orders"}"#;
        let (scrubbed, stats) = redact_sensitive_text(raw);

        assert!(scrubbed.contains("<REDACTED_API_KEY>"));
        assert!(scrubbed.contains(r#""password": "<REDACTED_SECRET>""#));
        assert!(scrubbed.contains("postgres://admin:<REDACTED_SECRET>@10.0.0.5:5432/orders"));
        assert!(!scrubbed.contains("MySuperSecretPassword123!"));
        assert!(!scrubbed.contains("p@ssw0rd99"));
        assert!(stats.api_keys_redacted >= 1);
        assert!(stats.secrets_redacted >= 2);
    }

    #[test]
    fn test_redact_customer_email_but_preserve_gcp_service_account() {
        let raw = "User alice.smith@example.com failed auth via serviceAccount checkout-sa@prod-project.iam.gserviceaccount.com";
        let (scrubbed, stats) = redact_sensitive_text(raw);

        assert!(scrubbed.contains("User <REDACTED_EMAIL> failed auth"));
        assert!(scrubbed.contains("checkout-sa@prod-project.iam.gserviceaccount.com"));
        assert_eq!(stats.emails_redacted, 1);
    }

    #[test]
    fn test_redact_luhn_credit_card_and_ssn() {
        let raw = "Payment failed for card 4532-0151-1283-0366 and SSN 123-45-6789 (req 1000200030004001)";
        let (scrubbed, stats) = redact_sensitive_text(raw);

        assert!(scrubbed.contains("<REDACTED_CARD>"));
        assert!(scrubbed.contains("<REDACTED_SSN>"));
        // Non-Luhn 16-digit request ID is preserved!
        assert!(scrubbed.contains("1000200030004001"));
        assert_eq!(stats.pii_numbers_redacted, 2);
        assert!(
            stats
                .banner()
                .unwrap()
                .contains("auto-redacted 2 sensitive item(s)")
        );
    }
}
