//! racoon CLI entry point.
//!
//! Responsibilities:
//! - Parse command-line arguments
//! - Initialize logging/tracing system
//! - Dispatch to appropriate command handlers
//! - Handle top-level errors gracefully

use std::io::IsTerminal;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use tokio::sync::mpsc;
use tracing::{Level, error, info, warn};
use tracing_subscriber::FmtSubscriber;

use gcp_racoon::ai::GeminiClient;
use gcp_racoon::cli::{
    Cli, Commands, ConfigAction, ConfigArgs, DownloadArgs, ExportArgs, QueryArgs, TailArgs,
    ViewArgs,
};
use gcp_racoon::config::Config;
use gcp_racoon::gcp::{GcpAuth, LogFilter, LogsClient, ProjectsClient};
use gcp_racoon::storage::{
    ExportFormat, ExportOptions, export_logs, export_logs_with_ai, import_logs,
};
use gcp_racoon::ui::theme::Theme;
use gcp_racoon::ui::{App, run_app};
use gcp_racoon::utils::sanitize_query;

#[tokio::main]
async fn main() -> Result<()> {
    // parse command line arguments
    let cli = Cli::parse();

    // load config
    let config = Config::load();

    // Determine whether this command will enter interactive TUI mode
    let is_tui = match &cli.command {
        Commands::Tail(_) => true,
        Commands::View(args) => !args.json && !args.no_tui && std::io::stdout().is_terminal(),
        _ => false,
    };

    // CLI --log-level overrides config.logging.level
    let log_level = cli
        .log_level
        .as_deref()
        .unwrap_or(&config.logging.level)
        .to_string();

    // Suppress tracing output during interactive TUI so terminal buffer isn't corrupted
    if !is_tui {
        init_tracing(&log_level)?;
        info!("racoon starting up 🦝");
    }

    // Dispatch to command handlers
    let result = match cli.command {
        Commands::Tail(args) => handle_tail(args, &config).await,
        Commands::Projects => handle_projects().await,
        Commands::Auth => handle_auth().await,
        Commands::Download(args) => handle_download(args, &config).await,
        Commands::Query(args) => handle_query(args, &config).await,
        Commands::Export(args) => handle_export(args, &config).await,
        Commands::View(args) => handle_view(args, &config).await,
        Commands::Config(args) => handle_config(args),
    };

    // Handle errors gracefully at the top level
    if let Err(ref e) = result {
        error!("Command failed: {:#}", e);
    }

    result
}

/// Parse log level string into `tracing::Level`.
fn parse_log_level(level: &str) -> Level {
    match level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" | "warning" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprintln!("Unknown log level '{}', defaulting to 'info'", level);
            Level::INFO
        }
    }
}

/// initialize tracing (writes to stderr so stdout JSON output stays clean)
fn init_tracing(level: &str) -> Result<()> {
    let level = parse_log_level(level);

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    Ok(())
}

/// handle tail command
async fn handle_tail(args: TailArgs, config: &Config) -> Result<()> {
    // Use config default for project if not specified
    let project = args
        .project
        .or_else(|| config.defaults.project.clone())
        .ok_or_else(|| anyhow::anyhow!("Project required. Use --project or set in config file."))?;

    // Apply theme
    let theme = Theme::parse(&config.ui.theme);

    // Use config default for since
    let since_str = args.since.as_deref().unwrap_or(&config.defaults.since);

    let duration = gcp_racoon::parse_duration_chrono(since_str)
        .map_err(|e| anyhow::anyhow!("Invalid --since: {}", e))?;

    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("Failed to verify GCP authentication")?;

    println!("✅ Connected to GCP");

    // Try to initialize Gemini client
    let gemini = match GeminiClient::from_config(config) {
        Ok(client) => {
            info!("AI features enabled (Gemini)");
            Some(client)
        }
        Err(e) => {
            warn!("AI features disabled: {}", e);
            None
        }
    };

    let client = LogsClient::new(auth.clone(), project.clone());

    let mut filter = LogFilter::new().since(duration);
    let mut ui_filter_parts: Vec<String> = Vec::new();

    if let Some(ref f) = args.filter {
        filter = filter.raw(f);
        ui_filter_parts.push(f.clone());
    }

    if let Some(ref sev) = args.severity {
        let sev_str = format!("{:?}", sev).to_uppercase();
        filter = filter.severity_min(&sev_str);
        ui_filter_parts.push(format!("severity>={}", sev_str));
    } else if let Some(ref default_sev) = config.defaults.severity
        && args.filter.is_none()
    {
        let sev_str = default_sev.to_uppercase();
        filter = filter.severity_min(&sev_str);
        ui_filter_parts.push(format!("severity>={}", sev_str));
    }

    if let Some(ref res) = args.resource {
        filter = filter.resource_type(res);
        ui_filter_parts.push(format!("resource.type=\"{}\"", res));
    }

    info!("Filter: {}", filter.build());

    // Start streaming
    let (rx, _handle) = client.stream_entries(Some(filter), duration).await?;

    let initial_ui_filter = if ui_filter_parts.is_empty() {
        None
    } else {
        Some(ui_filter_parts.join(" "))
    };

    // Create app state with config settings
    let app = App::new(project.clone())
        .with_theme(theme)
        .with_max_logs(config.ui.max_logs)
        .with_filter(initial_ui_filter);

    // Run TUI
    run_app(app, rx, gemini, Some(client)).await?;

    info!("TUI exited");
    Ok(())
}

/// handle projects command
async fn handle_projects() -> Result<()> {
    println!("{}", "Fetching GCP projects...".dimmed());

    let auth = GcpAuth::new().await.context("Authentication failed")?;
    auth.verify()
        .await
        .context("GCP credentials verification failed")?;

    let client = ProjectsClient::new(auth);

    let projects = client.list().await?;

    if projects.is_empty() {
        println!("{}", "No accessible projects found.".yellow());
        return Ok(());
    }

    println!(
        "\n{}",
        format!("Found {} projects:", projects.len()).green().bold()
    );
    println!("{}", "─".repeat(60));
    println!("{:<40} {:<20}", "PROJECT ID".bold(), "NAME".bold());
    println!("{}", "─".repeat(60));

    for project in &projects {
        println!("{:<40} {:<20}", project.project_id, project.display_name());
    }

    println!("{}", "─".repeat(60));
    println!(
        "\nUse {} to stream logs",
        "racoon tail --project <PROJECT_ID>".cyan()
    );

    Ok(())
}

/// handle auth command
async fn handle_auth() -> Result<()> {
    info!("Verifying GCP authentication");

    println!("Checking GCP credentials...\n");

    // Attempt to initialize authentication
    match GcpAuth::new().await {
        Ok(auth) => {
            let cred_info = auth.credential_info();
            println!(" Credential Source: {}", cred_info);

            // Attempt to verify by getting a token
            match auth.verify().await {
                Ok(()) => {
                    println!("\n✅ Authentication successful!");
                    println!("   Credentials are valid and can obtain access tokens.");

                    if let Ok(token) = auth.token().await {
                        let preview = if token.len() > 20 {
                            format!("{}...", &token[..20])
                        } else {
                            token.clone()
                        };
                        println!("   Access token preview: {}", preview);
                    }
                }
                Err(e) => {
                    println!("\n❌ Authentication failed during token verification");
                    println!("   Error: {}", e);
                    println!("\n💡 Try running: gcloud auth application-default login");
                    return Err(e.into());
                }
            }
        }
        Err(e) => {
            println!("❌ No valid credentials found");
            println!("   Error: {}", e);
            println!();
            println!("💡 To authenticate, run one of:");
            println!("   1. gcloud auth application-default login");
            println!("   2. Set GOOGLE_APPLICATION_CREDENTIALS=/path/to/key.json");
            return Err(e.into());
        }
    }

    Ok(())
}

// handle download command
async fn handle_download(args: DownloadArgs, config: &Config) -> Result<()> {
    info!(
        project = %args.project,
        output = %args.output,
        limit = args.limit,
        ai = ?args.ai,
        "Downloading logs"
    );

    // Initialize and verify authentication
    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("Failed to verify GCP authentication")?;

    println!("✅ Authentication successful!");

    let client = LogsClient::new(auth, args.project.clone());

    let options = ExportOptions {
        limit: args.limit,
        ..Default::default()
    };

    let result = if let Some(ref ai_prompt) = args.ai {
        let gemini =
            GeminiClient::from_config(config).context("AI download requires GEMINI_API_KEY")?;

        println!("🦝 Generating query: \"{}\"", ai_prompt);

        let (query, result) =
            export_logs_with_ai(&client, &gemini, ai_prompt, &args.output, options).await?;

        println!("📋 Query: {}", query);
        result
    } else {
        let filter = args.filter.as_ref().map(|f| LogFilter::new().raw(f));
        export_logs(&client, filter.as_ref(), &args.output, options).await?
    };

    println!("✅ Downloaded {} logs to {}", result.count, result.path);
    Ok(())
}

/// handle query command
async fn handle_query(args: QueryArgs, config: &Config) -> Result<()> {
    let project = args
        .project
        .or_else(|| config.defaults.project.clone())
        .ok_or_else(|| anyhow::anyhow!("Project required. Use --project or set in config file."))?;

    let since_str = args.since.as_deref().unwrap_or(&config.defaults.since);
    let duration = gcp_racoon::parse_duration_chrono(since_str)
        .map_err(|e| anyhow::anyhow!("Invalid --since: {}", e))?;

    let limit = args.limit.unwrap_or(config.defaults.limit);

    // Initialize and verify authentication
    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("GCP credentials verification failed")?;

    if !args.json {
        println!("✅ Authentication successful!");
    }

    let client = LogsClient::new(auth, project.clone());

    // Build filter starting with --since duration
    let mut filter = LogFilter::new().since(duration);

    if let Some(ref ai_prompt) = args.ai {
        let gemini =
            GeminiClient::from_config(config).context("AI query requires GEMINI_API_KEY")?;

        if !args.json {
            println!("🦝 Generating query for: \"{}\"", ai_prompt);
        }

        let raw_query = gemini
            .generate_query(ai_prompt)
            .await
            .context("Failed to generate query with AI")?;
        let clean_query = sanitize_query(&raw_query);

        if !args.json {
            println!("📋 Generated filter: {}", clean_query);
        }

        // If AI generated its own timestamp constraint, use raw query directly;
        // otherwise combine with our since filter.
        if clean_query.contains("timestamp") {
            filter = LogFilter::new().raw(&clean_query);
        } else {
            filter = filter.raw(&clean_query);
        }
    } else if let Some(ref f) = args.filter {
        filter = filter.raw(f);
    } else if let Some(ref default_sev) = config.defaults.severity {
        filter = filter.severity_min(&default_sev.to_uppercase());
    }

    // Fetch logs
    let entries = client.list_entries(Some(&filter), limit).await?;

    if args.json {
        // JSON output
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        // Human-readable output
        println!("Found {} log entries:\n", entries.len());

        for entry in &entries {
            let severity = entry.severity_or_default();
            let color = match severity {
                "ERROR" | "CRITICAL" => "\x1b[31m",
                "WARNING" => "\x1b[33m",
                _ => "",
            };

            println!(
                "{}{} [{}]\x1b[0m {}",
                color,
                entry.timestamp_str(),
                severity,
                entry.message_preview()
            );
        }
    }

    Ok(())
}

/// Handle `racoon view` - view logs from local file
async fn handle_view(args: ViewArgs, config: &Config) -> Result<()> {
    if !args.json {
        println!("📂 Loading logs from: {}", args.file);
    }

    let result = import_logs(&args.file).context("Failed to import logs")?;

    if !args.json {
        println!("   Format: {}", result.format.name());
        println!("   Loaded: {} entries", result.count);

        if result.errors > 0 {
            println!("   ⚠️  {} entries failed to parse", result.errors);
        }
    }

    if args.json {
        // Output as JSON
        let entries_to_show: Vec<_> = result.entries.into_iter().take(args.limit).collect();
        println!("{}", serde_json::to_string_pretty(&entries_to_show)?);
    } else if result.entries.is_empty() {
        println!("\n(no entries to display)");
    } else if !args.no_tui && std::io::stdout().is_terminal() {
        // Launch interactive TUI in offline mode
        let entries_to_show: Vec<_> = result.entries.into_iter().take(args.limit).collect();
        let theme = Theme::parse(&config.ui.theme);
        let gemini = GeminiClient::from_config(config).ok();

        let (_tx, rx) = mpsc::channel(1);
        let app = App::new(format!("file:{}", args.file))
            .with_theme(theme)
            .with_max_logs(config.ui.max_logs.max(entries_to_show.len()))
            .with_offline_logs(entries_to_show);

        run_app(app, rx, gemini, None).await?;
    } else {
        // Display in terminal (when --no-tui or piped)
        println!();

        let entries_to_show = result.entries.iter().take(args.limit);

        for entry in entries_to_show {
            let severity = entry.severity_or_default();
            let color = match severity {
                "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY" => "\x1b[31m",
                "WARNING" => "\x1b[33m",
                "INFO" => "\x1b[32m",
                _ => "",
            };

            println!(
                "{}{} [{}]\x1b[0m {}",
                color,
                entry.timestamp_str(),
                severity,
                entry.message_preview()
            );
        }

        if result.count > args.limit {
            println!(
                "\n... and {} more (use --limit to see more)",
                result.count - args.limit
            );
        }
    }

    Ok(())
}

/// Handle `racoon export` - advanced export with options
async fn handle_export(args: ExportArgs, config: &Config) -> Result<()> {
    let auth = GcpAuth::new().await.context("Authentication failed")?;
    auth.verify().await?;

    let client = LogsClient::new(auth, args.project.clone());

    // Parse format
    let format: ExportFormat = args
        .format
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    let options = ExportOptions {
        format,
        pretty: !args.compact,
        include_metadata: !args.no_metadata,
        limit: args.limit,
    };

    println!("📤 Exporting logs...");
    println!("   Project: {}", args.project);
    println!("   Output: {}", args.output);
    println!("   Format: {}", format.name());

    let result = if let Some(ref ai_prompt) = args.ai {
        // AI-assisted export
        let gemini =
            GeminiClient::from_config(config).context("AI export requires GEMINI_API_KEY")?;

        println!("   🦝 AI query: \"{}\"", ai_prompt);

        let (query, result) =
            export_logs_with_ai(&client, &gemini, ai_prompt, &args.output, options).await?;

        println!("   Generated filter: {}", query);
        result
    } else {
        // Regular export
        let filter = args.filter.as_ref().map(|f| LogFilter::new().raw(f));

        if let Some(ref f) = filter {
            println!("   Filter: {}", f.build());
        }

        export_logs(&client, filter.as_ref(), &args.output, options).await?
    };

    println!();
    println!("✅ Export complete!");
    println!("   Entries: {}", result.count);
    println!("   Size: {} bytes", result.size_bytes);
    println!("   File: {}", result.path);

    Ok(())
}

/// Handle `racoon config` - manage configuration.
fn handle_config(args: ConfigArgs) -> Result<()> {
    use colored::Colorize;

    match args.action {
        ConfigAction::Path => {
            let path = Config::default_path();
            println!("{}", path.display());
        }

        ConfigAction::Init => {
            let path = Config::init()?;
            println!("{} {}", "Created:".green(), path.display());
            println!(
                "\nEdit with: {}",
                format!("$EDITOR {}", path.display()).cyan()
            );
        }

        ConfigAction::Show => {
            let config = Config::load();
            let path = Config::default_path();

            println!("{}", "Configuration".bold());
            println!("{}", "─".repeat(50));
            println!("Config file: {}", path.display());
            println!("File exists: {}", path.exists());
            println!();

            println!("{}", "[ai]".cyan());
            println!(
                "  api_key: {}",
                config
                    .ai
                    .api_key
                    .as_ref()
                    .map(|k| format!("{}...", &k[..8.min(k.len())]))
                    .unwrap_or_else(|| "(not set)".dimmed().to_string())
            );
            println!("  model: {}", config.ai.model);
            println!();

            println!("{}", "[defaults]".cyan());
            println!(
                "  project: {}",
                config.defaults.project.as_deref().unwrap_or("(not set)")
            );
            println!("  since: {}", config.defaults.since);
            println!("  limit: {}", config.defaults.limit);
            println!(
                "  severity: {}",
                config.defaults.severity.as_deref().unwrap_or("(not set)")
            );
            println!();

            println!("{}", "[ui]".cyan());
            println!("  max_logs: {}", config.ui.max_logs);
            println!("  mouse: {}", config.ui.mouse);
            println!("  theme: {}", config.ui.theme);
            println!();

            println!("{}", "[logging]".cyan());
            println!("  level: {}", config.logging.level);
        }

        ConfigAction::Edit => {
            let path = Config::default_path();

            // Create when doesn't exist
            if !path.exists() {
                Config::init()?;
            }

            // Open in editor
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

            std::process::Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("Failed to open editor: {}", editor))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_log_level_valid_levels() {
        assert_eq!(parse_log_level("trace"), Level::TRACE);
        assert_eq!(parse_log_level("debug"), Level::DEBUG);
        assert_eq!(parse_log_level("info"), Level::INFO);
        assert_eq!(parse_log_level("warn"), Level::WARN);
        assert_eq!(parse_log_level("warning"), Level::WARN);
        assert_eq!(parse_log_level("error"), Level::ERROR);
        assert_eq!(parse_log_level("INFO"), Level::INFO);
        assert_eq!(parse_log_level("DEBUG"), Level::DEBUG);
        assert_eq!(parse_log_level("unknown"), Level::INFO);
    }
}
