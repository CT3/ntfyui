const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (sel) => document.querySelector(sel);

let config = { servers: [], subscriptions: [] };
let messages = [];
const statuses = {};

// ---------- helpers ----------

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (k === "class") node.className = v;
    else node.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c != null) node.append(c instanceof Node ? c : document.createTextNode(c));
  }
  return node;
}

function toast(text) {
  const t = $("#toast");
  t.textContent = text;
  t.classList.add("show");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => t.classList.remove("show"), 2500);
}

const serverById = (id) => config.servers.find((s) => s.id === id);

async function save(next) {
  try {
    config = await invoke("save_config", { config: next });
    render();
  } catch (e) {
    toast(`Failed to save: ${e}`);
  }
}

// ---------- tabs ----------

document.querySelectorAll(".tab").forEach((btn) =>
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((b) => b.classList.toggle("active", b === btn));
    document.querySelectorAll(".panel").forEach((p) => p.classList.toggle("active", p.id === btn.dataset.tab));
  })
);

// ---------- rendering ----------

function render() {
  renderServerSelect();
  renderSubscriptions();
  renderServers();
  renderMessages();
}

function renderServerSelect() {
  for (const sel of [$("#sub-server"), $("#send-server")]) {
    const prev = sel.value;
    sel.replaceChildren(...config.servers.map((s) => el("option", { value: s.id }, s.name)));
    if (serverById(prev)) sel.value = prev;
  }
  renderSendTopics();
}

/** Suggests the subscribed topics of the server selected on the Send tab. */
function renderSendTopics() {
  const serverId = $("#send-server").value;
  $("#send-topics").replaceChildren(
    ...config.subscriptions.filter((s) => s.server_id === serverId).map((s) => el("option", { value: s.topic }))
  );
}

function renderSubscriptions() {
  const list = $("#sub-list");
  list.replaceChildren(
    ...config.subscriptions.map((sub) => {
      const server = serverById(sub.server_id);
      const st = sub.enabled ? statuses[sub.id] : null;
      const toggle = el("input", { type: "checkbox" });
      toggle.checked = sub.enabled;
      toggle.addEventListener("change", () =>
        save({
          ...config,
          subscriptions: config.subscriptions.map((s) => (s.id === sub.id ? { ...s, enabled: toggle.checked } : s)),
        })
      );
      return el(
        "li",
        {},
        el("span", { class: `dot ${st?.state ?? ""}`, title: st?.state ?? "disabled" }),
        el(
          "div",
          { class: "grow" },
          el("div", {}, sub.topic),
          el("div", { class: "meta" }, server ? `${server.name} · ${server.url}` : "server removed"),
          st?.state === "error" && st.error ? el("div", { class: "err" }, st.error) : null
        ),
        el("label", { class: "switch", title: "Enabled" }, toggle, el("span")),
        el(
          "button",
          {
            class: "danger small",
            onclick: () => save({ ...config, subscriptions: config.subscriptions.filter((s) => s.id !== sub.id) }),
          },
          "Remove"
        )
      );
    })
  );
  $("#subs-empty").style.display = config.subscriptions.length ? "none" : "";
}

function renderServers() {
  $("#server-list").replaceChildren(
    ...config.servers.map((s) => {
      const count = config.subscriptions.filter((x) => x.server_id === s.id).length;
      const authLabel = { basic: "user/pass", token: "token" }[s.auth_type];
      return el(
        "li",
        {},
        el(
          "div",
          { class: "grow" },
          el("div", {}, s.name, " ", s.id === "default" ? el("span", { class: "badge" }, "default") : null),
          el("div", { class: "meta" }, `${s.url} · ${count} subscription${count === 1 ? "" : "s"}`)
        ),
        authLabel ? el("span", { class: "badge" }, authLabel) : null,
        el("button", { class: "secondary small", onclick: () => editServer(s) }, "Edit"),
        s.id === "default"
          ? null
          : el("button", { class: "danger small", onclick: () => removeServer(s) }, "Remove")
      );
    })
  );
}

const openUrl = (url) => invoke("open_url", { url }).catch((e) => toast(`Can't open link: ${e}`));

function link(url, text = url) {
  return el("a", { title: url, onclick: () => openUrl(url) }, text);
}

/** Splits text into text nodes and clickable links for any http(s) URLs. */
function linkify(text) {
  return text.split(/(https?:\/\/[^\s<>"]+)/g).map((part, i) => (i % 2 ? link(part) : part));
}

/** Buttons for the message's click URL, "view" actions and attachment. */
function messageLinks(m) {
  const links = [];
  if (m.click) links.push(el("button", { class: "secondary small", onclick: () => openUrl(m.click) }, "Open link"));
  for (const a of m.actions ?? []) {
    if (a.action === "view" && a.url) {
      links.push(el("button", { class: "secondary small", onclick: () => openUrl(a.url) }, a.label));
    }
  }
  if (m.attachment?.url) {
    links.push(
      el("button", { class: "secondary small", onclick: () => openUrl(m.attachment.url) }, `📎 ${m.attachment.name || "Attachment"}`)
    );
  }
  return links.length ? el("div", { class: "links" }, links) : null;
}

function renderMessages() {
  $("#message-list").replaceChildren(
    ...messages.map((m) =>
      el(
        "li",
        { class: `msg prio-${m.priority ?? 3}` },
        el(
          "div",
          { class: "top" },
          el("span", { class: "title" }, m.title || m.topic),
          el("span", { class: "meta" }, new Date(m.time * 1000).toLocaleString())
        ),
        el("div", { class: "body" }, linkify(m.message ?? "")),
        messageLinks(m),
        el(
          "div",
          { class: "meta" },
          `${m.topic} · ${m.server_name}`,
          m.tags?.length ? ` · ${m.tags.join(", ")}` : ""
        )
      )
    )
  );
  $("#messages-empty").style.display = messages.length ? "none" : "";
}

// ---------- subscriptions ----------

$("#sub-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const topic = $("#sub-topic").value.trim();
  const server_id = $("#sub-server").value;
  if (config.subscriptions.some((s) => s.topic === topic && s.server_id === server_id)) {
    toast("Already subscribed to that topic");
    return;
  }
  $("#sub-topic").value = "";
  save({
    ...config,
    subscriptions: [...config.subscriptions, { id: crypto.randomUUID(), server_id, topic, enabled: true }],
  });
});

// ---------- servers ----------

const serverForm = $("#server-form");
$("#server-auth").addEventListener("change", (e) => (serverForm.dataset.auth = e.target.value));

function resetServerForm() {
  serverForm.reset();
  $("#server-id").value = "";
  serverForm.dataset.auth = "none";
  $("#server-form-title").textContent = "Add server";
}

function editServer(s) {
  $("#server-id").value = s.id;
  $("#server-name").value = s.name;
  $("#server-url").value = s.url;
  $("#server-auth").value = s.auth_type;
  $("#server-username").value = s.username;
  $("#server-password").value = s.password;
  $("#server-token").value = s.token;
  serverForm.dataset.auth = s.auth_type;
  $("#server-form-title").textContent = `Edit ${s.name}`;
  $("#server-name").focus();
}

function removeServer(s) {
  save({
    servers: config.servers.filter((x) => x.id !== s.id),
    subscriptions: config.subscriptions.filter((x) => x.server_id !== s.id),
  });
  resetServerForm();
}

$("#server-cancel").addEventListener("click", resetServerForm);

serverForm.addEventListener("submit", (e) => {
  e.preventDefault();
  const id = $("#server-id").value || crypto.randomUUID();
  const server = {
    id,
    name: $("#server-name").value.trim(),
    url: $("#server-url").value.trim().replace(/\/+$/, ""),
    auth_type: $("#server-auth").value,
    username: $("#server-username").value,
    password: $("#server-password").value,
    token: $("#server-token").value.trim(),
  };
  const exists = config.servers.some((s) => s.id === id);
  save({
    ...config,
    servers: exists ? config.servers.map((s) => (s.id === id ? server : s)) : [...config.servers, server],
  });
  resetServerForm();
  toast(exists ? "Server updated" : "Server added");
});

// ---------- messages ----------

$("#test-notification").addEventListener("click", () =>
  invoke("test_notification").catch((e) => toast(`Notification failed: ${e}`))
);

$("#clear-messages").addEventListener("click", async () => {
  await invoke("clear_messages");
  messages = [];
  renderMessages();
});

// ---------- send ----------

$("#send-server").addEventListener("change", renderSendTopics);

$("#send-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const optional = (sel) => $(sel).value.trim() || null;
  const message = {
    topic: $("#send-topic").value.trim(),
    message: $("#send-message").value,
    title: optional("#send-title"),
    priority: Number($("#send-priority").value),
    tags: $("#send-tags").value.split(",").map((t) => t.trim()).filter(Boolean),
    click: optional("#send-click"),
  };
  const button = $("#send-submit");
  button.disabled = true;
  try {
    await invoke("publish", { serverId: $("#send-server").value, message });
    // Keep server/topic for follow-up messages; clear the content.
    for (const sel of ["#send-title", "#send-message", "#send-tags", "#send-click"]) $(sel).value = "";
    $("#send-priority").value = "3";
    toast(`Sent to ${message.topic}`);
  } catch (err) {
    toast(`Send failed: ${err}`);
  } finally {
    button.disabled = false;
  }
});

// ---------- settings ----------

const PERMISSION_TEXT = {
  authorized: "Allowed",
  denied: "Blocked — enable ntfyui in System Settings → Notifications",
  not_determined: "Not decided yet — macOS will ask on the next notification",
  unknown: "Unknown (only checked in the bundled app)",
};

async function refreshPermission() {
  const status = await invoke("notification_permission").catch(() => "unknown");
  const node = $("#notif-permission");
  node.textContent = PERMISSION_TEXT[status] ?? status;
  node.className = status === "denied" ? "err" : "meta";
}

$("#open-notif-settings").addEventListener("click", () => invoke("open_notification_settings"));
// Re-check when coming back from System Settings.
window.addEventListener("focus", refreshPermission);

$("#autostart").addEventListener("change", async (e) => {
  try {
    await invoke("set_autostart", { enabled: e.target.checked });
    toast(e.target.checked ? "ntfyui will launch at login" : "Launch at login disabled");
  } catch (err) {
    e.target.checked = !e.target.checked;
    toast(`Failed: ${err}`);
  }
});

// ---------- init ----------

listen("message", ({ payload }) => {
  messages.unshift(payload);
  messages = messages.slice(0, 200);
  renderMessages();
});

listen("status", ({ payload }) => {
  statuses[payload.id] = payload;
  renderSubscriptions();
});

(async () => {
  resetServerForm();
  config = await invoke("get_config");
  messages = await invoke("get_messages");
  for (const s of await invoke("get_statuses")) statuses[s.id] = s;
  $("#autostart").checked = await invoke("get_autostart").catch(() => false);
  refreshPermission();
  render();
})();
