pub mod api;
pub mod config;
mod db;
mod har;
pub mod proxy;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub resolver: hickory_resolver::TokioAsyncResolver,
}
