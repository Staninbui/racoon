//! Low-level HTTP transport and wire-protocol implementations for AI providers
//! (Google Gemini REST API, Google Cloud Vertex AI, OpenAI-compatible `/chat/completions`,
//! and Anthropic `/messages`).

use serde::{Deserialize, Serialize};

use super::agent::{AiClient, AiError};
use super::models::ProviderKind;
use crate::gcp::GcpAuth;

impl AiClient {
    /// Build the Vertex AI `generateContent` endpoint URL for the configured project, location, and model.
    pub fn vertex_endpoint_url(&self) -> Result<String, AiError> {
        let project = self
            .vertex_project
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| {
                AiError::ApiError(
                    "Vertex AI requires a GCP project ID. Pass --project <ID>, set [defaults].project, or set VERTEX_PROJECT / GOOGLE_CLOUD_PROJECT."
                        .to_string(),
                )
            })?;
        let location = self.vertex_location.trim();
        let default_base = ProviderKind::VertexAi.default_base_url();
        let host_base = if self.base_url != default_base && !self.base_url.trim().is_empty() {
            self.base_url.trim_end_matches('/').to_string()
        } else if location.eq_ignore_ascii_case("global") {
            "https://aiplatform.googleapis.com/v1".to_string()
        } else {
            format!("https://{}-aiplatform.googleapis.com/v1", location)
        };

        Ok(format!(
            "{}/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
            host_base, project, location, self.model
        ))
    }

    /// Generate content using the configured AI provider with a task-specific `max_output_tokens` cap.
    pub(crate) async fn generate_with_max_tokens(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, AiError> {
        match self.provider {
            ProviderKind::Google => self.generate_google(prompt, max_tokens).await,
            ProviderKind::VertexAi => self.generate_vertex(prompt, max_tokens).await,
            ProviderKind::OpenAi | ProviderKind::Agnes => {
                self.generate_openai_compatible(prompt, max_tokens).await
            }
            ProviderKind::Anthropic => self.generate_anthropic(prompt, max_tokens).await,
        }
    }

    /// Generate content via Google Cloud Vertex AI (`publishers/google/models/{model}:generateContent`)
    /// using GCP Application Default Credentials (or IAM Service Account Impersonation).
    async fn generate_vertex(&self, prompt: &str, max_tokens: u32) -> Result<String, AiError> {
        let url = self.vertex_endpoint_url()?;

        let bearer_token = if !self.api_key.trim().is_empty() {
            self.api_key.trim().to_string()
        } else if let Some(ref auth) = self.gcp_auth {
            auth.cloud_platform_token().await.map_err(|e| {
                AiError::ApiError(format!("Vertex AI GCP authentication failed: {}", e))
            })?
        } else {
            let auth = GcpAuth::new().await.map_err(|e| {
                AiError::ApiError(format!("Vertex AI GCP authentication init failed: {}", e))
            })?;
            auth.cloud_platform_token().await.map_err(|e| {
                AiError::ApiError(format!("Vertex AI GCP token acquisition failed: {}", e))
            })?
        };

        let request_body = GeminiRequest {
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.3),
                max_output_tokens: Some(max_tokens),
                top_p: Some(0.8),
                top_k: Some(40),
            }),
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(&bearer_token)
            .json(&request_body)
            .send()
            .await?;
        let status = response.status();

        if status == 429 {
            return Err(AiError::RateLimited);
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "Vertex AI HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or_else(|| AiError::ParseError("No content in Vertex AI response".to_string()))?;

        Ok(text)
    }

    /// Generate content via Google Gemini REST API (`generateContent`).
    async fn generate_google(&self, prompt: &str, max_tokens: u32) -> Result<String, AiError> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url.trim_end_matches('/'),
            self.model,
            self.api_key
        );

        let request_body = GeminiRequest {
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.3),
                max_output_tokens: Some(max_tokens),
                top_p: Some(0.8),
                top_k: Some(40),
            }),
        };

        let response = self.http.post(&url).json(&request_body).send().await?;
        let status = response.status();

        if status == 429 {
            return Err(AiError::RateLimited);
        }

        if status == 400 || status == 401 || status == 403 {
            let error_body = response.text().await.unwrap_or_default();
            if error_body.contains("API_KEY_INVALID") {
                return Err(AiError::InvalidApiKey);
            }
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: GeminiResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or_else(|| AiError::ParseError("No content in response".to_string()))?;

        Ok(text)
    }

    /// Generate content via OpenAI-compatible `/chat/completions` (used by OpenAI and Agnes AI).
    async fn generate_openai_compatible(
        &self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, AiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let request_body = OpenAiChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: Some(0.3),
            max_tokens: Some(max_tokens),
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if status == 429 {
            return Err(AiError::RateLimited);
        }
        if status == 401 || status == 403 {
            return Err(AiError::InvalidApiKey);
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: OpenAiChatResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| AiError::ParseError("No choices in response".to_string()))?;

        Ok(text)
    }

    /// Generate content via Anthropic `/messages` API.
    async fn generate_anthropic(&self, prompt: &str, max_tokens: u32) -> Result<String, AiError> {
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));

        let request_body = AnthropicMessagesRequest {
            model: self.model.clone(),
            max_tokens,
            temperature: Some(0.3),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await?;

        let status = response.status();
        if status == 429 {
            return Err(AiError::RateLimited);
        }
        if status == 401 || status == 403 {
            return Err(AiError::InvalidApiKey);
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let response_body: AnthropicMessagesResponse = response
            .json()
            .await
            .map_err(|e| AiError::ParseError(e.to_string()))?;

        let text = response_body
            .content
            .iter()
            .find_map(|b| b.text.clone())
            .ok_or_else(|| AiError::ParseError("No text content in response".to_string()))?;

        Ok(text)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Part {
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Content,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: ChatMessage,
}

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(default)]
    text: Option<String>,
}
