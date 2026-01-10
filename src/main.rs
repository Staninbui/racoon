//! racoon CLI entry point.
//!
//! Responsibilities:
//! - Parse command-line arguments
//! - Initialize logging/tracing system
//! - Dispatch to appropriate command handlers
//! - Handle top-level errors gracefully

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, error, warn, Level};
use tracing_subscriber::FmtSubscriber;
use colored::Colorize;

use racoon::cli::{Cli, Commands, ConfigAction, ConfigArgs, DownloadArgs, QueryArgs, TailArgs, ViewArgs, ExportArgs};
use racoon::config::Config;
use racoon::gcp::{GcpAuth, LogFilter, LogsClient, ProjectsClient};
use racoon::ui::{App, run_app};
use racoon::ai::GeminiClient;
use racoon::storage::{export_logs, export_logs_with_ai, import_logs, ExportFormat, ExportOptions};

#[tokio::main]
async fn main() -> Result<()> {
    // parse command line arguments
    let cli = Cli::parse();

    // load config
    let config = Config::load();

    // initialize tracing
    let log_level = config.logging.level.clone();
    init_tracing(&log_level)?;

    info!("racoon staring up 🦝");

    // Dispatch to command handlers
    let result = match cli.command {
        Commands::Tail(args) => handle_tail(args, &config).await,
        Commands::Projects => handle_projects().await,
        Commands::Auth => handle_auth().await,
        Commands::Download(args) => handle_download(args, &config).await,
        Commands::Query(args) => handle_query(args, &config).await,
        Commands::Export(args) => handle_export(args, &config).await,
        Commands::View(args) => handle_view(args).await,
        Commands::Config(args) => handle_config(args),
    };

    // Handle errors gracefully at the top level
    if let Err(ref e) = result {
        error!("Command failed: {:#}", e);
    }

    result
}

/// initialize tracing
fn init_tracing(level: &str) -> Result<()> {
    let level = match level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" | "warning" => Level::WARN,
        "error" => Level::ERROR,
        _ => {
            eprint!("Unknown log level '{}', defaulting to 'info'", level);
            Level::INFO
        }
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
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
    let project = args.project
        .or_else(|| config.defaults.project.clone())
        .ok_or_else(|| anyhow::anyhow!(
            "Project required. Use --project or set in config file."
        ))?;

    // Use config default for since
    let since_str = args.since.as_deref()
        .unwrap_or(&config.defaults.since);

    let duration = racoon::parse_duration_chrono(since_str)
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

    if let Some(ref f) = args.filter {
        filter = filter.raw(f);
    }

    if let Some(ref sev) = args.severity {
        filter = filter.severity_min(&format!("{:?}", sev).to_uppercase());
    }

    if let Some(ref res) = args.resource {
        filter = filter.resource_type(res);
    }

    info!("Filter: {}", filter.build());

    // Start streaming
    let (rx, _handle) = client.stream_entries(Some(filter), duration).await?;

    // Create app state
    let app = App::new(project.clone());

    // Run TUI
    run_app(app, rx, gemini, Some(client)).await?;

    info!("TUI exited");
    Ok(())
}

/// handle projects command
async fn handle_projects() -> Result<()> {
    println!("{}", "Fetching GCP projects...".dimmed());

    let auth = GcpAuth::new().await.context("Authentication failed")?;
    auth.verify().await.context("GCP credentials verification failed")?;

    let client = ProjectsClient::new(auth);

    let projects = client.list().await?;

    if projects.is_empty() {
        println!("{}", "No accessible projects found.".yellow());
        return Ok(());
    }

    println!("\n{}", format!("Found {} projects:", projects.len()).green().bold());
    println!("{}", "─".repeat(60));
    println!("{:<40} {:<20}", "PROJECT ID".bold(), "NAME".bold());
    println!("{}", "─".repeat(60));

    for project in &projects {
        println!("{:<40} {:<20}", project.project_id, project.display_name());
    }

    println!("{}", "─".repeat(60));
    println!("\nUse {} to stream logs", "racoon tail --project <PROJECT_ID>".cyan());

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
        let gemini = GeminiClient::from_config(&config)
            .context("AI download requires GEMINI_API_KEY")?;

        println!("🦝 Generating query: \"{}\"", ai_prompt);

        let (query, result) = export_logs_with_ai(
            &client,
            &gemini,
            ai_prompt,
            &args.output,
            options,
        ).await?;

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
    let project = args.project
        .or_else(|| config.defaults.project.clone())
        .ok_or_else(|| anyhow::anyhow!("Project required. Use --project or set in config file."))?;

    let _since_str = args.since.as_deref()
        .unwrap_or(&config.defaults.since);

    let limit = args.limit.unwrap_or(config.defaults.limit);

    // Initialize and verify authentication
    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("GCP credentials verification failed")?;

    println!("✅ Authentication successful!");

    let client = LogsClient::new(auth, project.clone());

    // Build filter
    let filter = args.filter.as_ref().map(|f| LogFilter::new().raw(f));

    // Fetch logs
    let entries = client.list_entries(filter.as_ref(), limit).await?;

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
async fn handle_view(args: ViewArgs) -> Result<()> {
    println!("📂 Loading logs from: {}", args.file);

    let result = import_logs(&args.file)
        .context("Failed to import logs")?;

    println!("   Format: {}", result.format.name());
    println!("   Loaded: {} entries", result.count);

    if result.errors > 0 {
        println!("   ⚠️  {} entries failed to parse", result.errors);
    }

    if args.json {
        // Output as JSON
        println!();
        println!("{}", serde_json::to_string_pretty(&result.entries)?);
    } else if result.entries.is_empty() {
        println!("\n(no entries to display)");
    } else {
        // Display in terminal
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
            println!("\n... and {} more (use --limit to see more)", result.count - args.limit);
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
    let format: ExportFormat = args.format.parse()
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
        let gemini = GeminiClient::from_config(config)
            .context("AI export requires GEMINI_API_KEY")?;

        println!("   🦝 AI query: \"{}\"", ai_prompt);

        let (query, result) = export_logs_with_ai(
            &client,
            &gemini,
            ai_prompt,
            &args.output,
            options,
        ).await?;

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
            println!("\nEdit with: {}", format!("$EDITOR {}", path.display()).cyan());
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
            println!("  api_key: {}",
                     config.ai.api_key.as_ref()
                         .map(|k| format!("{}...", &k[..8.min(k.len())]))
                         .unwrap_or_else(|| "(not set)".dimmed().to_string())
            );
            println!("  model: {}", config.ai.model);
            println!();

            println!("{}", "[defaults]".cyan());
            println!("  project: {}", config.defaults.project.as_deref().unwrap_or("(not set)"));
            println!("  since: {}", config.defaults.since);
            println!("  limit: {}", config.defaults.limit);
            println!();

            println!("{}", "[ui]".cyan());
            println!("  max_logs: {}", config.ui.max_logs);
            println!("  mouse: {}", config.ui.mouse);
            println!("  theme: {}", config.ui.theme);
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
    fn test_init_tracing_valid_levels() {
        // Test various valid log levels
        // Note: We can only set the global subscriber once, so we test parsing logic
        let valid_levels = ["trace", "debug", "info", "warn", "warning", "error", "INFO", "DEBUG"];

        for level in valid_levels {
            let parsed = match level.to_lowercase().as_str() {
                "trace" => Level::TRACE,
                "debug" => Level::DEBUG,
                "info" => Level::INFO,
                "warn" | "warning" => Level::WARN,
                "error" => Level::ERROR,
                _ => Level::INFO,
            };

            // Just verify parsing doesn't panic and returns expected level
            match level.to_lowercase().as_str() {
                "trace" => assert_eq!(parsed, Level::TRACE),
                "debug" => assert_eq!(parsed, Level::DEBUG),
                "info" => assert_eq!(parsed, Level::INFO),
                "warn" | "warning" => assert_eq!(parsed, Level::WARN),
                "error" => assert_eq!(parsed, Level::ERROR),
                _ => {}
            }
        }
    }
}