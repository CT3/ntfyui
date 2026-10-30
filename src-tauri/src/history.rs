//! Received-message history, kept in memory and persisted to `messages.json`.

use crate::ntfy::Message;
use std::collections::VecDeque;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const MAX_MESSAGES: usize = 200;

pub struct History {
    messages: VecDeque<Message>,
    path: Option<PathBuf>,
}

impl History {
    pub fn load(app: &AppHandle) -> Self {
        let path = app
            .path()
            .app_data_dir()
            .ok()
            .map(|d| d.join("messages.json"));
        let messages = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        History { messages, path }
    }

    /// Newest first.
    pub fn all(&self) -> Vec<Message> {
        self.messages.iter().cloned().collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.messages.iter().any(|m| m.id == id)
    }

    /// Id of the newest stored message for a subscription, used to resume after a restart.
    pub fn last_id(&self, subscription_id: &str) -> Option<String> {
        self.messages
            .iter()
            .find(|m| m.subscription_id == subscription_id)
            .map(|m| m.id.clone())
    }

    pub fn push(&mut self, msg: Message) -> Result<(), String> {
        self.messages.push_front(msg);
        self.messages.truncate(MAX_MESSAGES);
        self.save()
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.messages.clear();
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(&self.messages).map_err(|e| e.to_string())?;
        // Write to a temp file and rename so a crash mid-write can't corrupt the history.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}
