//! The `glovebox-web` binary: serves the web page on `127.0.0.1` only.
//! Diagnostics go to stderr.

use std::{net::Ipv4Addr, path::PathBuf};

use glovebox_web::{AppState, router};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let mut open_browser = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--open" => open_browser = true,
            other => {
                return Err(
                    format!("unknown argument {other:?}; the only option is --open").into(),
                );
            }
        }
    }

    let environment = glovebox_shared::env::resolve(|k| std::env::var(k).ok())?;
    let db = glovebox_shared::db::connect(&environment.db_path).await?;
    glovebox_shared::backup::spawn_daily(db.clone(), PathBuf::from(&environment.db_path));

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, environment.web_port))
        .await
        .map_err(|e| format!("cannot listen on 127.0.0.1:{}: {e}", environment.web_port))?;
    let url = format!("http://127.0.0.1:{}/", environment.web_port);
    eprintln!("glovebox ({}) is at {url}", environment.name);
    if open_browser {
        open_in_browser(&url);
    }

    let state = AppState {
        db,
        env_name: environment.name,
        today: glovebox_shared::services::today,
    };
    axum::serve(listener, router(state)).await?;
    Ok(())
}

// Asks the system to open `url`. A failure only costs the owner a click, so it
// is logged and the server keeps running.
fn open_in_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
        tracing::warn!(error = %e, "could not open the browser");
    }
}
