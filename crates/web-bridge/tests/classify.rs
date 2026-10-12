// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `classify_url`, with the URLs the window reported in a manual
//! test (2026-10-10), query strings removed as `Navigated` does.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use url::Url;
use web_bridge::{PageKind, Provider, classify_url};

fn kind(url: &str) -> PageKind {
    classify_url(&Url::parse(url).unwrap())
}

#[test]
fn epic_login_form() {
    assert_eq!(
        kind("https://www.epicgames.com/id/login"),
        PageKind::EpicLogin
    );
    assert_eq!(
        kind("https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com"),
        PageKind::EpicLogin
    );
    assert_eq!(
        kind("https://www.epicgames.com/id/login/epic"),
        PageKind::EpicLogin
    );
}

#[test]
fn authorization_code_page() {
    assert_eq!(
        kind(
            "https://www.epicgames.com/id/api/redirect?clientId=34a02cf8f4414e29b15921876da36f9a&responseType=code"
        ),
        PageKind::AuthCodePage
    );
    assert_eq!(
        kind("https://www.epicgames.com/id/api/redirect"),
        PageKind::AuthCodePage
    );
}

#[test]
fn third_party_providers_by_host() {
    for (url, provider) in [
        (
            "https://accounts.google.com/v3/signin/identifier",
            Provider::Google,
        ),
        ("https://appleid.apple.com/auth/authorize", Provider::Apple),
        (
            "https://www.facebook.com/v12.0/dialog/oauth",
            Provider::Facebook,
        ),
        (
            "https://login.live.com/oauth20_authorize.srf",
            Provider::Xbox,
        ),
        (
            "https://my.account.sony.com/sonyacct/signin/",
            Provider::PlayStation,
        ),
        ("https://steamcommunity.com/openid/login", Provider::Steam),
        (
            "https://accounts.nintendo.com/authorize_guide",
            Provider::Nintendo,
        ),
    ] {
        assert_eq!(kind(url), PageKind::ThirdPartyLogin(provider), "{url}");
    }
}

#[test]
fn third_party_providers_on_epic() {
    assert_eq!(
        kind("https://www.epicgames.com/id/login/google/forward"),
        PageKind::ThirdPartyLogin(Provider::Google)
    );
    for (path, provider) in [
        ("facebook", Provider::Facebook),
        ("psn", Provider::PlayStation),
        ("steam", Provider::Steam),
        ("nintendo", Provider::Nintendo),
    ] {
        assert_eq!(
            kind(&format!("https://www.epicgames.com/id/login/{path}")),
            PageKind::ThirdPartyLogin(provider)
        );
    }
    assert_eq!(
        kind("https://www.epicgames.com/id/login/google"),
        PageKind::ThirdPartyLogin(Provider::Google)
    );
    assert_eq!(
        kind("https://www.epicgames.com/id/login/apple?lang=en"),
        PageKind::ThirdPartyLogin(Provider::Apple)
    );
    assert_eq!(
        kind("https://www.epicgames.com/id/login/xbl"),
        PageKind::ThirdPartyLogin(Provider::Xbox)
    );
}

#[test]
fn epic_paths_round_trip() {
    for provider in [
        Provider::Google,
        Provider::Apple,
        Provider::Facebook,
        Provider::Xbox,
        Provider::PlayStation,
        Provider::Steam,
        Provider::Nintendo,
    ] {
        let url = format!(
            "https://www.epicgames.com/id/login/{}",
            provider.epic_path()
        );
        assert_eq!(
            kind(&url),
            PageKind::ThirdPartyLogin(provider),
            "{provider}"
        );
    }
}

#[test]
fn other_pages() {
    assert_eq!(kind("https://www.unrealengine.com/linux"), PageKind::Other);
    assert_eq!(kind("https://www.epicgames.com/store/"), PageKind::Other);
    assert_eq!(kind("about:blank"), PageKind::Other);
    assert_eq!(
        kind("https://evil.example/id/api/redirect"),
        PageKind::Other
    );
    assert_eq!(
        kind("https://www.epicgames.com.evil.example/id/login"),
        PageKind::Other
    );
}

#[test]
fn fab_pages_are_recognized() {
    for url in [
        "https://www.fab.com",
        "https://www.fab.com/",
        "https://www.fab.com/library",
        "https://www.fab.com/listings/0b1e2d3c-aaaa-bbbb-cccc-1234567890ab",
        "https://fab.com/sellers/Epic%20Games",
        "https://WWW.FAB.COM/search?q=trees",
    ] {
        assert_eq!(kind(url), PageKind::Fab, "{url}");
    }
}

#[test]
fn lookalike_hosts_are_not_fab() {
    for url in [
        "https://www.fab.com.example.net/library",
        "https://notfab.com/",
        "https://fab.com.evil.example/",
        "https://example.com/www.fab.com",
        "ftp://www.fab.com/library",
    ] {
        assert_eq!(kind(url), PageKind::Other, "{url}");
    }
}

#[test]
fn signing_in_from_fab_goes_through_epics_pages() {
    // Fab sends the user to Epic's login and back: the login pages keep
    // their own kind so the interface can handle them.
    assert_eq!(
        kind("https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.fab.com%2F"),
        PageKind::EpicLogin
    );
}
