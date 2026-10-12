// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use crate::error::{Error, Result};

/// `User-Agent` sent with every request.
pub const USER_AGENT: &str = concat!("UnLauncher/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Time without receiving data before a request fails. Not a total timeout:
/// downloads of many GB must be allowed to take hours.
const READ_TIMEOUT: Duration = Duration::from_mins(1);

/// Builds the HTTP client every UnLauncher crate uses. Build it once and share
/// clones (they share the connection pool).
///
/// # Errors
///
/// [`Error::HttpClient`] if the TLS backend cannot be initialised.
pub fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(Error::HttpClient)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_has_version() {
        assert!(USER_AGENT.starts_with("UnLauncher/"));
        assert!(USER_AGENT.len() > "UnLauncher/".len());
    }

    #[test]
    fn client_builds() {
        assert!(http_client().is_ok());
    }
}
