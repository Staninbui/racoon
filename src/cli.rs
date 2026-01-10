//! CLI entry point
//!
//! racoon is a simple gcp logs checking tool.

use clap::{Parser, Subcommand, ValueEnum};

/// racoon - GCP Log Spy with AI
///
/// it provides a simple interface for checking gcp logs with AI
#[derive(Parser, Debug)]
#[command(name = "racoon")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Global log level
    #[arg(short, long, global = true, default_value = "info")]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Commands,
}

/// the subcommand list that racoon provides
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// check logs by streaming
    Tail(TailArgs),

    /// list available projects
    Projects,

    /// check auth status
    Auth,

    /// download logs into local
    Download(DownloadArgs),

    /// check log in a single query
    Query(QueryArgs),

    /// Load and view logs from a local file (offline mode)
    View(ViewArgs),

    /// Export logs with advanced options
    Export(ExportArgs),
}

/// the arguments for tail command
#[derive(Parser, Debug)]
pub struct TailArgs {
    /// GCP project ID(required)
    #[arg(short, long)]
    pub project: String,

    /// GCP logging query filters
    ///
    /// Can use GCP logging query syntax, for example:
    /// - `severity>=ERROR`
    /// - `resource.type="cloud_run_revision"`
    /// - `jsonPayload.message=~"timeout"`
    #[arg(short, long)]
    pub filter: Option<String>,

    /// GCP logging severity
    #[arg(short, long, value_enum)]
    pub severity: Option<Severity>,

    /// GCP logging resource type
    ///
    /// for example: cloud_run_revision, gce_instance, gke_container, cloud_function
    #[arg(short, long)]
    pub resource: Option<String>,

    /// time range of logs
    ///
    /// for example: 30s, 5m, 1h, 2d
    #[arg(long, default_value = "1h")]
    pub since: String,
}

/// the arguments for download command
#[derive(Parser, Debug)]
pub struct DownloadArgs {
    /// gcp project id
    #[arg(short, long)]
    pub project: String,

    /// logging filters
    #[arg(short, long)]
    pub filter: Option<String>,

    /// output file path
    #[arg(short, long, default_value = "logs.json")]
    pub output: String,

    /// log limit
    #[arg(long, default_value = "1000")]
    pub limit: usize,

    /// Ai chat mode
    ///
    /// for example: --ai "error logs of cloud run in last 1 hour"
    #[arg(long)]
    pub ai: Option<String>,
}

/// the arguments for query command
#[derive(Parser, Debug)]
pub struct QueryArgs {
    /// gcp project id
    #[arg(short, long)]
    pub project: String,

    /// gcp logging filter
    #[arg(short, long)]
    pub filter: Option<String>,

    /// log limit
    #[arg(long, default_value = "50")]
    pub limit: usize,

    /// output format
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `racoon view` command.
#[derive(Parser, Debug)]
pub struct ViewArgs {
    /// Path to the log file to view
    pub file: String,

    /// Maximum number of logs to load
    #[arg(long, default_value = "100")]
    pub limit: usize,

    /// Output as JSON instead of TUI
    #[arg(long)]
    pub json: bool,
}

/// Arguments for `racoon export` command.
#[derive(Parser, Debug)]
pub struct ExportArgs {
    /// GCP Project ID
    #[arg(short, long)]
    pub project: String,

    /// Output file path
    #[arg(short, long)]
    pub output: String,

    /// GCP Logging filter
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Use AI to generate filter from natural language
    #[arg(long)]
    pub ai: Option<String>,

    /// Maximum number of logs to export
    #[arg(long, default_value = "1000")]
    pub limit: usize,

    /// Output format (json or ndjson)
    #[arg(long, default_value = "json")]
    pub format: String,

    /// Don't include metadata in output
    #[arg(long)]
    pub no_metadata: bool,

    /// Compact output (no pretty printing)
    #[arg(long)]
    pub compact: bool,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum Severity {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl Severity {
    /// query syntax translator
    pub fn to_filter(&self) -> &'static str {
        match self {
            Severity::Debug => "severity>=DEBUG",
            Severity::Info => "severity>=INFO",
            Severity::Notice => "severity>=NOTICE",
            Severity::Warning => "severity>=WARNING",
            Severity::Error => "severity>=ERROR",
            Severity::Critical => "severity>=CRITICAL",
            Severity::Alert => "severity>=ALERT",
            Severity::Emergency => "severity>=EMERGENCY",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        // verify the cli arguments
        Cli::command().debug_assert();
    }

    #[test]
    fn test_severity_to_filter() {
        assert_eq!(Severity::Debug.to_filter(), "severity>=DEBUG");
        assert_eq!(Severity::Warning.to_filter(), "severity>=WARNING");
    }
}