//! ponoma-http — serves the typed servant-rs ponoma API (see `ponoma_server::http`) over
//! servant's own hyper serving loop (`serve_listener`). The SPA talks to this for the real
//! book of record. Run: `cargo run -p ponoma-server --bin ponoma-http`
//! Env: PONOMA_DB (default sqlite://ponoma.db?mode=rwc), PONOMA_ADDR (default 127.0.0.1:5200).

use ponoma_server::{bootstrap, http};
use servant_server::adapter::serve_listener;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("PONOMA_DB").unwrap_or_else(|_| "sqlite://ponoma.db?mode=rwc".into());
    let addr = std::env::var("PONOMA_ADDR").unwrap_or_else(|_| "127.0.0.1:5200".into());

    let db = bootstrap(&url).await?;
    eprintln!("ponoma-http: db={url}");
    eprintln!("ponoma-http: routes:");
    for r in http::ROUTES {
        eprintln!("  {r}");
    }

    let service = http::router(db);
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("ponoma-http: listening on http://{addr}");
    serve_listener(listener, service).await?;
    Ok(())
}
