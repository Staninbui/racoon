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
mod logs;

pub use auth::{AuthError, GcpAuth};
// pub mod logs;