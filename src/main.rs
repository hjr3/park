use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Arg, Command};
use futures_util::future::join;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use park::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let matches = Command::new("park")
        .about("Generate har files for proxied requests")
        .arg(
            Arg::new("address")
                .help("The URL or socket to send requests to. Example: http://example.com or 127.0.0.1:8080")
                .index(1),
        )
        .arg(
            Arg::new("bind")
                .help("The port or socket to bind to. If IP address is not specified, then 127.0.0.1 is used. Example: 8080 or 127.0.0.1:8080")
                .index(2)
        )
        .arg(
            Arg::new("config")
                .short('f')
                .long("config")
                .help("Path to the configuration file")
                .value_name("FILE")
                .conflicts_with("address"),
        )
        .get_matches();

    let config: Config = if let Some(address) = matches.get_one::<String>("address") {
        let mut config = Config::default();

        let address = if let Ok(socket) = address.parse::<SocketAddr>() {
            url::Url::parse(&format!("http://{}", socket))?
        } else if let Ok(url) = url::Url::parse(address) {
            url
        } else {
            eprintln!("Invalid address: {}", address);
            std::process::exit(1);
        };
        // TODO make sure host and port are valid
        config.server.address = address;
        config
    } else if let Some(config_file) = matches.get_one::<String>("config") {
        let content = std::fs::read_to_string(config_file)?;
        toml::from_str(&content)?
    } else {
        eprintln!("You must specify either a domain or a configuration file.");
        std::process::exit(1);
    };

    // TODO support full IP addresses
    let port = if let Some(bind) = matches.get_one::<String>("bind") {
        bind.parse::<u16>().unwrap_or(0)
    } else {
        0
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "park=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Arc::new(config);
    let resolver = hickory_resolver::TokioAsyncResolver::tokio_from_system_conf()?;

    let state = park::AppState {
        db: sqlx::SqlitePool::connect(&config.database.url).await?,
        resolver,
    };

    let mut conn = state.db.acquire().await?;
    sqlx::migrate!().run(&mut conn).await?;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let proxy_config = config.clone();
    let proxy_state = state.clone();
    let proxy_srv = async move {
        let listener = TcpListener::bind(addr).await.expect("Proxy failed to bind");
        tracing::info!(
            "Proxy listening on {}",
            listener
                .local_addr()
                .expect("Proxy failed to get local address")
        );
        loop {
            let (stream, _) = listener
                .accept()
                .await
                .expect("Proxy failed to accept connection");

            let io = TokioIo::new(stream);

            let config = proxy_config.clone();
            let state = proxy_state.clone();
            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                    .serve_connection(
                        io,
                        service_fn(move |req| {
                            park::proxy::proxy(config.clone(), state.clone(), req)
                        }),
                    )
                    .with_upgrades()
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    };

    let api_config = config.clone();
    let api_state = state.clone();
    let api_srv = async move {
        let listener = TcpListener::bind("127.0.0.1:9000")
            .await
            .expect("API failed to bind");
        tracing::info!(
            "API listening on {}",
            listener
                .local_addr()
                .expect("API failed to get local address")
        );
        loop {
            let (stream, _) = listener
                .accept()
                .await
                .expect("API failed to accept connection");

            let io = TokioIo::new(stream);

            let config = api_config.clone();
            let state = api_state.clone();
            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |req| park::api::api(config.clone(), state.clone(), req)),
                    )
                    .with_upgrades()
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    };

    let _ret = join(proxy_srv, api_srv).await;

    Ok(())
}
