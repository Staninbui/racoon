//! racoon CLI entry point.
//!
//! Responsibilities:
//! - Parse command-line arguments
//! - Initialize logging/tracing system
//! - Dispatch to appropriate command handlers
//! - Handle top-level errors gracefully

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

use racoon::cli::{Cli, Commands, DownloadArgs, QueryArgs, TailArgs};
use racoon::gcp::GcpAuth;

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

    // parse time range
    let since_seconds = racoon::parse_duration(&args.since)
        .map_err(|e| anyhow::anyhow!("Invalid -- since value: {}", e))?;

    let auth = GcpAuth::new()
        .await
        .context("Failed to initialize GCP authentication")?;

    auth.verify()
        .await
        .context("Failed to verify GCP authentication")?;

    info!("Authentication successful: {}", auth.credential_info());

    // TODO:
    // 2. log client
    // 3. start GUI

    println!("✅ Authentication successful!");
    println!("   {}", auth.credential_info());
    println!();
    println!("🚧 TUI mode not yet implemented");
    println!("   Project: {}", args.project);
    println!("   Filter: {:?}", args.filter);
    println!("   Severity: {:?}", args.severity);
    println!("   Since: {} ({} seconds)", args.since, since_seconds);

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

    // TODO:
    // 1. --ai arg exists, call AI to generate query
    // 2. query and download


    if let Some(ref ai_prompt) = args.ai {
        println!("   AI-assisted download requested");
        println!("   Prompt: \"{}\"", ai_prompt);
        println!("   Will generate GCP query and download matching logs");
    }

    println!("🚧 Download not yet implemented");
    println!("   Project: {}", args.project);
    println!("   Output: {}", args.output);
    println!("   Limit: {} logs", args.limit);

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

    // TODO:
    // 2. query
    // 3. output, support --json

    println!("🚧 Query not yet implemented");
    println!("   Project: {}", args.project);
    println!("   Filter: {:?}", args.filter);
    println!("   Limit: {} logs", args.limit);
    println!("   JSON output: {}", args.json);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_valid_levels() {

    }
}