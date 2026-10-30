use crate::config::{Server, Subscription};
use crate::AppState;
use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
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
    let status = Status { id: id.into(), state, error };
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
            Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw)))
        }
        "token" if !server.token.is_empty() => Some(format!("Bearer {}", server.token)),
        _ => None,
    }
}

/// Keeps a subscription connected forever, reconnecting with backoff.
pub async fn run(app: AppHandle, server: Server, sub: Subscription) {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("http client");
    let mut since: Option<String> = None;
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
        // Catch up on anything missed while disconnected.
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
            let Ok(mut msg) = serde_json::from_slice::<Message>(&line) else { continue };
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

fn deliver(app: &AppHandle, msg: Message) {
    let title = match &msg.title {
        Some(t) if !t.is_empty() => t.clone(),
        _ => msg.topic.clone(),
    };
    let mut body = msg.message.clone().unwrap_or_default();
    if let Some(tags) = msg.tags.as_ref().filter(|t| !t.is_empty()) {
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

    let state = app.state::<AppState>();
    let mut messages = state.messages.lock().unwrap();
    messages.push_front(msg.clone());
    messages.truncate(crate::MAX_MESSAGES);
    if let Err(e) = crate::config::save_messages(app, &messages) {
        eprintln!("failed to save messages: {e}");
    }
    drop(messages);
    let _ = app.emit("message", msg);
}
