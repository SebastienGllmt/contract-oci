//! CLI tool for serving WASM components via OCI registry protocol.

use wasm_oci_serve::{oci, registry, server};

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};

use crate::registry::Registry;
use crate::server::AppState;

/// Serve WASM components via OCI registry protocol.
#[derive(Parser, Debug)]
#[command(name = "wasm-oci-serve")]
#[command(about = "Serve WASM components via OCI registry protocol")]
struct Args {
    /// Path to a .wasm file or directory containing .wasm files
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Port to listen on
    #[arg(short, long, default_value = "5000")]
    port: u16,

    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    // Load all WASM files
    let mut registry = Registry::new();
    let mut loaded_count = 0;

    for path in &args.paths {
        if path.is_dir() {
            // Load all .wasm files from directory
            let entries = std::fs::read_dir(path)
                .with_context(|| format!("Failed to read directory: {}", path.display()))?;

            for entry in entries {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.extension().map_or(false, |ext| ext == "wasm") {
                    match load_wasm_file(&entry_path) {
                        Ok(entry) => {
                            info!(
                                "Loaded: {} ({})",
                                entry.reference(),
                                entry_path.display()
                            );
                            registry.register(entry);
                            loaded_count += 1;
                        }
                        Err(e) => {
                            warn!("Failed to load {}: {:?}", entry_path.display(), e);
                        }
                    }
                }
            }
        } else if path.extension().map_or(false, |ext| ext == "wasm") {
            match load_wasm_file(path) {
                Ok(entry) => {
                    info!("Loaded: {} ({})", entry.reference(), path.display());
                    registry.register(entry);
                    loaded_count += 1;
                }
                Err(e) => {
                    warn!("Failed to load {}: {:?}", path.display(), e);
                }
            }
        } else {
            warn!("Skipping non-.wasm file: {}", path.display());
        }
    }

    if loaded_count == 0 {
        anyhow::bail!("No WASM components were loaded");
    }

    info!("Loaded {} component(s)", loaded_count);

    // Print available references
    println!("\nAvailable components:");
    for entry in registry.entries() {
        println!(
            "  localhost:{}/{}/{}:{}",
            args.port, entry.namespace, entry.name, entry.version
        );
    }
    println!();

    // Create server
    let state = Arc::new(AppState { registry });
    let app = server::create_router(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .context("Invalid address")?;

    info!("Listening on http://{}", addr);
    println!("Server running at http://{}", addr);
    println!("Use Ctrl+C to stop\n");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Load a WASM file and create a ComponentEntry.
fn load_wasm_file(path: &PathBuf) -> Result<registry::ComponentEntry> {
    let wasm_bytes =
        std::fs::read(path).with_context(|| format!("Failed to read file: {}", path.display()))?;

    // Extract filename without extension as fallback name
    let filename_hint = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());

    oci::load_component(wasm_bytes, filename_hint.as_deref())
        .with_context(|| format!("Failed to process component: {}", path.display()))
}
