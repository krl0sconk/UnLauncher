// SPDX-License-Identifier: GPL-3.0-or-later

//! Messages between the interface and `unlauncher-web`: one JSON object per
//! line, tagged by `type` in `snake_case`.
//!
//! ```
//! use web_bridge::protocol::{Command, encode_command};
//!
//! let line = encode_command(&Command::Open { url: "https://www.fab.com/".to_owned() });
//! assert_eq!(line, r#"{"type":"open","url":"https://www.fab.com/"}"#);
//! ```

use common::Secret;
use serde::{Deserialize, Serialize};

/// What the interface asks the window to do (written to its stdin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Show the window and load `url`.
    Open {
        /// The page to load.
        url: String,
    },
    /// Load `url` without showing the window (background checks such as the
    /// new-version notice).
    Load {
        /// The page to load.
        url: String,
    },
    /// Run `fetch(url, {credentials: 'include'})` inside the current page and
    /// answer with [`Event::FetchResult`].
    Fetch {
        /// Absolute, or relative to the current page.
        url: String,
    },
    /// Read every link (`<a href>`) of the current page and answer with
    /// [`Event::Links`].
    Links,
    /// Close the window and exit.
    Close,
    /// Delete cookies and every other browsing data of the window.
    ClearData,
}

/// What the window reports (read from its stdout).
///
/// `Debug` never shows the authorization code: it is a [`Secret`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The window exists (still hidden) and accepts commands.
    Ready,
    /// A page finished loading in the main frame. The URL has no query or
    /// fragment, which can carry codes.
    Navigated {
        /// Scheme, host and path of the page.
        url: String,
    },
    /// The page tried to open a new window (`window.open`, `target=_blank`):
    /// for example Epic's Google and Apple buttons. The window blocked it;
    /// the interface decides what to do (a third-party login continues in
    /// the system browser). The URL has no query or fragment.
    PopupBlocked {
        /// Scheme, host and path of the page the popup wanted to open.
        url: String,
    },
    /// The answer to a [`Command::Links`].
    Links {
        /// The page's links, in document order.
        links: Vec<Link>,
    },
    /// Epic's authorization-code page was read.
    AuthCode {
        /// The one-time authorization code.
        #[serde(with = "secret_string")]
        code: Secret,
    },
    /// The answer to a [`Command::Fetch`].
    FetchResult {
        /// HTTP status.
        status: u16,
        /// Response body as text.
        body: String,
    },
    /// The user closed the window, or [`Command::Close`] arrived.
    Closed,
    /// Something failed inside the window. Never contains secrets.
    Error {
        /// What failed.
        message: String,
    },
}

/// A link of a page. The URL can be a signed download link, so it is a
/// [`Secret`]: `Debug` never shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The link's visible text, trimmed (at most 100 characters).
    pub text: String,
    /// The absolute URL.
    #[serde(with = "secret_string")]
    pub url: Secret,
}

/// One command as a line of JSON, without the trailing newline.
#[must_use]
pub fn encode_command(command: &Command) -> String {
    // Serializing these plain enums cannot fail.
    serde_json::to_string(command).unwrap_or_default()
}

/// Parses one line written by the interface, or `None` if it is not a
/// command.
#[must_use]
pub fn decode_command(line: &str) -> Option<Command> {
    serde_json::from_str(line.trim()).ok()
}

/// One event as a line of JSON, without the trailing newline.
#[must_use]
pub fn encode_event(event: &Event) -> String {
    serde_json::to_string(event).unwrap_or_default()
}

/// Parses one line written by the window, or `None` if it is not an event.
/// The caller must not log a line that fails to parse: it may hold a code.
#[must_use]
pub fn decode_event(line: &str) -> Option<Event> {
    serde_json::from_str(line.trim()).ok()
}

/// (De)serializes a [`Secret`] as a plain JSON string. Only used on the pipe
/// between the two processes.
mod secret_string {
    use common::Secret;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(secret: &Secret, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(secret.expose())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Secret, D::Error> {
        String::deserialize(deserializer).map(Secret::new)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip() {
        for command in [
            Command::Open {
                url: "https://www.unrealengine.com/linux".to_owned(),
            },
            Command::Load {
                url: "https://www.fab.com/".to_owned(),
            },
            Command::Fetch {
                url: "/api/blobs/linux".to_owned(),
            },
            Command::Links,
            Command::Close,
            Command::ClearData,
        ] {
            let line = encode_command(&command);
            assert!(!line.contains('\n'));
            assert_eq!(decode_command(&line), Some(command));
        }
        assert_eq!(
            encode_command(&Command::ClearData),
            r#"{"type":"clear_data"}"#
        );
    }

    #[test]
    fn events_round_trip() {
        for event in [
            Event::Ready,
            Event::Navigated {
                url: "https://www.epicgames.com/id/login".to_owned(),
            },
            Event::PopupBlocked {
                url: "https://www.epicgames.com/id/login/google/forward".to_owned(),
            },
            Event::AuthCode {
                code: Secret::new("0123456789abcdef0123456789abcdef"),
            },
            Event::FetchResult {
                status: 200,
                body: "{\"blobs\":[]}\n".to_owned(),
            },
            Event::Links {
                links: vec![Link {
                    text: "Download".to_owned(),
                    url: Secret::new("https://example.invalid/blob?X-Amz-Signature=s"),
                }],
            },
            Event::Closed,
            Event::Error {
                message: "boom".to_owned(),
            },
        ] {
            let line = encode_event(&event);
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(decode_event(&line), Some(event));
        }
    }

    #[test]
    fn auth_code_travels_as_a_plain_string() {
        let line = r#"{"type":"auth_code","code":"abc123"}"#;
        let Some(Event::AuthCode { code }) = decode_event(line) else {
            panic!("not an auth code");
        };
        assert_eq!(code.expose(), "abc123");
    }

    #[test]
    fn debug_never_shows_the_auth_code() {
        let event = Event::AuthCode {
            code: Secret::new("abc123"),
        };
        let debug = format!("{event:?}");
        assert!(!debug.contains("abc123"), "{debug}");
        assert!(debug.contains("[redacted]"));
    }

    #[test]
    fn debug_never_shows_link_urls() {
        let event = Event::Links {
            links: vec![Link {
                text: "Download".to_owned(),
                url: Secret::new("https://example.invalid/blob?X-Amz-Signature=s3cr3t"),
            }],
        };
        let debug = format!("{event:?}");
        assert!(!debug.contains("s3cr3t"), "{debug}");
        assert!(debug.contains("Download"));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(decode_event("not json"), None);
        assert_eq!(decode_event(r#"{"type":"unknown"}"#), None);
        assert_eq!(decode_command(r#"{"type":"open"}"#), None);
    }
}
