pub mod pairing;
pub mod query;
pub mod setup;

pub use pairing::{DaemonPairingClient, DaemonPairingRequestError};
pub use query::DaemonQueryClient;
pub use setup::DaemonSetupClient;

use anyhow::{anyhow, Result};
use reqwest::header::AUTHORIZATION;
use reqwest::{Method, RequestBuilder};

use crate::DaemonConnectionState;

pub fn authorized_daemon_request(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    method: Method,
    path: &str,
) -> Result<RequestBuilder> {
    let connection = connection_state
        .get()
        .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
    let url = format!("{}{}", connection.base_url, path);

    Ok(http
        .request(method, url)
        .header(AUTHORIZATION, format!("Bearer {}", connection.token)))
}
