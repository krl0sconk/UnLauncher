// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::sync::Mutex;

use common::Secret;

use crate::error::{Error, Result};

/// Keyring service name for every UnLauncher secret.
const SERVICE: &str = "io.github.krl0sconk.UnLauncher";

/// A secret UnLauncher stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialKey {
    /// GitHub token used to clone `EpicGames/UnrealEngine`.
    GitHubToken,
    /// Epic Games refresh token.
    EpicRefreshToken,
}

impl CredentialKey {
    fn account(self) -> &'static str {
        match self {
            Self::GitHubToken => "github-token",
            Self::EpicRefreshToken => "epic-refresh-token",
        }
    }
}

/// Where secrets are kept. [`KeyringStore`] in the app, [`MemoryStore`] in
/// tests.
pub trait SecretStore: Send + Sync {
    /// Reads a secret; `Ok(None)` if it was never stored.
    ///
    /// # Errors
    ///
    /// [`Error::Keyring`] if the store cannot be used.
    fn get(&self, key: CredentialKey) -> Result<Option<Secret>>;

    /// Stores or replaces a secret.
    ///
    /// # Errors
    ///
    /// [`Error::Keyring`] if the store cannot be used.
    fn set(&self, key: CredentialKey, secret: &Secret) -> Result<()>;

    /// Deletes a secret; deleting a missing one is not an error.
    ///
    /// # Errors
    ///
    /// [`Error::Keyring`] if the store cannot be used.
    fn delete(&self, key: CredentialKey) -> Result<()>;
}

/// The system keyring (Secret Service on Linux, reachable from Flatpak through
/// `--talk-name=org.freedesktop.secrets`).
///
/// Calls may block on D-Bus; run them off the UI thread.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

fn entry(key: CredentialKey) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key.account()).map_err(keyring_error)
}

#[allow(clippy::needless_pass_by_value, reason = "used with map_err")]
fn keyring_error(error: keyring::Error) -> Error {
    // keyring errors describe the store, never the secret.
    Error::Keyring {
        message: error.to_string(),
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: CredentialKey) -> Result<Option<Secret>> {
        match entry(key)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn set(&self, key: CredentialKey, secret: &Secret) -> Result<()> {
        entry(key)?
            .set_password(secret.expose())
            .map_err(keyring_error)?;
        tracing::info!(credential = key.account(), "credential stored");
        Ok(())
    }

    fn delete(&self, key: CredentialKey) -> Result<()> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

/// An in-memory store for tests.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<CredentialKey, Secret>>);

impl MemoryStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<CredentialKey, Secret>> {
        // A poisoned lock only means another test thread panicked.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, key: CredentialKey) -> Result<Option<Secret>> {
        Ok(self.map().get(&key).cloned())
    }

    fn set(&self, key: CredentialKey, secret: &Secret) -> Result<()> {
        self.map().insert(key, secret.clone());
        Ok(())
    }

    fn delete(&self, key: CredentialKey) -> Result<()> {
        self.map().remove(&key);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        assert_eq!(store.get(CredentialKey::GitHubToken).unwrap(), None);
        store
            .set(CredentialKey::GitHubToken, &Secret::new("ghp_x"))
            .unwrap();
        assert_eq!(
            store
                .get(CredentialKey::GitHubToken)
                .unwrap()
                .unwrap()
                .expose(),
            "ghp_x"
        );
        store.delete(CredentialKey::GitHubToken).unwrap();
        store.delete(CredentialKey::GitHubToken).unwrap();
        assert_eq!(store.get(CredentialKey::GitHubToken).unwrap(), None);
    }

    #[test]
    fn keys_have_distinct_accounts() {
        assert_ne!(
            CredentialKey::GitHubToken.account(),
            CredentialKey::EpicRefreshToken.account()
        );
    }
}
