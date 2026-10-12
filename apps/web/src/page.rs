// SPDX-License-Identifier: GPL-3.0-or-later

//! Scripts run inside the page and the messages they post back.
//!
//! Every message carries a per-process nonce, so a page cannot fake a fetch
//! result or a code page by calling `window.ipc.postMessage` on its own.

use std::io::Read;

use url::Url;

/// A random nonce for this process, as hex.
pub fn new_nonce() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        // Not a secret, only an anti-spoofing tag: fall back to the clock and pid.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        bytes = (nanos ^ u128::from(std::process::id())).to_le_bytes();
    }
    bytes.iter().fold(String::with_capacity(32), |mut hex, b| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

/// A JavaScript string literal for `text`.
fn js_string(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// Runs `fetch(url, {credentials: 'include'})` and posts the status and body.
pub fn fetch_script(nonce: &str, url: &str) -> String {
    format!(
        r"(async () => {{
  const nonce = {nonce};
  const post = (o) => window.ipc.postMessage(JSON.stringify(Object.assign({{nonce}}, o)));
  try {{
    const response = await fetch({url}, {{credentials: 'include'}});
    const body = await response.text();
    post({{kind: 'fetch_result', status: response.status, body}});
  }} catch (error) {{
    post({{kind: 'fetch_error', message: String(error)}});
  }}
}})();",
        nonce = js_string(nonce),
        url = js_string(url),
    )
}

/// Posts every link of the current page (absolute http(s) URLs and their
/// text).
pub fn links_script(nonce: &str) -> String {
    format!(
        r"(() => {{
  const links = [...document.querySelectorAll('a[href]')]
    .filter((a) => /^https?:/.test(a.href))
    .map((a) => ({{text: (a.innerText || '').trim().slice(0, 100), url: a.href}}));
  window.ipc.postMessage(JSON.stringify({{nonce: {nonce}, kind: 'links', links}}));
}})();",
        nonce = js_string(nonce),
    )
}

/// Posts the text of the current page (used on the authorization-code page).
pub fn page_text_script(nonce: &str) -> String {
    format!(
        r"window.ipc.postMessage(JSON.stringify({{nonce: {nonce}, kind: 'page_text', text: document.body ? document.body.innerText : ''}}));",
        nonce = js_string(nonce),
    )
}

/// A message posted by one of the scripts above.
#[derive(Debug, PartialEq, Eq)]
pub enum PageMessage {
    /// The answer to a fetch.
    FetchResult { status: u16, body: String },
    /// A fetch that failed before getting an answer.
    FetchError { message: String },
    /// The text of the current page.
    PageText { text: String },
    /// The links of the current page: `(text, url)`.
    Links(Vec<(String, String)>),
}

/// Parses a posted message, or `None` if it is malformed or its nonce does
/// not match (posted by the page itself, not by UnLauncher's scripts).
pub fn parse_message(body: &str, nonce: &str) -> Option<PageMessage> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("nonce")?.as_str()? != nonce {
        return None;
    }
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    match value.get("kind")?.as_str()? {
        "fetch_result" => Some(PageMessage::FetchResult {
            status: u16::try_from(value.get("status")?.as_u64()?).ok()?,
            body: text("body")?,
        }),
        "fetch_error" => Some(PageMessage::FetchError {
            message: text("message")?,
        }),
        "page_text" => Some(PageMessage::PageText {
            text: text("text")?,
        }),
        "links" => Some(PageMessage::Links(
            value
                .get("links")?
                .as_array()?
                .iter()
                .filter_map(|link| {
                    Some((
                        link.get("text")?.as_str()?.to_owned(),
                        link.get("url")?.as_str()?.to_owned(),
                    ))
                })
                .collect(),
        )),
        _ => None,
    }
}

/// What the authorization-code page says.
#[derive(Debug, PartialEq, Eq)]
pub enum CodePage {
    /// The code.
    Code(String),
    /// `"authorizationCode": null`: no web session (not signed in).
    NotSignedIn,
    /// Not the JSON Epic shows.
    Unreadable,
}

/// Reads Epic's authorization-code page:
/// `{"redirectUrl": …, "authorizationCode": "…", …}`.
pub fn read_code_page(text: &str) -> CodePage {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return CodePage::Unreadable;
    };
    match value.get("authorizationCode") {
        Some(serde_json::Value::String(code)) if !code.trim().is_empty() => {
            CodePage::Code(code.trim().to_owned())
        }
        Some(serde_json::Value::Null) => CodePage::NotSignedIn,
        _ => CodePage::Unreadable,
    }
}

/// The URL without query or fragment, which can carry codes, or `None` if it
/// is not a web page.
pub fn public_url(url: &str) -> Option<Url> {
    let mut url = Url::parse(url).ok()?;
    if !matches!(url.scheme(), "https" | "http") {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn nonces_are_random_hex() {
        let (a, b) = (new_nonce(), new_nonce());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn scripts_quote_their_arguments() {
        let script = fetch_script("n1", "/api/blobs/linux?a='b'\"c\"");
        assert!(
            script.contains(r#"fetch("/api/blobs/linux?a='b'\"c\"""#),
            "{script}"
        );
        assert!(script.contains(r#"const nonce = "n1";"#));
        assert!(page_text_script("n1").contains(r#"nonce: "n1""#));
    }

    #[test]
    fn parses_messages_with_the_right_nonce() {
        let body = r#"{"nonce":"n1","kind":"fetch_result","status":200,"body":"{}"}"#;
        assert_eq!(
            parse_message(body, "n1"),
            Some(PageMessage::FetchResult {
                status: 200,
                body: "{}".to_owned()
            })
        );
        assert_eq!(parse_message(body, "other"), None);
        assert_eq!(
            parse_message(r#"{"kind":"page_text","text":"x"}"#, "n1"),
            None
        );
        assert_eq!(
            parse_message(r#"{"nonce":"n1","kind":"page_text","text":"x"}"#, "n1"),
            Some(PageMessage::PageText {
                text: "x".to_owned()
            })
        );
        assert_eq!(
            parse_message(
                r#"{"nonce":"n1","kind":"fetch_error","message":"Load failed"}"#,
                "n1"
            ),
            Some(PageMessage::FetchError {
                message: "Load failed".to_owned()
            })
        );
        assert_eq!(parse_message("not json", "n1"), None);
    }

    #[test]
    fn parses_links() {
        let body = r#"{"nonce":"n1","kind":"links","links":[{"text":"Download","url":"https://a/b?s=1"},{"text":1}]}"#;
        assert_eq!(
            parse_message(body, "n1"),
            Some(PageMessage::Links(vec![(
                "Download".to_owned(),
                "https://a/b?s=1".to_owned()
            )]))
        );
        assert!(links_script("n1").contains(r#"nonce: "n1""#));
    }

    #[test]
    fn reads_the_code_page() {
        let page = r#"{"warning":"Do not share this code with any 3rd party service.","redirectUrl":"https://localhost/launcher/authorized?code=abc","authorizationCode":"0123456789abcdef0123456789abcdef","exchangeCode":null,"sid":null}"#;
        assert_eq!(
            read_code_page(page),
            CodePage::Code("0123456789abcdef0123456789abcdef".to_owned())
        );
        assert_eq!(
            read_code_page(r#"{"redirectUrl":"x","authorizationCode":null}"#),
            CodePage::NotSignedIn
        );
        assert_eq!(read_code_page("<html>"), CodePage::Unreadable);
        assert_eq!(
            read_code_page(r#"{"authorizationCode":""}"#),
            CodePage::Unreadable
        );
    }

    #[test]
    fn fab_urls_keep_their_path_and_lose_the_query() {
        let url = public_url("https://www.fab.com/library?sort_by=-created_at#top").unwrap();
        assert_eq!(url.as_str(), "https://www.fab.com/library");
        assert_eq!(
            web_bridge::classify_url(&url),
            web_bridge::PageKind::Fab,
            "the window reports Fab pages as such"
        );
    }

    #[test]
    fn public_urls_drop_query_and_fragment() {
        assert_eq!(
            public_url("https://www.epicgames.com/id/api/redirect?code=secret#x")
                .unwrap()
                .as_str(),
            "https://www.epicgames.com/id/api/redirect"
        );
        assert_eq!(public_url("about:blank"), None);
        assert_eq!(public_url("not a url"), None);
    }
}
