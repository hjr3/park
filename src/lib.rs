use anyhow::Result;

mod api;
mod config;
mod db;
mod har;
mod proxy;

pub use api::api;
pub use config::Config;
pub use proxy::proxy;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub client: reqwest::Client,
    pub har_queue: tokio::sync::mpsc::Sender<crate::har::Har>,
}

pub async fn app(config: &config::Config) -> Result<AppState> {
    let client = reqwest::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(config.server.server_timeout))
        .build()?;
    let db = crate::db::init_db(&config.database).await?;
    let har_queue = crate::har::writer::queue(db.clone()).await;

    let state = crate::AppState {
        db,
        client,
        har_queue,
    };

    Ok(state)
}
