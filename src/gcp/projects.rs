//! GCP Project management operations.
//!
//! Uses the Cloud Resource Manager API to list and query projects.

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

use super::auth::GcpAuth;

/// Errors that can occur during project operations.
#[derive(Error, Debug)]
pub enum ProjectsError {
    #[error("Authentication error: {0}")]
    Auth(#[from] super::auth::AuthError),

    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },
}

/// GCP Project info.
#[derive(Debug, Deserialize, Clone)]
pub struct Project {
    /// Project ID (e.g., "my-project-123")
    #[serde(rename = "projectId")]
    pub project_id: String,

    /// Project display name
    pub name: Option<String>,

    /// Project number
    #[serde(rename = "projectNumber")]
    pub project_number: Option<String>,

    /// Project lifecycle state (ACTIVE, DELETE_REQUESTED, etc.)
    #[serde(rename = "lifecycleState")]
    pub lifecycle_state: Option<String>,
}

impl Project {
    /// Check if the project is active.
    pub fn is_active(&self) -> bool {
        self.lifecycle_state.as_deref() == Some("ACTIVE")
    }

    /// Get display name or project ID as fallback.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.project_id)
    }
}

/// Client for GCP Project operations.
#[derive(Clone)]
pub struct ProjectsClient {
    auth: GcpAuth,
    http: Client,
}

/// Response from the Resource Manager API.
#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    projects: Option<Vec<Project>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

impl ProjectsClient {
    /// Create a new projects client.
    pub fn new(auth: GcpAuth) -> Self {
        Self {
            auth,
            http: Client::new(),
        }
    }

    /// List all accessible GCP projects.
    ///
    /// Only returns ACTIVE projects by default.
    pub async fn list(&self) -> Result<Vec<Project>, ProjectsError> {
        self.list_filtered(true).await
    }

    /// List projects with optional filtering.
    ///
    /// # Arguments
    ///
    /// * `active_only` - If true, only return ACTIVE projects
    pub async fn list_filtered(&self, active_only: bool) -> Result<Vec<Project>, ProjectsError> {
        let token = self.auth.token().await?;

        let mut all_projects = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = "https://cloudresourcemanager.googleapis.com/v1/projects".to_string();

            if let Some(ref token) = page_token {
                url = format!("{}?pageToken={}", url, token);
            }

            let response = self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                return Err(ProjectsError::Api {
                    status,
                    message: body,
                });
            }

            let data: ProjectsResponse = response.json().await?;

            if let Some(projects) = data.projects {
                all_projects.extend(projects);
            }

            match data.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        // Filter if needed
        let projects = if active_only {
            all_projects.into_iter().filter(|p| p.is_active()).collect()
        } else {
            all_projects
        };

        Ok(projects)
    }

    /// Get a specific project by ID.
    pub async fn get(&self, project_id: &str) -> Result<Project, ProjectsError> {
        let token = self.auth.token().await?;

        let url = format!(
            "https://cloudresourcemanager.googleapis.com/v1/projects/{}",
            project_id
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(ProjectsError::Api {
                status,
                message: body,
            });
        }

        let project: Project = response.json().await?;
        Ok(project)
    }

    // 🔮 Future expansion:
    // pub async fn get_iam_policy(&self, project_id: &str) -> Result<IamPolicy, ProjectsError>
    // pub async fn get_billing_info(&self, project_id: &str) -> Result<BillingInfo, ProjectsError>
    // pub async fn list_services(&self, project_id: &str) -> Result<Vec<Service>, ProjectsError>
}
