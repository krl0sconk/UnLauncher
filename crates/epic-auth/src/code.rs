// SPDX-License-Identifier: GPL-3.0-or-later

use common::Secret;

use crate::error::{Error, Result};

/// Epic's login page for the launcher client. After signing in, the browser
/// ends on a JSON page whose `authorizationCode` the user pastes into
/// UnLauncher (see [`parse_authorization_code`]).
///
/// Same URL as Legendary and `egs-api` (client `34a02cf8f4414e29b15921876da36f9a`).
pub const LOGIN_URL: &str = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode";

/// Epic's page that signs a browser in to Epic's websites with a one-time
/// exchange code (what the Epic Games Launcher does for its store pages).
pub const WEB_EXCHANGE_URL: &str = "https://www.epicgames.com/id/exchange";

/// Epic's login page for the launcher client, going straight to a
/// third-party provider. `provider_path` is the provider's segment in
/// `https://www.epicgames.com/id/login/<provider>` (`google`, `apple`,
/// `facebook`, `xbl`, `psn`, `steam`, `nintendo`). Like [`LOGIN_URL`], it
/// ends on the authorization-code page.
#[must_use]
pub fn provider_login_url(provider_path: &str) -> String {
    LOGIN_URL.replacen("/id/login?", &format!("/id/login/{provider_path}?"), 1)
}

/// The URL that signs the built-in web window in with `exchange_code` (see
/// [`crate::EpicSession::web_exchange_code`]) and then opens `redirect_url`.
///
/// It is a [`Secret`] because it carries the code: never log it.
#[must_use]
pub fn web_login_url(exchange_code: &Secret, redirect_url: &str) -> Secret {
    let url = reqwest::Url::parse_with_params(
        WEB_EXCHANGE_URL,
        &[
            ("exchangeCode", exchange_code.expose()),
            ("redirectUrl", redirect_url),
        ],
    )
    .map(String::from)
    .unwrap_or_default();
    Secret::new(url)
}

/// Length of an Epic authorization code (hexadecimal characters).
const CODE_LEN: usize = 32;

/// Extracts the authorization code from what the user pasted.
///
/// Accepts the bare 32-character code (optionally surrounded by quotes or
/// whitespace) or the whole JSON page Epic shows,
/// `{"redirectUrl":"…","authorizationCode":"…","sid":null}`.
///
/// # Errors
///
/// - [`Error::NotSignedIn`] if the page says `"authorizationCode": null`.
/// - [`Error::InvalidAuthorizationCode`] for anything else that is not a code.
///   The error never contains the pasted text.
pub fn parse_authorization_code(pasted: &str) -> Result<Secret> {
    let trimmed = trim_quotes(pasted);
    if trimmed.starts_with('{') {
        let page: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|_| Error::InvalidAuthorizationCode)?;
        return match page.get("authorizationCode") {
            Some(serde_json::Value::String(code)) => validate(trim_quotes(code)),
            Some(serde_json::Value::Null) => Err(Error::NotSignedIn),
            _ => Err(Error::InvalidAuthorizationCode),
        };
    }
    validate(trimmed)
}

fn trim_quotes(text: &str) -> &str {
    text.trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace())
}

fn validate(code: &str) -> Result<Secret> {
    if code.len() == CODE_LEN && code.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(Secret::new(code))
    } else {
        Err(Error::InvalidAuthorizationCode)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CODE: &str = "0123456789abcdef0123456789ABCDEF";

    #[test]
    fn accepts_raw_code() {
        assert_eq!(parse_authorization_code(CODE).unwrap().expose(), CODE);
    }

    #[test]
    fn trims_whitespace_and_quotes() {
        let pasted = format!("  \"{CODE}\"\n");
        assert_eq!(parse_authorization_code(&pasted).unwrap().expose(), CODE);
    }

    #[test]
    fn accepts_json_page() {
        let page = format!(
            r#" {{"warning":"Do not share this code with any 3rd party service.","redirectUrl":"https://localhost/launcher/authorized?code={CODE}","authorizationCode":"{CODE}","exchangeCode":null,"sid":null}} "#
        );
        assert_eq!(parse_authorization_code(&page).unwrap().expose(), CODE);
    }

    #[test]
    fn null_code_means_not_signed_in() {
        let page = r#"{"redirectUrl":"https://www.epicgames.com/id/login","authorizationCode":null,"sid":null}"#;
        assert!(matches!(
            parse_authorization_code(page),
            Err(Error::NotSignedIn)
        ));
    }

    #[test]
    fn rejects_garbage() {
        for pasted in [
            "",
            "hello",
            "0123456789abcdef",
            "0123456789abcdef0123456789abcdeg",
            "{not json",
            r#"{"redirectUrl":"x"}"#,
            r#"{"authorizationCode":42}"#,
        ] {
            assert!(
                matches!(
                    parse_authorization_code(pasted),
                    Err(Error::InvalidAuthorizationCode)
                ),
                "{pasted}"
            );
        }
    }

    #[test]
    fn errors_do_not_echo_the_input() {
        let error = parse_authorization_code("secret-ish-input").unwrap_err();
        assert!(!error.to_string().contains("secret-ish-input"));
    }

    #[test]
    fn provider_login_urls_keep_the_redirect() {
        let url = provider_login_url("google");
        assert!(
            url.starts_with("https://www.epicgames.com/id/login/google?redirectUrl="),
            "{url}"
        );
        assert!(url.ends_with(LOGIN_URL.split_once('?').map_or("", |(_, q)| q)));
    }

    #[test]
    fn web_login_url_encodes_both_parameters() {
        let url = web_login_url(&Secret::new("abc123"), "https://www.unrealengine.com/linux");
        assert_eq!(
            url.expose(),
            "https://www.epicgames.com/id/exchange?exchangeCode=abc123&redirectUrl=https%3A%2F%2Fwww.unrealengine.com%2Flinux"
        );
        assert_eq!(format!("{url:?}"), "[redacted]");
    }
}
