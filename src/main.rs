//! racoon CLI entry point
//!
//! the main tasks
//! - parse command line arguments
//! - initialize logging
//! - run the main loop

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn, Level};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::FmtSubscriber;

use racoon::cli::{Cli, Commands, DownloadArgs, QueryArgs, TailArgs};

#[tokio::main]
async fn main() -> Result<()> {
    // parse command line arguments
    let cli = Cli::parse();

    // initialize tracing
    init_tracing(&cli.log_level)?;

    info!("racoon staring up 🦝");

    // run the main loop
    match cli.command {
        Commands::Tail(args) => {
            handle_tail(args).await?;
        }
        Commands::Projects => {
            handle_projects().await?;
        }
        Commands::Auth => {
            handle_auth().await?;
        }
        Commands::Download(args) => {
            handle_download(args).await?;
        }
        Commands::Query(args) => {
            handle_query(args).await?;
        }
    }

    Ok(())
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
        since = ?args.since,
        "Starting log tail"
    );

    // parse time range
    let since_seconds = racoon::parse_duration(&args.since)
        .map_err(|e| anyhow::anyhow!("Invalid -- since value: {}", e))?;

    info!("Looking back {} seconds", since_seconds);

    // TODO:
    // 1. gcp auth
    // 2. log client
    // 3. start GUI

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

    // TODO:
    // 1. gcp auth
    // 2. call api

    println!("🚧 Project listing not yet implemented");
    println!("   This will list all GCP projects you have access to.");

    Ok(())
}

/// handle auth command
async fn handle_auth() -> Result<()> {
    info!("Verifying GCP authentication");

    // TODO:
    // 1. get default credentials
    // 2. show auth status

    println!("🚧 Auth verification not yet implemented");
    println!("   Checking for:");
    println!("   - Application Default Credentials (gcloud auth application-default login)");
    println!("   - GOOGLE_APPLICATION_CREDENTIALS environment variable");

    match std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        Ok(path) => {
            println!("\n✅ GOOGLE_APPLICATION_CREDENTIALS is set: {}", path);
        }
        Err(_) => {
            println!("\n⚠️  GOOGLE_APPLICATION_CREDENTIALS is not set");
            println!("   Will try Application Default Credentials...");
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

    // TODO:
    // 1. gcp auth
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