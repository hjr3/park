use std::net::SocketAddr;

use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
pub struct Config {
    pub database: Database,
    pub server: Server,
}

#[derive(Deserialize)]
pub struct Database {
    /// Database URI
    ///
    /// Currently supported:
    /// - sqlite
    pub uri: String,

    /// Maximum size of the database in bytes
    ///
    /// Defaults to 10MiB
    #[serde(default = "default_max_size")]
    pub max_size: u64,
}

const fn default_max_size() -> u64 {
    10 * 1024 * 1024
}

#[derive(Deserialize)]
pub struct Server {
    /// The address of the upstream/backend server to proxy requests to
    ///
    /// Options:
    /// - IP address and port
    /// - URL
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Url,

    /// listen for requests on a given IP address and port. Defaults to 127.0.0.1:3000
    #[serde(default = "default_bind")]
    pub bind: SocketAddr,

    /// The maximum number of connections to allow. Defaults to 10
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// The timeout in ms for the downstream client to send a request. Defaults to 10 seconds
    #[serde(default = "default_client_timeout")]
    pub client_timeout: usize,

    /// The timeout for the upstream server to respond to a request. Defaults to 10 seconds
    #[serde(default = "default_server_timeout")]
    pub server_timeout: u64,

    /// The path to the SSL certificate pem file
    ///
    /// Required if the address is an https address
    pub ssl_cert: Option<String>,
}

fn deserialize_address<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let url = Url::deserialize(deserializer)?;

    if !url.has_host() {
        return Err(serde::de::Error::custom("missing host"));
    }

    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(serde::de::Error::custom("scheme must be http or https")),
    }
}

fn default_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 3000))
}

const fn default_max_connections() -> usize {
    10
}

const fn default_client_timeout() -> usize {
    10
}

const fn default_server_timeout() -> u64 {
    10
}
