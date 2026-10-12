// SPDX-License-Identifier: GPL-3.0-or-later

//! Epic Games login and everything UnLauncher needs to talk to remote services.
//!
//! - [`http_client`]: the single shared `reqwest::Client` (timeouts and
//!   `User-Agent: UnLauncher/<version>`); `fab`, `ue-prebuilt` and `ue-build`
//!   receive it from here.
//! - [`SecretStore`], [`KeyringStore`]: tokens in the system keyring, never in
//!   files or logs.
//! - [`LOGIN_URL`], [`parse_authorization_code`], [`EpicSession`]: sign in
//!   through the system browser. The user opens [`LOGIN_URL`],
//!   signs in, pastes the authorization code Epic shows, and
//!   [`EpicSession::login`] exchanges it through `egs-api`.

mod code;
mod credentials;
mod error;
mod http;
mod session;

pub use code::{
    LOGIN_URL, WEB_EXCHANGE_URL, parse_authorization_code, provider_login_url, web_login_url,
};
pub use credentials::{CredentialKey, KeyringStore, MemoryStore, SecretStore};
pub use error::{Error, Result};
pub use http::{USER_AGENT, http_client};
pub use session::EpicSession;
