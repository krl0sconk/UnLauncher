// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

const REDACTED: &str = "[redacted]";

/// A token or credential.
///
/// `Debug` and `Display` print `[redacted]`, so a `Secret` can be stored in
/// structs that are logged without leaking its value. The value is only
/// reachable through [`Secret::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the secret value. Call it only where the value is sent to its
    /// destination (an HTTP header, the keyring), never to log or display it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_are_redacted() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        assert_eq!(format!("{:?}", Some(&secret)), "Some([redacted])");
    }

    #[test]
    fn expose_returns_the_value() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
    }
}
