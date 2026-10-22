//! Real startup HTTP service fixture; no Engine, wallet, clipboard or GUI is opened.
use uc_engine::StartupProgress;
use uc_webserver::api::{auth::load_or_create_auth_token_from_conn, startup::StartupServer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let root = std::env::var_os("UC_STARTUP_E2E_ROOT")
        .ok_or_else(|| anyhow::anyhow!("UC_STARTUP_E2E_ROOT is required"))?;
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir_all(&root)?;
    let path = root.join("daemon-startup.conn");
    let token = load_or_create_auth_token_from_conn(&path)?;
    let (input, progress) = StartupProgress::channel();
    let server = StartupServer::bind(progress, token, path).await?;
    // The public API safely models an interrupted startup by dropping its input.
    if std::env::var("STARTUP_FIXTURE").as_deref() != Ok("still-starting") {
        drop(input);
    }
    tokio::select! {
        () = server.retry_requested() => {}
        () = tokio::time::sleep(std::time::Duration::from_secs(10)) => { anyhow::bail!("fixture deadline expired"); }
    }
    server.shutdown().await
}
