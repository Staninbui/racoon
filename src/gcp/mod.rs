//! GCP integration module.
//!
//! This module provides authentication and API access to GCP services,
//! primarily focused on Cloud Logging.
//!
//! # Submodules
//!
//! - `auth`: Authentication and authorization for GCP services.
//! - `logs`: Cloud Logging API access.

pub mod auth;
pub mod logs;
pub mod projects;

// Re-export commonly used types
pub use auth::{AuthError, CredentialInfo, CredentialSource, GcpAuth};
pub use logs::{
    LogEntry, LogFilter, LogsClient, LogsError, StreamHandle, extract_first_gcp_project_id,
    is_local_only_filter, matches_local_filter, parse_resource_names,
};
pub use projects::{Project, ProjectsClient, ProjectsError};
