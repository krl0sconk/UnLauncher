// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the web window may go.

use url::Url;

/// Domains the window may navigate to, with all their subdomains.
///
/// Epic's sign-in and web session (`epicgames.com`, `epicgames.dev`),
/// Epic's Linux download page (`unrealengine.com`), Fab (`fab.com`),
/// Cloudflare's challenge (`cloudflare.com`) and the captcha Epic's sign-in
/// form embeds (`hcaptcha.com`).
pub const ALLOWED_DOMAINS: [&str; 6] = [
    "epicgames.com",
    "unrealengine.com",
    "fab.com",
    "epicgames.dev",
    "cloudflare.com",
    "hcaptcha.com",
];

/// Whether the web window may navigate to `url`.
///
/// Only `https` on [`ALLOWED_DOMAINS`] (or a subdomain of one), on the default
/// port and without credentials, plus `about:blank`. Everything else is
/// refused: `file:`, `data:`, `javascript:`, `http:`, other hosts, and
/// anything that does not parse. The caller reports what was refused so the
/// interface can offer the system browser.
#[must_use]
pub fn is_allowed_navigation(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    if url.scheme() == "about" {
        return url.as_str() == "about:blank";
    }
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    // `Url` lowercases the host and has already dropped a trailing-dot-free
    // form of it; a trailing dot is kept, so strip it before comparing.
    let Some(host) = url.host_str().map(|h| h.trim_end_matches('.')) else {
        return false;
    };
    ALLOWED_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::is_allowed_navigation;

    #[test]
    fn allowed_and_blocked_urls() {
        let allowed = [
            "https://www.epicgames.com/id/login",
            "https://epicgames.com/id/api/redirect?clientId=x",
            "https://www.unrealengine.com/linux",
            "https://www.fab.com/i/unreal-engine/versions",
            "https://dev.epicgames.com/documentation",
            "https://eos.epicgames.dev/x",
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b",
            "https://newassets.hcaptcha.com/captcha/v1",
            "HTTPS://WWW.FAB.COM",
            "https://WWW.Epicgames.COM./id",
            "https://www.fab.com:443/",
            "about:blank",
        ];
        for url in allowed {
            assert!(is_allowed_navigation(url), "should allow {url}");
        }
        let blocked = [
            "https://epicgames.com.evil.net",
            "https://evil.net/?x=epicgames.com",
            "https://evil.net/epicgames.com",
            "https://notepicgames.com",
            "https://fab.com.evil.net/",
            "https://epicgames.com@evil.net/",
            "https://user:pw@www.epicgames.com/",
            "https://www.fab.com:8443/",
            "http://www.epicgames.com/id/login",
            "file:///etc/passwd",
            "data:text/html,<script>alert(1)</script>",
            "javascript:alert(1)",
            "blob:https://www.fab.com/1234",
            "about:srcdoc",
            "about:blank#x",
            "https://accounts.google.com/v3/signin",
            "ftp://www.fab.com/",
            "",
            "not a url",
            "https://",
        ];
        for url in blocked {
            assert!(!is_allowed_navigation(url), "should block {url}");
        }
    }
}
