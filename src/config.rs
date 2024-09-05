use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
pub struct Config {
    pub database: Database,
    pub server: Server,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: Database {
                url: "sqlite::memory:".to_string(),
            },
            server: Server {
                address: "http://127.0.0.1"
                    .parse()
                    .expect("default address is valid"),
            },
        }
    }
}

#[derive(Deserialize)]
pub struct Database {
    pub url: String,
}

#[derive(Deserialize)]
pub struct Server {
    pub address: Url,
}
