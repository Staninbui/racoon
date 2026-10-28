//! AI integration module for racoon.
//!
//! Provides AI-powered log analysis and natural language query generation
//! across multiple LLM providers (Google Gemini, Vertex AI, OpenAI GPT, Anthropic Claude, Agnes AI).
//!
//! Notice that this module has zero dependency on the `ui` layer.

pub mod agent;
pub mod cache;
pub mod compact;
pub mod models;
pub mod prompts;
pub mod providers;
pub mod redact;

pub use crate::ui::selector::{ModelSelectorState, run_model_selector, select_with_terminal};
pub use agent as gemini;
pub use agent::{AiAgent, AiClient, AiError, GeminiClient, QuestionAnswer, UserIntent};
pub use compact::{
    CompactionStats, compact_log_for_ai, compact_logs_for_rca, compact_text, estimate_tokens,
    try_local_intent,
};
pub use models::{ModelInfo, ModelsRegistry, ProviderKind, ProviderUpdateStatus, builtin_models};
pub use redact::{RedactionStats, redact_sensitive_text};
