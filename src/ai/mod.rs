//! AI integration module for racoon.
//!
//! Provides AI-powered log analysis and natural language query generation
//! across multiple LLM providers (Google Gemini, OpenAI GPT, Anthropic Claude, Agnes AI).

pub mod gemini;
pub mod models;
pub mod prompts;
pub mod selector;

pub use gemini::{AiClient, AiError, GeminiClient, UserIntent};
pub use models::{ModelInfo, ModelsRegistry, ProviderKind, ProviderUpdateStatus, builtin_models};
pub use selector::{run_model_selector, select_with_terminal};
