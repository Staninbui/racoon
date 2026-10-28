//! Prompt templates for AI operations.
//!
//! Contains carefully crafted prompts for different AI tasks.
//! These prompts are designed to produce consistent, useful output.

/// Generate a prompt for explaining a single log entry.
///
/// # Arguments
///
/// * `log_json` - The compacted log entry serialized as JSON
pub fn explain_log_prompt(log_json: &str) -> String {
    format!(
        r#"You are a Senior GCP SRE. Analyze this compacted log entry (max 160 words, no filler):
{log_json}

Provide:
1. **Summary**: What happened (1-2 sentences)
2. **Severity & Impact**: Normal, warning, or critical? What is affected?
3. **Next Steps**: Concrete remediation or investigation commands"#
    )
}

/// Generate a prompt for converting natural language to GCP query.
///
/// # Arguments
///
/// * `description` - Natural language description of what to search for
pub fn generate_query_prompt(description: &str) -> String {
    format!(
        r#"Convert to a valid GCP Cloud Logging filter query (return ONLY raw query syntax, no markdown):
Request: "{description}"
Fields: severity (DEBUG|INFO|WARNING|ERROR|CRITICAL), resource.type (cloud_run_revision|k8s_container|gce_instance|cloud_function), timestamp, textPayload, jsonPayload.
Examples:
- "errors from cloud run" -> resource.type="cloud_run_revision" severity>=ERROR
- "warnings in the last hour" -> severity>=WARNING timestamp>="-1h"
Query:"#
    )
}

/// Generate a prompt for analyzing multiple logs.
///
/// # Arguments
///
/// * `logs_text` - Compacted & RLE-folded chronological logs
pub fn analyze_logs_prompt(logs_text: &str) -> String {
    format!(
        r#"You are a Senior GCP SRE performing Root Cause Analysis on chronological logs (`xN repeats` = folded identical patterns):
{logs_text}

Provide concisely (max 240 words):
1. **Timeline Summary**: Key sequence of events
2. **Root Cause Analysis**: Primary trigger vs cascading symptoms
3. **Affected Components**: Impacted services/resources
4. **Recommendations**: Immediate fix & prevention"#
    )
}

/// Generate a prompt for classifying user input intent and synthesizing a GCP filter query in one LLM call.
pub fn detect_intent_prompt(input: &str) -> String {
    format!(
        r#"Classify user input for GCP Cloud Logging and synthesize a GCP filter query if applicable.
Input: "{input}"
Intents:
- QUESTION: asking about logs (include "gcp_query" to fetch relevant logs)
- SET_FILTER: changing active filter
- GENERATE_QUERY: requesting query syntax
Return JSON only:
{{"intent":"QUESTION"|"SET_FILTER"|"GENERATE_QUERY","gcp_query":"<valid GCP filter>","description":"<short note>"}}"#
    )
}

/// Generate a prompt for suggesting filters based on current log content.
///
/// # Arguments
///
/// * `log_sample` - A sample log entry to analyze
pub fn suggest_filters_prompt(log_sample: &str) -> String {
    format!(
        r#"Based on this GCP log entry, suggest 3-5 useful filter queries:

```json
{log_sample}
```

For each suggestion, provide:
1. The GCP Logging query
2. A brief description of what it filters

Format each as:
QUERY: <query>
DESC: <description>

Focus on practically useful filters for debugging and monitoring."#
    )
}

/// Generate a prompt for answering questions about logs.
pub fn answer_question_prompt(question: &str, logs_summary: &str, log_count: usize) -> String {
    format!(
        r#"You are an expert Site Reliability Engineer (SRE) analyzing GCP logs.

## User's Question
"{question}"

## Query Results
Found {count} matching log entries:

```
{logs}
```

## Instructions
1. Answer in the SAME LANGUAGE as the question
2. Be direct and concise
3. If there are errors, explain: what, when, impact, and suggested fixes
4. If no relevant logs, clearly state that
5. Max 200 words

Your answer:"#,
        question = question,
        count = log_count,
        logs = logs_summary
    )
}

/// Generate a prompt for answering a multi-turn follow-up question in the context of
/// a previous AI analysis / RCA and the active log/trace buffer.
pub fn followup_question_prompt(
    question: &str,
    previous_analysis: &str,
    logs_context: &str,
) -> String {
    format!(
        r#"You are a Senior GCP SRE continuing an active incident investigation.

## Previous AI Diagnosis
{previous_analysis}

## Active Log / Trace Context
```
{logs_context}
```

## Engineer's Follow-up Question
"{question}"

## Instructions
1. Answer in the SAME LANGUAGE as the engineer's follow-up question
2. Directly build upon the Previous AI Diagnosis and Active Log Context
3. Provide concrete commands, configuration snippets, or root-cause clarifications
4. Keep response under 200 words, no filler"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explain_log_prompt_contains_json() {
        let prompt = explain_log_prompt(r#"{"severity": "ERROR"}"#);
        assert!(prompt.contains(r#"{"severity": "ERROR"}"#));
        assert!(prompt.contains("SRE"));
    }

    #[test]
    fn test_generate_query_prompt_contains_description() {
        let prompt = generate_query_prompt("errors from cloud run");
        assert!(prompt.contains("errors from cloud run"));
        assert!(prompt.contains("severity"));
    }

    #[test]
    fn test_analyze_logs_prompt() {
        let prompt = analyze_logs_prompt("log1\n---\nlog2");
        assert!(prompt.contains("Timeline Summary"));
        assert!(prompt.contains("Root Cause"));
    }

    #[test]
    fn test_followup_question_prompt_retains_previous_analysis() {
        let prompt = followup_question_prompt(
            "How do I increase HikariCP pool size?",
            "### Root Cause\nHikariCP pool exhausted",
            "[12:00:01] [ERROR] HikariPool-1 - Connection is not available",
        );
        assert!(prompt.contains("How do I increase HikariCP pool size?"));
        assert!(prompt.contains("HikariCP pool exhausted"));
        assert!(prompt.contains("HikariPool-1"));
    }
}
