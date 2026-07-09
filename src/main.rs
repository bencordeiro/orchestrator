//! Thin binary entry point for the headless orchestrator MCP server.

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use orchestrator::config::{write_example_if_missing, LoadedConfig};
use orchestrator::conversation::ConversationStore;
use orchestrator::core::Orchestrator;
use orchestrator::mcp::server::{build_router, load_bearer_token, McpState};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::{KeyringSecretStore, SecretStore};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator=info,rmcp=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        args.push("serve".into());
    }

    match args[0].as_str() {
        "serve" => cmd_serve(args.get(1).map(PathBuf::from)).await,
        "secrets" => cmd_secrets(&args[1..]),
        "init" => cmd_init(args.get(1).map(PathBuf::from)),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            bail!("unknown command");
        }
    }
}

fn print_help() {
    eprintln!(
        r#"orchestrator — slot-based model delegation MCP server

Usage:
  orchestrator serve [slots.json]   Start MCP server (default config: ./slots.json)
  orchestrator init [slots.json]    Write example slots.json if missing
  orchestrator secrets set <name> <value>
  orchestrator secrets get <name>
  orchestrator secrets delete <name>

Environment:
  ORCHESTRATOR_SLOTS   Path to slots.json (overrides default)
  RUST_LOG             Log filter (default: orchestrator=info)

MCP clients:
  claude mcp add --transport http orchestrator http://127.0.0.1:7420/mcp \
    --header "Authorization: Bearer <token>"
"#
    );
}

fn default_slots_path() -> PathBuf {
    env::var_os("ORCHESTRATOR_SLOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("slots.json"))
}

fn cmd_init(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(default_slots_path);
    write_example_if_missing(&path)?;
    println!("wrote example config (if missing): {}", path.display());
    println!("set secrets next:");
    println!("  orchestrator secrets set mcp_bearer_token <token>");
    println!("  orchestrator secrets set worker_api_key <api-key>");
    Ok(())
}

fn cmd_secrets(args: &[String]) -> Result<()> {
    let store = KeyringSecretStore;
    match args {
        [cmd, name, value] if cmd == "set" => {
            store.set(name, value)?;
            println!("set secret '{name}' in OS keychain (service={})", orchestrator::KEYRING_SERVICE);
            Ok(())
        }
        [cmd, name] if cmd == "get" => {
            match store.get(name)? {
                Some(v) => {
                    // Avoid dumping full secrets to shell history logs if redirected;
                    // still print for admin use.
                    println!("{v}");
                }
                None => bail!("secret '{name}' not found"),
            }
            Ok(())
        }
        [cmd, name] if cmd == "delete" => {
            store.delete(name)?;
            println!("deleted secret '{name}'");
            Ok(())
        }
        _ => {
            eprintln!("usage: orchestrator secrets set|get|delete <name> [value]");
            bail!("bad secrets usage");
        }
    }
}

async fn cmd_serve(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(default_slots_path);
    if !path.exists() {
        write_example_if_missing(&path)?;
        eprintln!(
            "created example {}; edit it, set secrets, then re-run serve",
            path.display()
        );
    }

    let loaded = LoadedConfig::load(&path)
        .with_context(|| format!("load config {}", path.display()))?;
    let listen = loaded.file.listen.clone();
    let bearer_ref = loaded.file.bearer_token_ref.clone();
    let conv_dir = loaded.conversations_path();

    let secrets: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore);
    let bearer = match load_bearer_token(secrets.as_ref(), &bearer_ref) {
        Ok(t) => t,
        Err(e) => {
            // Dev convenience: allow ORCHESTRATOR_BEARER_TOKEN env override.
            if let Ok(t) = env::var("ORCHESTRATOR_BEARER_TOKEN") {
                tracing::warn!("using ORCHESTRATOR_BEARER_TOKEN env (keychain ref missing)");
                t
            } else {
                return Err(e.into());
            }
        }
    };

    // Optional: seed API keys from env for first-run convenience (never written to config).
    if let Ok(key) = env::var("ORCHESTRATOR_WORKER_API_KEY") {
        if secrets.get("worker_api_key")?.is_none() {
            tracing::info!("seeding worker_api_key into keychain from ORCHESTRATOR_WORKER_API_KEY");
            secrets.set("worker_api_key", &key)?;
        }
    }

    let registry = Arc::new(SlotRegistry::open(&path)?);
    let conversations = Arc::new(ConversationStore::new(conv_dir)?);
    let orchestrator = Orchestrator::new(registry, conversations, secrets);

    let state = McpState {
        orchestrator,
        bearer_token: Arc::new(bearer),
    };

    tracing::info!("config: {}", path.display());
    tracing::info!("MCP endpoint: http://{listen}/mcp");
    tracing::info!("health:       http://{listen}/health");

    let addr: std::net::SocketAddr = listen.parse()?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown");
        })
        .await?;
    Ok(())
}
