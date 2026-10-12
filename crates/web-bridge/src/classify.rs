// SPDX-License-Identifier: GPL-3.0-or-later

//! What kind of page the web window is showing.

use std::fmt;

use url::Url;

/// A login provider Epic offers besides its own email and password form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Google.
    Google,
    /// Apple.
    Apple,
    /// Facebook.
    Facebook,
    /// Xbox (Microsoft account).
    Xbox,
    /// PlayStation Network.
    PlayStation,
    /// Steam.
    Steam,
    /// Nintendo Account.
    Nintendo,
}

impl Provider {
    /// The provider's segment in Epic's
    /// `https://www.epicgames.com/id/login/<provider>` page, to start the same
    /// login in the system browser. Every third-party login goes there: the
    /// providers don't like embedded web views, and the browser already has
    /// the user's accounts.
    #[must_use]
    pub fn epic_path(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
            Self::Facebook => "facebook",
            Self::Xbox => "xbl",
            Self::PlayStation => "psn",
            Self::Steam => "steam",
            Self::Nintendo => "nintendo",
        }
    }

    /// The provider's name for the interface.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Apple => "Apple",
            Self::Facebook => "Facebook",
            Self::Xbox => "Xbox",
            Self::PlayStation => "PlayStation",
            Self::Steam => "Steam",
            Self::Nintendo => "Nintendo",
        }
    }

    /// The provider whose login page lives on `host`.
    fn from_host(host: &str) -> Option<Self> {
        let provider = match host {
            "accounts.google.com" => Self::Google,
            "appleid.apple.com" | "idmsa.apple.com" => Self::Apple,
            "facebook.com" | "www.facebook.com" | "m.facebook.com" => Self::Facebook,
            "login.live.com" | "login.microsoftonline.com" => Self::Xbox,
            "auth.api.sonyentertainmentnetwork.com" => Self::PlayStation,
            h if h.ends_with(".account.sony.com") => Self::PlayStation,
            "steamcommunity.com" | "store.steampowered.com" => Self::Steam,
            "accounts.nintendo.com" => Self::Nintendo,
            _ => return None,
        };
        Some(provider)
    }

    /// The provider of Epic's `/id/login/<provider>` page.
    fn from_epic_path(segment: &str) -> Option<Self> {
        let provider = match segment.to_ascii_lowercase().as_str() {
            "google" => Self::Google,
            "apple" => Self::Apple,
            "facebook" => Self::Facebook,
            "xbl" | "xbox" => Self::Xbox,
            "psn" | "playstation" => Self::PlayStation,
            "steam" => Self::Steam,
            "nintendo" => Self::Nintendo,
            _ => return None,
        };
        Some(provider)
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a page is, as far as the login flow cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// Epic's own login form (`www.epicgames.com/id/login…`).
    EpicLogin,
    /// The page that shows the authorization code as JSON
    /// (`www.epicgames.com/id/api/redirect`).
    AuthCodePage,
    /// A third-party login, at the provider or on Epic's page that hands
    /// over to it.
    ThirdPartyLogin(Provider),
    /// A page of Fab (`www.fab.com`), where the user browses and gets more
    /// assets.
    Fab,
    /// Anything else.
    Other,
}

/// Classifies a page by its URL. Only the host and path are used.
#[must_use]
pub fn classify_url(url: &Url) -> PageKind {
    if !matches!(url.scheme(), "https" | "http") {
        return PageKind::Other;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return PageKind::Other;
    };
    if let Some(provider) = Provider::from_host(&host) {
        return PageKind::ThirdPartyLogin(provider);
    }
    if host == "www.fab.com" || host == "fab.com" {
        return PageKind::Fab;
    }
    if host != "www.epicgames.com" && host != "epicgames.com" {
        return PageKind::Other;
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    match segments.as_slice() {
        ["id", "api", "redirect", ..] => PageKind::AuthCodePage,
        ["id", "login", provider, ..] => Provider::from_epic_path(provider)
            .map_or(PageKind::EpicLogin, PageKind::ThirdPartyLogin),
        ["id", "login"] => PageKind::EpicLogin,
        _ => PageKind::Other,
    }
}
