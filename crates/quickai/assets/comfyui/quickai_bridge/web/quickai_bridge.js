// QuickAI's bridge, the page's half (the other is __init__.py).
//
// ComfyUI's page asks a host program to fetch a model that a workflow needs ("Download" in its list of
// missing models) by calling window.__comfyDesktop2.downloadModel(url, name, directory), which ComfyUI Desktop
// supplies. In a browser nothing does, and the page saves the file with the browser, in its Downloads folder.
// This supplies it: the file is fetched by ComfyUI itself, into the models folder, and what it did is told in a
// small panel. What the bridge will not fetch (an address that is not the hub's, a file that is not
// safetensors or GGUF, a file that wants a login) is left to the browser, as the page would have done.

import { api } from "../../scripts/api.js";
import { app } from "../../scripts/app.js";

const ROUTE = "/quickai/downloads";
const POLL_MS = 1000;
const LINGER_MS = 8000; // how long what finished stays in the panel; a failure stays until it is closed

const rows = new Map(); // id -> { view, element, closed }
let panel = null;
let timer = null;
const announced = new Set(); // ids whose end the page has been told of

function size(bytes) {
  if (bytes < 1000) return `${bytes} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = "bytes";
  for (const next of units) {
    if (value < 1000) break;
    value /= 1000;
    unit = next;
  }
  return `${value.toFixed(value < 10 ? 2 : 1)} ${unit}`;
}

function span(seconds) {
  if (seconds < 90) return `${Math.max(1, Math.round(seconds))} s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  return `${(seconds / 3600).toFixed(1)} h`;
}

function describe(view) {
  switch (view.state) {
    case "queued":
      return "waiting for the one before it";
    case "checking":
      return `reading what was fetched before (${size(view.received)} of ${size(view.total)})`;
    case "running": {
      if (!view.total) return `${size(view.received)}`;
      const part = Math.floor((100 * view.received) / view.total);
      let text = `${part}% · ${size(view.received)} of ${size(view.total)}`;
      if (view.speed > 0) {
        text += ` · ${size(view.speed)}/s · about ${span((view.total - view.received) / view.speed)} left`;
      }
      return text;
    }
    case "done":
      return `in place, in the ${view.directory} folder`;
    case "exists":
      return `already in the ${view.directory} folder`;
    case "cancelled":
      return "stopped";
    default:
      return view.error || "it did not work";
  }
}

const active = (view) => ["queued", "running", "checking"].includes(view.state);

function ensurePanel() {
  if (panel) return panel;
  panel = document.createElement("div");
  panel.id = "quickai-downloads";
  panel.setAttribute("role", "status");
  panel.setAttribute("aria-live", "polite");
  Object.assign(panel.style, {
    position: "fixed",
    left: "80px",
    bottom: "16px",
    zIndex: "10000",
    width: "380px",
    maxWidth: "calc(100vw - 96px)",
    padding: "10px 12px",
    borderRadius: "8px",
    border: "1px solid var(--border-color, #4e4e4e)",
    background: "var(--comfy-menu-bg, #202020)",
    color: "var(--fg-color, #ddd)",
    font: "13px/1.4 system-ui, sans-serif",
    boxShadow: "0 4px 16px rgba(0, 0, 0, 0.45)",
    display: "none",
  });
  const title = document.createElement("div");
  title.textContent = "Models, fetched by ComfyUI";
  Object.assign(title.style, { fontWeight: "600", marginBottom: "6px" });
  panel.append(title);
  document.body.append(panel);
  return panel;
}

function build(view) {
  const element = document.createElement("div");
  element.style.margin = "6px 0";
  const head = document.createElement("div");
  Object.assign(head.style, { display: "flex", gap: "8px", alignItems: "baseline" });
  const name = document.createElement("span");
  Object.assign(name.style, { flex: "1", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" });
  const close = document.createElement("button");
  close.type = "button";
  Object.assign(close.style, {
    border: "0",
    background: "transparent",
    color: "inherit",
    cursor: "pointer",
    fontSize: "15px",
    lineHeight: "1",
    padding: "0 2px",
  });
  head.append(name, close);
  const bar = document.createElement("div");
  Object.assign(bar.style, { height: "4px", margin: "4px 0", borderRadius: "2px", background: "rgba(128, 128, 128, 0.3)" });
  const fill = document.createElement("div");
  Object.assign(fill.style, { height: "100%", width: "0%", borderRadius: "2px", background: "var(--p-primary-color, #4a9eff)" });
  bar.append(fill);
  const text = document.createElement("div");
  Object.assign(text.style, { opacity: "0.8", wordBreak: "break-word" });
  element.append(head, bar, text);
  return { element, name, close, bar, fill, text };
}

function show(view) {
  const where = ensurePanel();
  let row = rows.get(view.id);
  if (!row) {
    row = { ...build(view), closed: false };
    rows.set(view.id, row);
    where.append(row.element);
    row.close.addEventListener("click", () => {
      if (active(row.view)) {
        api.fetchApi(`${ROUTE}/${row.view.id}`, { method: "DELETE" }).catch(() => {});
      } else {
        remove(row.view.id);
      }
    });
  }
  row.view = view;
  row.name.textContent = view.name;
  row.close.textContent = "×";
  row.close.setAttribute("aria-label", active(view) ? `Stop fetching ${view.name}` : `Close the note about ${view.name}`);
  row.close.title = active(view) ? "Stop" : "Close";
  row.fill.style.width = view.total ? `${Math.floor((100 * view.received) / view.total)}%` : "0%";
  row.bar.style.display = active(view) ? "block" : "none";
  row.fill.style.background = view.state === "checking" ? "#c9a227" : "var(--p-primary-color, #4a9eff)";
  row.text.textContent = describe(view);
  row.text.style.color = view.state === "failed" ? "var(--error-text, #ff6b6b)" : "inherit";
  where.style.display = "block";
}

function remove(id) {
  const row = rows.get(id);
  if (!row) return;
  row.element.remove();
  rows.delete(id);
  if (rows.size === 0 && panel) panel.style.display = "none";
}

async function refreshModels() {
  // what the "r" key does: the lists of models in the nodes, and the list of what is missing, are read again
  try {
    await app.extensionManager.command.execute("Comfy.RefreshNodeDefinitions");
  } catch (error) {
    try {
      await app.refreshComboInNodes();
    } catch (ignored) {
      console.warn("[QuickAI] could not refresh the lists of models", error);
    }
  }
}

function ended(view) {
  if (announced.has(view.id)) return;
  announced.add(view.id);
  if (view.state === "done" || view.state === "exists") refreshModels();
  if (view.state === "done" || view.state === "exists" || view.state === "cancelled") {
    setTimeout(() => remove(view.id), LINGER_MS);
  }
}

async function poll() {
  let views;
  try {
    const reply = await api.fetchApi(ROUTE);
    if (!reply.ok) throw new Error(reply.statusText);
    views = await reply.json();
  } catch (error) {
    console.warn("[QuickAI] could not read the downloads", error);
    return;
  }
  for (const view of views) {
    if (active(view) || rows.has(view.id)) show(view);
    if (!active(view) && rows.has(view.id)) ended(view);
  }
  if (!views.some(active)) stopPolling();
}

function startPolling() {
  if (timer === null) timer = setInterval(poll, POLL_MS);
}

function stopPolling() {
  if (timer !== null) clearInterval(timer);
  timer = null;
}

function browserDownload(url, name) {
  // what the page does when there is no host program: the browser saves it, in its own folder
  let scheme = "";
  try {
    scheme = new URL(url).protocol;
  } catch (ignored) {
    return;
  }
  if (scheme !== "https:" && scheme !== "http:") return;
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  link.click();
}

let notes = 0;
function note(name, text, stays) {
  const view = { id: `note-${notes++}`, name, directory: "", state: stays ? "failed" : "cancelled", received: 0, total: 0, speed: 0, error: text };
  show(view);
  rows.get(view.id).text.textContent = text;
  if (!stays) setTimeout(() => remove(view.id), LINGER_MS * 2);
}

async function downloadModel(url, name, directory) {
  let reply;
  let body;
  try {
    reply = await api.fetchApi(ROUTE, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url, name, directory }),
    });
    body = await reply.json();
  } catch (error) {
    note(name, `ComfyUI could not be asked to fetch it (${error}). The browser will save it instead.`);
    browserDownload(url, name);
    return;
  }
  if (reply.ok) {
    show(body);
    if (active(body)) startPolling();
    else ended(body);
    return;
  }
  if (body.browser) {
    note(name, `${body.error}. The browser will save it instead, in its own folder.`);
    browserDownload(url, name);
  } else {
    note(name, body.error || "it did not work", true);
  }
}

// Only where no host program is: ComfyUI Desktop has its own, and keeps it.
if (!window.__comfyDesktop2?.downloadModel) {
  window.__comfyDesktop2 = Object.assign(window.__comfyDesktop2 || {}, {
    isRemote: () => false,
    downloadModel,
  });
}

// A page that is opened (or opened again) while something is being fetched shows it.
poll().then(() => {
  if (rows.size > 0) startPolling();
});
