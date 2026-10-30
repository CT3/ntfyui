use crate::config::{Server, Subscription};
use crate::AppState;
use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// ntfy sends a keepalive every ~45s; treat a longer silence as a dead connection.
const READ_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_BACKOFF_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub time: i64,
    pub event: String,
    pub topic: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub click: Option<String>,
    #[serde(default)]
    pub actions: Option<Vec<Action>>,
    #[serde(default)]
    pub attachment: Option<Attachment>,
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub subscription_id: String,
}

/// Action button; only "view" actions (open a URL) are used by the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub action: String,
    pub label: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub id: String,
    pub state: &'static str,
    pub error: Option<String>,
}

fn set_status(app: &AppHandle, id: &str, state: &'static str, error: Option<String>) {
    let status = Status {
        id: id.into(),
        state,
        error,
    };
    app.state::<AppState>()
        .statuses
        .lock()
        .unwrap()
        .insert(id.into(), status.clone());
    let _ = app.emit("status", status);
}

pub fn auth_header(server: &Server) -> Option<String> {
    match server.auth_type.as_str() {
        "basic" if !server.username.is_empty() => {
            let raw = format!("{}:{}", server.username, server.password);
            Some(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(raw)
            ))
        }
        "token" if !server.token.is_empty() => Some(format!("Bearer {}", server.token)),
        _ => None,
    }
}

/// Keeps a subscription connected forever, reconnecting with backoff.
/// `since` is the last message already seen, so anything newer is caught up on.
pub async fn run(app: AppHandle, server: Server, sub: Subscription, mut since: Option<String>) {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("http client");
    let mut backoff = 1;
    loop {
        set_status(&app, &sub.id, "connecting", None);
        let result = stream_once(&app, &client, &server, &sub, &mut since, &mut backoff).await;
        let err = match result {
            Ok(()) => "connection closed".to_string(),
            Err(e) => e,
        };
        set_status(&app, &sub.id, "error", Some(err));
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

async fn stream_once(
    app: &AppHandle,
    client: &reqwest::Client,
    server: &Server,
    sub: &Subscription,
    since: &mut Option<String>,
    backoff: &mut u64,
) -> Result<(), String> {
    let mut url = format!("{}/{}/json", server.url.trim_end_matches('/'), sub.topic);
    if let Some(id) = since.as_ref() {
        // Catch up on anything missed while disconnected or while the app was closed.
        url.push_str(&format!("?since={id}"));
    }
    let mut req = client.get(&url);
    if let Some(h) = auth_header(server) {
        req = req.header("Authorization", h);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {}", body.trim()));
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::time::timeout(READ_TIMEOUT, stream.next())
            .await
            .map_err(|_| "read timeout".to_string())?;
        let Some(chunk) = chunk else { return Ok(()) };
        buf.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let Ok(mut msg) = serde_json::from_slice::<Message>(&line) else {
                continue;
            };
            match msg.event.as_str() {
                "open" => {
                    *backoff = 1;
                    set_status(app, &sub.id, "connected", None);
                }
                "message" => {
                    *since = Some(msg.id.clone());
                    msg.server_name = server.name.clone();
                    msg.subscription_id = sub.id.clone();
                    deliver(app, msg);
                }
                _ => {}
            }
        }
    }
}

/// Tags that are emoji shortcodes (e.g. "warning" -> ⚠️) are shown as emoji, like the
/// official ntfy apps do. The map is shared with the UI.
fn split_tags(tags: &[String]) -> (String, Vec<&str>) {
    static EMOJI: OnceLock<HashMap<String, String>> = OnceLock::new();
    let map = EMOJI.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/emoji.json")).expect("valid emoji.json")
    });
    let mut emojis = String::new();
    let mut rest = Vec::new();
    for tag in tags {
        match map.get(tag) {
            Some(e) => emojis.push_str(e),
            None => rest.push(tag.as_str()),
        }
    }
    (emojis, rest)
}

fn deliver(app: &AppHandle, msg: Message) {
    let state = app.state::<AppState>();
    let mut history = state.history.lock().unwrap();
    // A catch-up after reconnecting can repeat messages we already have.
    if history.contains(&msg.id) {
        return;
    }
    if let Err(e) = history.push(msg.clone()) {
        eprintln!("failed to save messages: {e}");
    }
    drop(history);
    let _ = app.emit("message", &msg);

    let (emojis, tags) = split_tags(msg.tags.as_deref().unwrap_or_default());
    let title = match &msg.title {
        Some(t) if !t.is_empty() => t.as_str(),
        _ => msg.topic.as_str(),
    };
    let title = if emojis.is_empty() {
        title.to_string()
    } else {
        format!("{emojis} {title}")
    };
    let mut body = msg.message.clone().unwrap_or_default();
    if !tags.is_empty() {
        body = format!("{body}\n[{}]", tags.join(", "));
    }
    // Clicking the notification opens the message's click URL, or its first "view" action.
    let url = msg.click.clone().or_else(|| {
        msg.actions
            .iter()
            .flatten()
            .find(|a| a.action == "view")
            .and_then(|a| a.url.clone())
    });
    crate::notify(app, &msg.id, &title, &body, url.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(auth_type: &str) -> Server {
        Server {
            id: "s".into(),
            name: "s".into(),
            url: "https://example.com".into(),
            auth_type: auth_type.into(),
            username: "user".into(),
            password: "pass".into(),
            token: "tk_abc".into(),
        }
    }

    #[test]
    fn auth_header_by_type() {
        assert_eq!(auth_header(&server("none")), None);
        assert_eq!(
            auth_header(&server("basic")).as_deref(),
            Some("Basic dXNlcjpwYXNz")
        );
        assert_eq!(
            auth_header(&server("token")).as_deref(),
            Some("Bearer tk_abc")
        );
    }

    #[test]
    fn emoji_tags_are_split_from_plain_tags() {
        let tags = vec!["warning".to_string(), "nas".into(), "fire".into()];
        let (emojis, rest) = split_tags(&tags);
        assert_eq!(emojis, "⚠️🔥");
        assert_eq!(rest, vec!["nas"]);
    }

    #[test]
    fn parses_ntfy_message_line() {
        let line = r#"{"id":"abc","time":1,"event":"message","topic":"t","message":"hi","tags":["tada"],"click":"https://x.y"}"#;
        let msg: Message = serde_json::from_str(line).unwrap();
        assert_eq!(msg.message.as_deref(), Some("hi"));
        assert_eq!(msg.click.as_deref(), Some("https://x.y"));
        assert!(msg.actions.is_none());
    }
}
