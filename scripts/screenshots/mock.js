// Fake Tauri bridge + window frame used by scripts/screenshots.sh to render README screenshots.
// All data is fictional. Wrapped in a block so names don't clash with main.js.
{
const now = Math.floor(Date.now() / 1000);
const servers = [
  { id: "default", name: "ntfy.sh", url: "https://ntfy.sh", auth_type: "none", username: "", password: "", token: "" },
  { id: "home", name: "Home server", url: "https://ntfy.example.com", auth_type: "token", username: "", password: "", token: "tk_x" },
];
const subscriptions = [
  { id: "s1", server_id: "home", topic: "homelab", enabled: true },
  { id: "s2", server_id: "home", topic: "backups", enabled: true },
  { id: "s3", server_id: "default", topic: "deploys", enabled: true },
  { id: "s4", server_id: "default", topic: "weather-alerts", enabled: false },
];
const messages = [
  { id: "m1", time: now - 60, event: "message", topic: "homelab", title: "Disk almost full", message: "/dev/sda1 on nas is at 92% capacity.", priority: 4, tags: ["warning", "nas"], click: "https://grafana.example.com/d/disks", server_name: "Home server", subscription_id: "s1" },
  { id: "m2", time: now - 900, event: "message", topic: "deploys", title: "Deploy succeeded", message: "web-app v2.4.1 is live in production.\nChangelog: https://github.com/example/web-app/releases", priority: 3, tags: ["rocket"], actions: [{ action: "view", label: "Open dashboard", url: "https://example.com" }], server_name: "ntfy.sh", subscription_id: "s3" },
  { id: "m3", time: now - 3600, event: "message", topic: "backups", title: "Nightly backup complete", message: "142 GB in 18 min, 0 errors.", priority: 2, tags: ["white_check_mark"], server_name: "Home server", subscription_id: "s2" },
  { id: "m4", time: now - 7200, event: "message", topic: "homelab", title: "Server unreachable", message: "pihole did not respond to 3 health checks.", priority: 5, tags: ["rotating_light"], attachment: { name: "healthcheck.log", url: "https://example.com/log" }, server_name: "Home server", subscription_id: "s1" },
];
const statuses = [
  { id: "s1", state: "connected", error: null },
  { id: "s2", state: "connected", error: null },
  { id: "s3", state: "connected", error: null },
];
const responses = {
  get_config: { servers, subscriptions },
  get_messages: messages,
  get_statuses: statuses,
  get_autostart: true,
  notification_permission: "authorized",
};
window.__TAURI__ = {
  core: { invoke: async (cmd) => responses[cmd] ?? null },
  event: { listen: async () => () => {} },
};
window.addEventListener("load", () => {
  const tab = location.hash.slice(1);
  setTimeout(() => {
    if (tab) document.querySelector(`.tab[data-tab="${tab}"]`)?.click();
    if (tab === "send") {
      document.querySelector("#send-server").value = "home";
      document.querySelector("#send-topic").value = "homelab";
      document.querySelector("#send-title").value = "Backup started";
      document.querySelector("#send-message").value = "Weekly offsite backup is running…";
      document.querySelector("#send-tags").value = "floppy_disk";
    }
    if (tab === "servers") {
      document.querySelector("#server-form").remove();
    }
  }, 50);
});

// Wrap the app in a macOS-style window on a gradient backdrop.
document.addEventListener("DOMContentLoaded", () => {
  const style = document.createElement("style");
  style.textContent = `
    html { height: 100%; }
    body.shot { height: 100vh; display: grid; place-items: center; background: linear-gradient(135deg, #2f7a6d 0%, #1d4f6b 100%); }
    .win { width: 820px; height: 590px; border-radius: 12px; overflow: hidden; display: flex; flex-direction: column;
           box-shadow: 0 24px 60px rgba(0,0,0,.35), 0 0 0 1px rgba(0,0,0,.2); background: var(--bg); }
    .bar { height: 30px; flex-shrink: 0; background: var(--sidebar); border-bottom: 1px solid var(--border); display: flex; align-items: center;
           padding: 0 12px; gap: 8px; position: relative; font-size: 13px; color: var(--muted); }
    .bar i { width: 12px; height: 12px; border-radius: 50%; }
    .bar span { position: absolute; inset: 0; display: grid; place-items: center; }
    .appbody { flex: 1; display: flex; min-height: 0; }
  `;
  document.head.append(style);
  const win = document.createElement("div");
  win.className = "win";
  win.innerHTML = `<div class="bar"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i><span>ntfyui</span></div>`;
  const appbody = document.createElement("div");
  appbody.className = "appbody";
  appbody.append(document.querySelector(".sidebar"), document.querySelector("main"));
  win.append(appbody);
  document.body.prepend(win);
  document.body.classList.add("shot");
});
}
