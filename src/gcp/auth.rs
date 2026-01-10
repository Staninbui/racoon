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

use gcp_auth::TokenProvider;
use std::fmt::Formatter;
use std::sync::Arc;
use thiserror::Error;

/// OAuth2 scopes required for Cloud Logging read access.
const LOGGING_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/logging.read",
    "https://www.googleapis.com/auth/cloud-platform.read-only",
];

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
}

impl GcpAuth {
    /// Create a new authentication manager.
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

        Ok(Self { provider })
    }

    /// Get an OAuth2 access token for GCP API calls.
    ///
    /// The returned token can be used as a Bearer token in HTTP headers:
    /// ```text
    /// Authorization: Bearer <token>
    /// ```
    ///
    /// # Token Lifecycle
    ///
    /// - Tokens are cached internally and automatically refreshed when expired.
    /// - It's safe to call this method before every API request.
    /// - Tokens typically expire after 1 hour but are refreshed automatically.
    ///
    /// # Errors
    ///
    /// Returns `AuthError::TokenRefresh` if the token cannot be obtained or refreshed.
    pub async fn token(&self) -> Result<String, AuthError> {
        let token = self
            .provider
            .token(LOGGING_SCOPES)
            .await
            .map_err(|e| AuthError::TokenRefresh(e.to_string()))?;

        Ok(token.as_str().to_string())
    }

    /// Verify that the current credentials are valid.
    ///
    /// This is a convenience method that attempts to obtain a token and
    /// return `Ok(())` if successful, Use this for health checks or before
    /// starting long-running operations.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are invalid or token refresh fails.
    pub async fn verify(&self) -> Result<(), AuthError> {
        self.token().await?;
        Ok(())
    }

    /// Get detailed information about the current credentials.
    ///
    /// Returns a human-readable description of the credential source.
    /// Useful for debugging and user feedback.
    pub fn credential_info(&self) -> CredentialInfo {
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
    /// Additional details (e.g., file path).
    pub details: Option<String>,
}

impl std::fmt::Display for CredentialInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.source {
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
        assert!(display.contains("credentials.json"))
    }

    #[test]
    fn test_auth_error_messages() {
        let err = AuthError::NoCredentials;
        let msg = err.to_string();
        assert!(msg.contains("gcloud auth application-default login"));
    }

    // Note: Integration tests for actual GCP auth would require
    // valid credentials and are better suited for tests/ directory
    // with proper test fixtures or mocking.
}
