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
    /// Global log level (overrides config file when specified)
    #[arg(short, long, global = true)]
    pub log_level: Option<String>,

    /// Impersonate a GCP service account email for all Cloud Logging & Vertex AI calls
    /// (e.g. `sre-prod-reader@my-proj.iam.gserviceaccount.com`)
    #[arg(long, global = true)]
    pub impersonate_service_account: Option<String>,

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

    /// Manage configuration
    Config(ConfigArgs),

    /// Select, list, or update AI models (Google, OpenAI, Anthropic, Agnes AI)
    #[command(visible_alias = "model")]
    Models(ModelsArgs),

    /// Check for racoon updates and refresh/overwrite the AI models registry
    Update,
}

/// the arguments for tail command
#[derive(Parser, Debug)]
pub struct TailArgs {
    /// GCP project ID(s) or Log View resource names (comma-separated, e.g. `prod-gw,prod-order`)
    #[arg(short, long)]
    pub project: Option<String>,

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

    /// time range of logs (uses config default if not specified)
    ///
    /// for example: 30s, 5m, 1h, 2d
    #[arg(long)]
    pub since: Option<String>,

    /// Start timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-2h')
    #[arg(long)]
    pub from: Option<String>,

    /// End timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-1h')
    #[arg(long)]
    pub to: Option<String>,
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

    /// Relative time range of logs (e.g., 30s, 5m, 1h, 2d)
    #[arg(long)]
    pub since: Option<String>,

    /// Start timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-2h')
    #[arg(long)]
    pub from: Option<String>,

    /// End timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-1h')
    #[arg(long)]
    pub to: Option<String>,

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
    /// gcp project id (uses config default if not specified)
    #[arg(short, long)]
    pub project: Option<String>,

    /// gcp logging filter
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Use AI to generate filter from natural language
    ///
    /// for example: --ai "all errors from Cloud Run today"
    #[arg(long)]
    pub ai: Option<String>,

    /// time range of logs (e.g., 30s, 5m, 1h, 2d)
    #[arg(long)]
    pub since: Option<String>,

    /// Start timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-2h')
    #[arg(long)]
    pub from: Option<String>,

    /// End timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-1h')
    #[arg(long)]
    pub to: Option<String>,

    /// log limit
    #[arg(long)]
    pub limit: Option<usize>,

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

    /// Print logs to stdout instead of launching interactive TUI
    #[arg(long)]
    pub no_tui: bool,
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

    /// Relative time range of logs (e.g., 30s, 5m, 1h, 2d)
    #[arg(long)]
    pub since: Option<String>,

    /// Start timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-2h')
    #[arg(long)]
    pub from: Option<String>,

    /// End timestamp for log window (RFC3339, 'YYYY-MM-DD HH:MM:SS', or relative like '-1h')
    #[arg(long)]
    pub to: Option<String>,

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

/// Config subcommands
#[derive(Parser, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

/// Config subcommands.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show config file path
    Path,

    /// Initialize default config file
    Init,

    /// Show current configuration
    Show,

    /// Edit config file in default editor
    Edit,
}

/// Arguments for `racoon models` command.
#[derive(Parser, Debug)]
pub struct ModelsArgs {
    /// Action to perform (defaults to interactive model selector if omitted)
    #[command(subcommand)]
    pub action: Option<ModelsAction>,
}

/// Subcommands for `racoon models`.
#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelsAction {
    /// Interactively select AI provider and model using arrow keys (default)
    #[command(visible_alias = "use", visible_alias = "switch")]
    Select,

    /// List all available providers and models
    List,

    /// Manually update model lists from remote provider APIs
    #[command(visible_alias = "refresh")]
    Update,
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

    #[test]
    fn test_query_args_ai_flag() {
        let cli = Cli::try_parse_from([
            "racoon",
            "query",
            "--project",
            "demo-proj",
            "--ai",
            "errors in last hour",
            "--since",
            "2h",
        ])
        .unwrap();
        match cli.command {
            Commands::Query(args) => {
                assert_eq!(args.project.as_deref(), Some("demo-proj"));
                assert_eq!(args.ai.as_deref(), Some("errors in last hour"));
                assert_eq!(args.since.as_deref(), Some("2h"));
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_models_and_update_commands() {
        // `racoon models` without arguments defaults to None (interactive selector)
        let cli = Cli::try_parse_from(["racoon", "models"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Models(ModelsArgs { action: None })
        ));

        // `racoon models select` does not accept model name arguments
        let err = Cli::try_parse_from(["racoon", "models", "select", "gpt-4o"]);
        assert!(err.is_err());

        // `racoon models update`
        let cli_update = Cli::try_parse_from(["racoon", "models", "update"]).unwrap();
        assert!(matches!(
            cli_update.command,
            Commands::Models(ModelsArgs {
                action: Some(ModelsAction::Update)
            })
        ));

        // `racoon update`
        let root_update = Cli::try_parse_from(["racoon", "update"]).unwrap();
        assert!(matches!(root_update.command, Commands::Update));
    }

    #[test]
    fn test_global_impersonate_service_account_flag() {
        let cli = Cli::try_parse_from([
            "racoon",
            "--impersonate-service-account",
            "sre-reader@prod-proj.iam.gserviceaccount.com",
            "tail",
            "--project",
            "prod-proj",
        ])
        .unwrap();
        assert_eq!(
            cli.impersonate_service_account.as_deref(),
            Some("sre-reader@prod-proj.iam.gserviceaccount.com")
        );
    }

    #[test]
    fn test_historical_time_range_flags_from_to_since() {
        let q_cli = Cli::try_parse_from([
            "racoon",
            "query",
            "--project",
            "prod-proj",
            "--from",
            "2026-10-27T14:00:00Z",
            "--to",
            "2026-10-27T14:30:00Z",
        ])
        .unwrap();
        match q_cli.command {
            Commands::Query(args) => {
                assert_eq!(args.from.as_deref(), Some("2026-10-27T14:00:00Z"));
                assert_eq!(args.to.as_deref(), Some("2026-10-27T14:30:00Z"));
            }
            _ => panic!("Expected Query command"),
        }

        let exp_cli = Cli::try_parse_from([
            "racoon",
            "export",
            "--project",
            "prod-proj",
            "--output",
            "incident.json",
            "--since",
            "2h",
            "--from",
            "2026-10-27 14:00:00",
            "--to",
            "2026-10-27 15:00:00",
        ])
        .unwrap();
        match exp_cli.command {
            Commands::Export(args) => {
                assert_eq!(args.since.as_deref(), Some("2h"));
                assert_eq!(args.from.as_deref(), Some("2026-10-27 14:00:00"));
                assert_eq!(args.to.as_deref(), Some("2026-10-27 15:00:00"));
            }
            _ => panic!("Expected Export command"),
        }
    }
}
