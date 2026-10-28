//! AI integration module for racoon.
//!
//! Provides AI-powered log analysis and natural language query generation
//! across multiple LLM providers (Google Gemini, OpenAI GPT, Anthropic Claude, Agnes AI).

pub mod compact;
pub mod gemini;
pub mod models;
pub mod prompts;
pub mod redact;
pub mod selector;

pub use compact::{
    CompactionStats, compact_log_for_ai, compact_logs_for_rca, compact_text, estimate_tokens,
    try_local_intent,
};
pub use gemini::{AiClient, AiError, GeminiClient, UserIntent};
pub use models::{ModelInfo, ModelsRegistry, ProviderKind, ProviderUpdateStatus, builtin_models};
pub use redact::{RedactionStats, redact_sensitive_text};
pub use selector::{run_model_selector, select_with_terminal};
