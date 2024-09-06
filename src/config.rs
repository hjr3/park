use std::net::SocketAddr;

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
                bind: None,
                max_connections: None,
                client_timeout: None,
                server_timeout: None,
                ssl_cert: None,
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
    /// The address of the upstream/backend server to proxy requests to
    ///
    /// Options:
    /// - IP address and port
    /// - URL
    pub address: Url,

    /// listen for requests on a given IP address and port. Defaults to 127.0.0.1:3000
    pub bind: Option<SocketAddr>,

    /// The maximum number of connections to allow. Defaults to 10
    pub max_connections: Option<usize>,

    /// The timeout for the downstream client to send a request. Defaults to 10 seconds
    pub client_timeout: Option<u64>,

    /// The timeout for the upstream server to respond to a request. Defaults to 10 seconds
    pub server_timeout: Option<u64>,

    /// The path to the SSL certificate pem file
    ///
    /// Required if the address is an https address
    pub ssl_cert: Option<String>,
}
