use std::net::SocketAddr;
use std::sync::Arc;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use park::config::Config;
use park::config::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "park=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config {
        database: park::config::Database {
            url: "sqlite::memory:".to_string(),
        },
        server: Server {
            addr: "127.0.0.1:8080".parse().unwrap(),
        },
    };
    let config = Arc::new(config);
    let state = park::AppState {
        db: sqlx::SqlitePool::connect(&config.database.url).await?,
    };

    let mut conn = state.db.acquire().await?;
    sqlx::migrate!().run(&mut conn).await?;

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    tracing::info!("Listening on {}", addr);
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (stream, _) = listener.accept().await?;

        let io = TokioIo::new(stream);

        let config = config.clone();
        let state = state.clone();
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(
                    io,
                    service_fn(move |req| park::proxy(config.clone(), state.clone(), req)),
                )
                .with_upgrades()
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
