//! Prompt templates for AI operations.
//!
//! Contains carefully crafted prompts for different AI tasks.
//! These prompts are designed to produce consistent, useful output.

/// Generate a prompt for explaining a single log entry.
///
/// # Arguments
///
/// * `log_json` - The log entry serialized as JSON
pub fn explain_log_prompt(log_json: &str) -> String {
    format!(
        r#"You are an expert Site Reliability Engineer (SRE) and Google Cloud Platform specialist.

Analyze this GCP log entry and provide a concise explanation:

```json
{log_json}
```

Please provide:
1. **Summary**: What this log entry indicates (1-2 sentences)
2. **Severity Assessment**: Is this normal, concerning, or critical?
3. **Impact**: What might be affected by this event?
4. **Next Steps**: If action is needed, what should be done?

Keep your response concise and actionable. Use bullet points where appropriate.
Do not repeat the log content back - focus on insights."#
    )
}

/// Generate a prompt for converting natural language to GCP query.
///
/// # Arguments
///
/// * `description` - Natural language description of what to search for
pub fn generate_query_prompt(description: &str) -> String {
    format!(
        r#"You are a Google Cloud Logging query syntax expert.

Convert this natural language request into a valid GCP Logging query:

"{description}"

Rules:
1. Use correct GCP Logging query syntax
2. Common fields: severity, resource.type, timestamp, textPayload, jsonPayload
3. Common resource types: cloud_run_revision, gce_instance, gke_container, cloud_function, gae_app
4. Severity levels: DEBUG, INFO, NOTICE, WARNING, ERROR, CRITICAL, ALERT, EMERGENCY
5. Use proper quoting for string values
6. For time ranges, use relative expressions like: timestamp>="2024-01-01T00:00:00Z"

Return ONLY the query string, no explanations or markdown.

Examples:
- "errors from cloud run" → resource.type="cloud_run_revision" severity>=ERROR
- "warnings in the last hour" → severity>=WARNING timestamp>="-1h"
- "logs containing timeout" → "timeout"

Your query:"#
    )
}

/// Generate a prompt for analyzing multiple logs.
///
/// # Arguments
///
/// * `logs_text` - Multiple logs serialized, separated by ---
pub fn analyze_logs_prompt(logs_text: &str) -> String {
    format!(
        r#"You are an expert Site Reliability Engineer analyzing a series of related GCP logs.

Logs (oldest first):
{logs_text}

Analyze these logs and provide:

1. **Timeline Summary**: What happened in sequence?
2. **Root Cause Analysis**: What might be the underlying cause?
3. **Affected Components**: Which services/resources are impacted?
4. **Recommendations**: What actions should be taken?

Be concise and focus on actionable insights. If you cannot determine the root cause,
suggest what additional information or logs might be helpful."#
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
}
