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

use racoon::cli::{Cli, Commands, DownloadArgs, QueryArgs, TailArgs};
use racoon::gcp::{GcpAuth, LogFilter, LogsClient};
use racoon::ui::{App, run_app};
use racoon::ai::GeminiClient;

#[tokio::main]
async fn main() -> Result<()> {
    // parse command line arguments
    let cli = Cli::parse();

    // initialize tracing
    init_tracing(&cli.log_level)?;

    info!("racoon staring up 🦝");

    // Dispatch to command handlers
    let result = match cli.command {
        Commands::Tail(args) => handle_tail(args).await,
        Commands::Projects => handle_projects().await,
        Commands::Auth => handle_auth().await,
        Commands::Download(args) => handle_download(args).await,
        Commands::Query(args) => handle_query(args).await,
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
async fn handle_tail(args: TailArgs) -> Result<()> {
    info!(
        project = %args.project,
        filter = ?args.filter,
        severity = ?args.severity,
        since = %args.since,
        "Starting log tail"
    );

    let duration = racoon::parse_duration_chrono(&args.since)
        .map_err(|e| anyhow::anyhow!("Invalid --since: {}", e))?;

    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("Failed to verify GCP authentication")?;

    println!("✅ Connected to GCP");

    // Try to initialize Gemini client
    let gemini = match GeminiClient::from_env() {
        Ok(client) => {
            info!("AI features enabled (Gemini)");
            Some(client)
        }
        Err(e) => {
            warn!("AI features disabled: {}", e);
            None
        }
    };

    let client = LogsClient::new(auth.clone(), args.project.clone());

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
    let app = App::new(args.project.clone());

    // Run TUI
    run_app(app, rx, gemini, Some(client)).await?;

    info!("TUI exited");
    Ok(())
}

/// handle projects command
async fn handle_projects() -> Result<()> {
    info!("Listing accessible GCP projects");

    // Initialize and verify authentication first
    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("Failed to verify GCP authentication")?;

    println!("✅ Authentication successful!");
    println!("   {}", auth.credential_info());
    println!();

    // TODO:
    // 2. call api

    println!("🚧 Project listing not yet implemented");
    println!("   This will list all GCP projects you have access to.");

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
async fn handle_download(args: DownloadArgs) -> Result<()> {
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

    // Check if AI-assisted download
    let filter = if let Some(ref ai_prompt) = args.ai {
        println!("🤖 Generating query from: \"{}\"", ai_prompt);

        let gemini = GeminiClient::from_env()
            .context("AI download requires GEMINI_API_KEY")?;

        let query = gemini.generate_query(ai_prompt).await?;
        println!("📋 Generated query: {}", query);

        Some(LogFilter::new().raw(&query))
    } else {
        args.filter.as_ref().map(|f| LogFilter::new().raw(f))
    };

    println!("📥 Downloading logs...");
    let entries = client.list_entries(filter.as_ref(), args.limit).await?;

    let file = std::fs::File::create(&args.output)?;
    serde_json::to_writer_pretty(std::io::BufWriter::new(file), &entries)?;

    println!("✅ Downloaded {} logs to {}", entries.len(), args.output);
    Ok(())
}

/// handle query command
async fn handle_query(args: QueryArgs) -> Result<()> {
    info!(
        project = %args.project,
        filter = ?args.filter,
        limit = args.limit,
        json = args.json,
        "Executing query"
    );

    // Initialize and verify authentication
    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("GCP credentials verification failed")?;

    println!("✅ Authentication successful!");

    let client = LogsClient::new(auth, args.project.clone());

    // Build filter
    let filter = args.filter.as_ref().map(|f| LogFilter::new().raw(f));

    // Fetch logs
    let entries = client.list_entries(filter.as_ref(), args.limit).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_valid_levels() {

    }
}