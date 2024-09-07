pub mod api;
pub mod config;
pub mod db;
mod har;
pub mod proxy;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub client: reqwest::Client,
}
