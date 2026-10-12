// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use common::Secret;
use egs_api::EpicGames;
use egs_api::api::types::account::UserData;

use crate::credentials::{CredentialKey, SecretStore};
use crate::error::{Error, Result, from_egs};
use crate::http::http_client;

/// Epic's account service (used to end the session on logout).
const OAUTH_HOST: &str = "https://account-public-service-prod03.ol.epicgames.com";

/// A signed-in Epic Games session.
///
/// Cheap to clone; clones share the same tokens. The refresh token is kept in
/// the [`SecretStore`] under [`CredentialKey::EpicRefreshToken`] and replaced
/// every time Epic rotates it. `Debug` never prints tokens.
#[derive(Clone)]
pub struct EpicSession {
    inner: Arc<Inner>,
}

struct Inner {
    /// The `egs-api` client with the current tokens. Locked only to clone or
    /// replace it, never across an `.await`.
    egs: Mutex<EpicGames>,
    /// Serialises refreshes so two tasks do not rotate the token at once.
    refresh: tokio::sync::Mutex<()>,
    store: Arc<dyn SecretStore>,
    http: reqwest::Client,
    account_id: String,
    display_name: String,
    oauth_host: String,
}

impl fmt::Debug for EpicSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EpicSession")
            .field("account_id", &self.inner.account_id)
            .field("display_name", &self.inner.display_name)
            .finish_non_exhaustive()
    }
}

impl EpicSession {
    /// Signs in with an authorization code (see [`crate::LOGIN_URL`] and
    /// [`crate::parse_authorization_code`]) and stores the refresh token.
    ///
    /// # Errors
    ///
    /// [`Error::LoginRejected`] if Epic rejects the code (expired or already
    /// used), [`Error::Network`] / [`Error::EpicUnavailable`] if Epic cannot be
    /// reached, [`Error::ApiChanged`] if the answer lacks the expected fields,
    /// [`Error::Keyring`] if the token cannot be stored.
    pub async fn login(code: &Secret, store: Arc<dyn SecretStore>) -> Result<Self> {
        let mut egs = EpicGames::new();
        let accepted = egs
            .try_auth_code(None, Some(code.expose().to_owned()))
            .await
            .map_err(|e| from_egs("sign-in", e))?;
        if !accepted {
            return Err(Error::LoginRejected);
        }
        let session = Self::from_egs(egs, store, OAUTH_HOST)?;
        session.save_refresh_token().await?;
        tracing::info!(account_id = %session.inner.account_id, "signed in to Epic");
        Ok(session)
    }

    /// Restores the session from the stored refresh token.
    ///
    /// Returns `Ok(None)` if no token is stored, or if Epic rejects it (the
    /// stale token is then deleted and the user has to sign in again).
    ///
    /// # Errors
    ///
    /// [`Error::Keyring`], [`Error::Network`], [`Error::EpicUnavailable`] or
    /// [`Error::ApiChanged`]; the stored token is kept in those cases.
    pub async fn restore(store: Arc<dyn SecretStore>) -> Result<Option<Self>> {
        let reader = Arc::clone(&store);
        let Some(token) = blocking(move || reader.get(CredentialKey::EpicRefreshToken)).await?
        else {
            return Ok(None);
        };
        let mut egs = EpicGames::new();
        let mut user = UserData::new();
        user.set_refresh_token(Some(token.expose().to_owned()));
        egs.set_user_details(user);
        match egs.try_auth_code(None, None).await {
            Ok(true) => {}
            Ok(false) | Err(egs_api::api::error::EpicAPIError::InvalidCredentials) => {
                return forget(store).await.map(|()| None);
            }
            Err(error) => match from_egs("session refresh", error) {
                Error::LoginRejected => return forget(store).await.map(|()| None),
                other => return Err(other),
            },
        }
        let session = Self::from_egs(egs, store, OAUTH_HOST)?;
        session.save_refresh_token().await?;
        tracing::info!(account_id = %session.inner.account_id, "Epic session restored");
        Ok(Some(session))
    }

    /// Ends the session at Epic (best effort) and deletes the stored refresh
    /// token.
    ///
    /// # Errors
    ///
    /// [`Error::Keyring`] if the stored token cannot be deleted. Failing to
    /// reach Epic is only logged.
    pub async fn logout(self) -> Result<()> {
        if let Some(access) = self.egs().user_details().access_token().map(Secret::new) {
            let url = format!(
                "{}/account/api/oauth/sessions/kill/{}",
                self.inner.oauth_host,
                access.expose()
            );
            let result = self
                .inner
                .http
                .delete(url)
                .bearer_auth(access.expose())
                .send()
                .await;
            match result {
                Ok(response) if response.status().is_success() => {
                    tracing::info!("Epic session ended");
                }
                Ok(response) => {
                    tracing::warn!(status = %response.status(), "Epic refused to end the session");
                }
                Err(error) => {
                    // The URL holds the token: never log it.
                    tracing::warn!(error = %error.without_url(), "cannot end the Epic session");
                }
            }
        }
        forget(Arc::clone(&self.inner.store)).await
    }

    /// Epic account id.
    #[must_use]
    pub fn account_id(&self) -> &str {
        &self.inner.account_id
    }

    /// Display name of the account.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.inner.display_name
    }

    /// A snapshot of the authenticated `egs-api` client, for `fab` and
    /// `ue-prebuilt`.
    ///
    /// It is a clone (cheap: the HTTP pool is shared) because some `egs-api`
    /// calls need `&mut`. Call [`EpicSession::ensure_fresh`] first so its
    /// access token is valid. Never `Debug`-print it: it contains tokens.
    #[must_use]
    pub fn egs(&self) -> EpicGames {
        self.lock().clone()
    }

    /// A one-time exchange code that signs the built-in web window in to
    /// Epic's websites with this session (see [`crate::web_login_url`]). This
    /// is how a session started in the system browser (Google, Apple…) also
    /// works in the window.
    ///
    /// # Errors
    ///
    /// As [`EpicSession::ensure_fresh`], and [`Error::Network`],
    /// [`Error::EpicUnavailable`] or [`Error::ApiChanged`] if Epic does not
    /// give the code.
    pub async fn web_exchange_code(&self) -> Result<Secret> {
        self.ensure_fresh().await?;
        let mut egs = self.egs();
        let token = egs
            .try_game_token()
            .await
            .map_err(|e| from_egs("exchange code", e))?;
        Ok(Secret::new(token.code))
    }

    /// The shared UnLauncher HTTP client (see [`crate::http_client`]), for
    /// CDN downloads that need no Epic token.
    #[must_use]
    pub fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    /// Refreshes the tokens if the access token expires in less than ten
    /// minutes, and stores the rotated refresh token.
    ///
    /// # Errors
    ///
    /// [`Error::SessionExpired`] if Epic rejects the refresh token (it is
    /// deleted: sign in again), [`Error::Network`], [`Error::EpicUnavailable`],
    /// [`Error::ApiChanged`] or [`Error::Keyring`].
    pub async fn ensure_fresh(&self) -> Result<()> {
        let _refreshing = self.inner.refresh.lock().await;
        let mut egs = self.egs();
        if egs.is_logged_in() {
            return Ok(());
        }
        tracing::debug!("refreshing the Epic session");
        match egs.try_auth_code(None, None).await {
            Ok(true) => {}
            Ok(false) | Err(egs_api::api::error::EpicAPIError::InvalidCredentials) => {
                forget(Arc::clone(&self.inner.store)).await?;
                return Err(Error::SessionExpired);
            }
            Err(error) => {
                return Err(match from_egs("session refresh", error) {
                    Error::LoginRejected => {
                        forget(Arc::clone(&self.inner.store)).await?;
                        Error::SessionExpired
                    }
                    other => other,
                });
            }
        }
        *self.lock() = egs;
        self.save_refresh_token().await
    }

    fn from_egs(egs: EpicGames, store: Arc<dyn SecretStore>, oauth_host: &str) -> Result<Self> {
        let user = egs.user_details();
        if user.access_token().is_none() || user.refresh_token().is_none() {
            return Err(Error::ApiChanged {
                what: "sign-in answer without tokens".to_owned(),
            });
        }
        let account_id = user.account_id.clone().ok_or_else(|| Error::ApiChanged {
            what: "sign-in answer without account id".to_owned(),
        })?;
        Ok(Self {
            inner: Arc::new(Inner {
                egs: Mutex::new(egs),
                refresh: tokio::sync::Mutex::new(()),
                store,
                http: http_client()?,
                display_name: user.display_name.unwrap_or_else(|| account_id.clone()),
                account_id,
                oauth_host: oauth_host.to_owned(),
            }),
        })
    }

    async fn save_refresh_token(&self) -> Result<()> {
        let Some(token) = self.egs().user_details().refresh_token().map(Secret::new) else {
            return Ok(());
        };
        let store = Arc::clone(&self.inner.store);
        blocking(move || store.set(CredentialKey::EpicRefreshToken, &token)).await
    }

    fn lock(&self) -> MutexGuard<'_, EpicGames> {
        // Nothing can panic while the lock is held (clone or assignment).
        self.inner
            .egs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Deletes the stored refresh token.
async fn forget(store: Arc<dyn SecretStore>) -> Result<()> {
    tracing::info!("forgetting the stored Epic session");
    blocking(move || store.delete(CredentialKey::EpicRefreshToken)).await
}

/// Runs a blocking keyring call off the async threads.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|error| Error::Keyring {
            message: format!("keyring task failed: {error}"),
        })?
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::credentials::MemoryStore;

    fn fake_session(store: Arc<dyn SecretStore>, oauth_host: &str) -> EpicSession {
        let mut egs = EpicGames::new();
        let mut user = UserData::new();
        user.set_access_token(Some("access-abc".to_owned()));
        user.set_refresh_token(Some("refresh-xyz".to_owned()));
        user.account_id = Some("acc-1".to_owned());
        user.display_name = Some("Tester".to_owned());
        egs.set_user_details(user);
        EpicSession::from_egs(egs, store, oauth_host).unwrap()
    }

    #[tokio::test]
    async fn restore_without_token_is_none() {
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        assert!(EpicSession::restore(store).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn saves_and_exposes_account() {
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let session = fake_session(Arc::clone(&store), "http://127.0.0.1:9");
        session.save_refresh_token().await.unwrap();
        assert_eq!(session.account_id(), "acc-1");
        assert_eq!(session.display_name(), "Tester");
        let stored = store.get(CredentialKey::EpicRefreshToken).unwrap().unwrap();
        assert_eq!(stored.expose(), "refresh-xyz");
    }

    #[tokio::test]
    async fn debug_hides_tokens() {
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let shown = format!("{:?}", fake_session(store, "http://127.0.0.1:9"));
        assert!(!shown.contains("access-abc"));
        assert!(!shown.contains("refresh-xyz"));
        assert!(shown.contains("acc-1"));
    }

    #[tokio::test]
    async fn logout_kills_session_and_forgets_token() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/account/api/oauth/sessions/kill/access-abc"))
            .and(header("authorization", "Bearer access-abc"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let session = fake_session(Arc::clone(&store), &server.uri());
        session.save_refresh_token().await.unwrap();
        session.logout().await.unwrap();
        assert!(
            store
                .get(CredentialKey::EpicRefreshToken)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn logout_forgets_token_even_if_epic_fails() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let session = fake_session(Arc::clone(&store), &server.uri());
        session.save_refresh_token().await.unwrap();
        session.logout().await.unwrap();
        assert!(
            store
                .get(CredentialKey::EpicRefreshToken)
                .unwrap()
                .is_none()
        );
    }
}
