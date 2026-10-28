//! GCP Authentication module.
//!
//! Handles authentication with Google Cloud Platform using:
//! 1. Application Default Credentials (ADC) - from `gcloud auth application-default login`
//! 2. Service Account JSON key - via `GOOGLE_APPLICATION_CREDENTIALS` env var
//! 3. GCE metadata server (when running on Google Cloud)
//! 4. gcloud CLI tool (if available on PATH)
//!
//! # Example
//!
//! ```rust,no_run
//! use gcp_racoon::gcp::GcpAuth;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let auth = GcpAuth::new().await?;
//!
//!     // Verify credentials are valid
//!     auth.verify().await?;
//!
//!     // Get a bearer token for API calls
//!     let token = auth.token().await?;
//!     println!("Token: {}", &token[..20]); // Don't print full token!
//!
//!     Ok(())
//! }
//! ```

use chrono::{DateTime, Duration, Utc};
use gcp_auth::TokenProvider;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::debug;

/// OAuth2 scopes required for Cloud Logging read access.
const LOGGING_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/logging.read",
    "https://www.googleapis.com/auth/cloud-platform.read-only",
];

/// OAuth2 scope required for IAM Service Account Impersonation and Vertex AI calls.
const CLOUD_PLATFORM_SCOPES: &[&str] = &["https://www.googleapis.com/auth/cloud-platform"];

/// Cached short-lived access token obtained via IAM Service Account Impersonation.
#[derive(Debug, Clone)]
struct CachedImpersonatedToken {
    access_token: String,
    expires_at: DateTime<Utc>,
}

impl CachedImpersonatedToken {
    /// Returns `true` if the token is still valid with a 60-second safety margin.
    fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        now + Duration::seconds(60) < self.expires_at
    }
}

#[derive(Debug, Serialize)]
struct GenerateAccessTokenRequest<'a> {
    scope: &'a [&'a str],
    lifetime: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateAccessTokenResponse {
    access_token: String,
    expire_time: String,
}

/// Build the IAM Credentials REST API endpoint URL for impersonating a target service account.
pub fn iam_impersonation_url(service_account_email: &str) -> String {
    format!(
        "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{}:generateAccessToken",
        service_account_email.trim()
    )
}

/// Errors that can occur during GCP auth.
#[derive(Error, Debug)]
pub enum AuthError {
    /// No valid credentials found on the system.
    ///
    /// This typically means that user needs to run:
    /// `gcloud auth application-default login`
    #[error(
        "No valid credentials found on the system, Run `gcloud auth application-default login` \
    or set GOOGLE_APPLICATION_CREDENTIALS"
    )]
    NoCredentials,

    /// Credentials exist but token refresh failed.
    ///
    /// This could be due to expired refresh token, revoked credentials,
    /// or network issues.
    #[error("Failed to refresh access token: {0}")]
    TokenRefresh(String),

    /// The specified GCP project is invalid
    #[error("Invalid or inaccessible project: {0}")]
    InvalidProject(String),

    /// Network / API error
    #[error("Network / API error: {0}")]
    ApiError(String),
}

/// GCP authentication manager.
///
/// Wraps `gcp_auth::AuthenticationManager` to provide a simplified interface
/// for obtaining OAuth2 access tokens for GCP API calls.
///
/// # Token Caching
///
/// The underlying `AuthenticationManager` automatically caches and refreshes
/// tokens, so it's safe (and recommended) to call `token()` before each API
/// request without worrying about performance.
#[derive(Clone)]
pub struct GcpAuth {
    /// The underlying authentication manager
    provider: Arc<dyn TokenProvider>,
    /// HTTP client for IAM Service Account Impersonation requests
    http: Client,
    /// Optional target service account email to impersonate (`--impersonate-service-account`)
    impersonate_service_account: Option<String>,
    /// Cached impersonated OAuth2 access token and expiry
    impersonated_cache: Arc<Mutex<Option<CachedImpersonatedToken>>>,
}

impl GcpAuth {
    /// Create a new authentication manager using default credentials (and `GOOGLE_IMPERSONATE_SERVICE_ACCOUNT` if set).
    ///
    /// This will attempt to find credentials in the following order:
    /// 1. `GOOGLE_APPLICATION_CREDENTIALS` environment variable (Service Account)
    /// 2. Application Default Credentials (from gcloud CLI)
    /// 3. GCE metadata server (when running on Google Cloud)
    /// 4. gcloud CLI tool (if available on PATH)
    ///
    /// # Errors
    ///
    /// Returns `AuthError::NoCredentials` if no valid credentials can be found.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use gcp_racoon::gcp::GcpAuth;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     match GcpAuth::new().await {
    ///         Ok(auth) => println!("Authentication successful!"),
    ///         Err(e) => eprintln!("Auth failed: {}", e),
    ///     }
    /// }
    /// ```
    pub async fn new() -> Result<Self, AuthError> {
        let env_impersonate = std::env::var("GOOGLE_IMPERSONATE_SERVICE_ACCOUNT")
            .ok()
            .or_else(|| std::env::var("CLOUDSDK_AUTH_IMPERSONATE_SERVICE_ACCOUNT").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self::with_impersonation(env_impersonate).await
    }

    /// Create an authentication manager with an explicit optional target service account to impersonate.
    pub async fn with_impersonation(
        impersonate_service_account: Option<String>,
    ) -> Result<Self, AuthError> {
        let provider = gcp_auth::provider().await.map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Could not find default credentials")
                || error_msg.contains("No credentials")
                || error_msg.contains("unable to initialize")
            {
                AuthError::NoCredentials
            } else {
                AuthError::ApiError(error_msg)
            }
        })?;

        let normalized_sa = impersonate_service_account
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        Ok(Self {
            provider,
            http: Client::new(),
            impersonate_service_account: normalized_sa,
            impersonated_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Return the target service account email if Service Account Impersonation is active.
    pub fn impersonated_service_account(&self) -> Option<&str> {
        self.impersonate_service_account.as_deref()
    }

    /// Get an OAuth2 access token for GCP Cloud Logging API calls.
    pub async fn token(&self) -> Result<String, AuthError> {
        self.token_with_scopes(LOGGING_SCOPES).await
    }

    /// Get an OAuth2 access token with `cloud-platform` scope (used by Vertex AI and IAM APIs).
    pub async fn cloud_platform_token(&self) -> Result<String, AuthError> {
        self.token_with_scopes(CLOUD_PLATFORM_SCOPES).await
    }

    /// Get an OAuth2 access token for the specified scopes, transparently exchanging via
    /// IAM Credentials `generateAccessToken` when `impersonate_service_account` is configured.
    pub async fn token_with_scopes(&self, scopes: &[&str]) -> Result<String, AuthError> {
        if let Some(ref target_sa) = self.impersonate_service_account {
            return self.impersonated_token(target_sa).await;
        }

        let token = self
            .provider
            .token(scopes)
            .await
            .map_err(|e| AuthError::TokenRefresh(e.to_string()))?;

        Ok(token.as_str().to_string())
    }

    /// Obtain or return a cached short-lived access token for `target_sa` via IAM Service Account Impersonation.
    async fn impersonated_token(&self, target_sa: &str) -> Result<String, AuthError> {
        if let Ok(guard) = self.impersonated_cache.lock()
            && let Some(ref cached) = *guard
            && cached.is_valid_at(Utc::now())
        {
            return Ok(cached.access_token.clone());
        }

        let source_token = self
            .provider
            .token(CLOUD_PLATFORM_SCOPES)
            .await
            .map_err(|e| {
                AuthError::TokenRefresh(format!(
                    "Failed to obtain source ADC token for impersonating '{}': {}",
                    target_sa, e
                ))
            })?;

        let url = iam_impersonation_url(target_sa);
        let requested_scopes = &[
            "https://www.googleapis.com/auth/logging.read",
            "https://www.googleapis.com/auth/cloud-platform.read-only",
            "https://www.googleapis.com/auth/cloud-platform",
        ];
        let req_body = GenerateAccessTokenRequest {
            scope: requested_scopes,
            lifetime: "3600s",
        };

        debug!(
            target_service_account = %target_sa,
            "Requesting impersonated access token via IAM Credentials API"
        );

        let resp = self
            .http
            .post(&url)
            .bearer_auth(source_token.as_str())
            .json(&req_body)
            .send()
            .await
            .map_err(|e| AuthError::ApiError(format!("IAM impersonation HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenRefresh(format!(
                "IAM Service Account Impersonation failed for '{}' (HTTP {}): {}. Ensure your account has 'roles/iam.serviceAccountTokenCreator' on the target service account.",
                target_sa, status, body
            )));
        }

        let parsed: GenerateAccessTokenResponse = resp.json().await.map_err(|e| {
            AuthError::TokenRefresh(format!(
                "Failed to parse IAM generateAccessToken response: {}",
                e
            ))
        })?;

        let expires_at = DateTime::parse_from_rfc3339(&parsed.expire_time)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now() + Duration::minutes(55));

        if let Ok(mut guard) = self.impersonated_cache.lock() {
            *guard = Some(CachedImpersonatedToken {
                access_token: parsed.access_token.clone(),
                expires_at,
            });
        }

        Ok(parsed.access_token)
    }

    /// Verify that the current credentials are valid.
    pub async fn verify(&self) -> Result<(), AuthError> {
        self.token().await?;
        Ok(())
    }

    /// Get detailed information about the current credentials.
    ///
    /// Returns a human-readable description of the credential source.
    /// Useful for debugging and user feedback.
    pub fn credential_info(&self) -> CredentialInfo {
        if let Some(ref sa) = self.impersonate_service_account {
            return CredentialInfo {
                source: CredentialSource::ImpersonatedServiceAccount,
                details: Some(sa.clone()),
            };
        }

        // Check for Service Account key file.
        if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            return CredentialInfo {
                source: CredentialSource::ServiceAccountFile,
                details: Some(path),
            };
        }

        // Check for ADC file in default location
        let adc_path =
            dirs::home_dir().map(|h| h.join(".config/gcloud/application_default_credentials.json"));

        if let Some(path) = adc_path
            && path.exists()
        {
            return CredentialInfo {
                source: CredentialSource::ApplicationDefaultCredentials,
                details: Some(path.to_string_lossy().to_string()),
            };
        }

        // Fallback - might be running on GCE or other envs
        CredentialInfo {
            source: CredentialSource::Unknown,
            details: None,
        }
    }
}

/// Information about the credential source.
#[derive(Debug, Clone)]
pub struct CredentialInfo {
    /// The type of credential being used.
    pub source: CredentialSource,
    /// Additional details (e.g., file path or impersonated service account email).
    pub details: Option<String>,
}

impl std::fmt::Display for CredentialInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            CredentialSource::ImpersonatedServiceAccount => {
                write!(f, "IAM Service Account Impersonation")?;
                if let Some(sa) = &self.details {
                    write!(f, " ({})", sa)?;
                }
            }
            CredentialSource::ServiceAccountFile => {
                write!(f, "Service Account Key")?;
                if let Some(path) = &self.details {
                    write!(f, " ({})", path)?;
                }
            }
            CredentialSource::ApplicationDefaultCredentials => {
                write!(f, "Application Default Credentials")?;
                if let Some(path) = &self.details {
                    write!(f, " ({})", path)?;
                }
            }
            CredentialSource::MetadataServer => {
                write!(f, "GCE Metadata Server")?;
            }
            CredentialSource::Unknown => {
                write!(f, "Unknown source")?;
            }
        }

        Ok(())
    }
}

/// The source of GCP credentials
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// IAM Service Account Impersonation (`--impersonate-service-account`)
    ImpersonatedServiceAccount,
    /// Service Account JSON key file (via GOOGLE_APPLICATION_CREDENTIALS)
    ServiceAccountFile,
    /// Application Default Credentials (from gcloud CLI)
    ApplicationDefaultCredentials,
    /// GCE Metadata Server (when running on Google Cloud)
    MetadataServer,
    /// Other unknown source
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_info_display() {
        let info = CredentialInfo {
            source: CredentialSource::ApplicationDefaultCredentials,
            details: Some("/home/user/.config/gcloud/credentials.json".to_string()),
        };

        let display = info.to_string();
        assert!(display.contains("Application Default Credentials"));
        assert!(display.contains("credentials.json"));

        let imp_info = CredentialInfo {
            source: CredentialSource::ImpersonatedServiceAccount,
            details: Some("sre-log-viewer@prod-proj.iam.gserviceaccount.com".to_string()),
        };
        assert_eq!(
            imp_info.to_string(),
            "IAM Service Account Impersonation (sre-log-viewer@prod-proj.iam.gserviceaccount.com)"
        );
    }

    #[test]
    fn test_iam_impersonation_url_and_token_cache_validity() {
        let url = iam_impersonation_url("sre-viewer@my-prod.iam.gserviceaccount.com");
        assert_eq!(
            url,
            "https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/sre-viewer@my-prod.iam.gserviceaccount.com:generateAccessToken"
        );

        let now = Utc::now();
        let valid_token = CachedImpersonatedToken {
            access_token: "ya29.impersonated".to_string(),
            expires_at: now + Duration::minutes(10),
        };
        assert!(valid_token.is_valid_at(now));

        // Token expiring in 30 seconds (< 60s safety margin) is treated as expired
        let expiring_soon = CachedImpersonatedToken {
            access_token: "ya29.expiring".to_string(),
            expires_at: now + Duration::seconds(30),
        };
        assert!(!expiring_soon.is_valid_at(now));
    }

    #[test]
    fn test_auth_error_messages() {
        let err = AuthError::NoCredentials;
        let msg = err.to_string();
        assert!(msg.contains("gcloud auth application-default login"));
    }
}
