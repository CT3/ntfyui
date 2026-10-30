# ntfyui

Desktop client for [ntfy](https://ntfy.sh): subscribe to topics on ntfy.sh or your own
server and get native system notifications. Built with Tauri v2 (Rust + plain HTML/JS),
so the same code targets macOS, Linux and Windows.

## Develop

    npm install
    npm run dev          # run with hot reload of the UI

## Build

    npm run build        # all bundle types for the current OS
    npx tauri build --bundles app   # just the macOS .app

Output: `src-tauri/target/release/bundle/`.

## macOS notifications

Notifications use `UNUserNotificationCenter` (`src-tauri/src/macos_notify.rs`), which only
works from the bundled, signed `.app` — not from `npm run dev`. The build ad-hoc signs the
bundle (`bundle.macOS.signingIdentity: "-"`) so macOS recognises it as `com.ntfyui.desktop`.

If notifications don't appear, enable **ntfyui** in System Settings → Notifications
(the Settings tab shows the current permission and links there). Clicking a notification
opens the message's link, or the app window if it has none.

## How it works

- `src-tauri/src/ntfy.rs` – one long-lived HTTP stream per subscription
  (`GET <server>/<topic>/json`), auto-reconnects with backoff and catches up on
  missed messages using `?since=<last id>`. Each message triggers a native notification.
- `src-tauri/src/config.rs` – settings stored at
  `~/Library/Application Support/com.ntfyui.desktop/config.json` (mode 0600; credentials are in plain text).
- Message history (last 200) is saved to `messages.json` in the same folder and reloaded on start.
- `src-tauri/src/lib.rs` – commands for the UI, tray icon, close-to-tray behaviour, opening
  links (click URL, "view" actions, attachments, URLs in the body) and launch at login
  (a LaunchAgent that starts the app with `--hidden`, straight into the tray).
- `src/` – UI: Messages, Send (any topic, with title/priority/tags/link), Subscriptions,
  Servers and Settings tabs.
