// encchat web client.
//
// This file does no cryptography. Every operation that touches a key goes
// through the WASM module, which is a thin wrapper around `encchat-core` — the
// same code the CLI runs. The browser is only responsible for transport
// (WebSocket), the UI, and holding the *encrypted* local state.

import init, {
  WasmClient,
  decode_server_frame,
  encode_relay,
  generate_mnemonic,
  new_salt,
  open_state,
  room_fingerprint,
} from "../pkg/encchat_wasm.js";
import {
  DEFAULT_APPEARANCE,
  DEFAULT_COLOR_FALLBACKS,
  applyAppearance,
  appearancePresets,
  colorToCss,
  formatTimestamp,
  loadAliases,
  loadAppearance,
  loadOwnNick,
  peerColor,
  sanitizeLabel,
  saveAliases,
  saveAppearance,
  saveOwnNick,
  type Appearance,
} from "./appearance.js";
import {
  COMMANDS,
  completeCommand,
  filterCommands,
  findCommand,
  parseCommand,
  type CommandSpec,
} from "./commands.js";
import { SettingsPage, type SettingsContext } from "./settings-page.js";
import "./style.css";

// --- Types mirroring the WASM views -----------------------------------------

interface DeviceView {
  id: string;
  mac: string;
}

type ServerFrame =
  | { kind: "joined"; room: string; devices: DeviceView[] }
  | { kind: "devices"; devices: DeviceView[] }
  | { kind: "deliver"; from: string; payload: Uint8Array }
  | { kind: "error"; code: string; message: string }
  | { kind: "pong" };

type Action =
  | { kind: "send"; to: string; data: Uint8Array }
  | { kind: "received"; from: string; msgId: string; sentAt: number; body: string }
  | { kind: "handshake"; with: string }
  | { kind: "notice"; message: string }
  | { kind: "nick"; from: string; nick: string };

interface Outgoing {
  to: string;
  data: Uint8Array;
}

interface PeerNick {
  device: string;
  nick: string;
}

interface SavedState {
  salt: string;
  blob: string;
}

type ConnectionState = "idle" | "connecting" | "online" | "offline";

// --- Element handles --------------------------------------------------------

function el<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node as T;
}

const serverUrlInput = el<HTMLInputElement>("serverUrl");
const mnemonicInput = el<HTMLTextAreaElement>("mnemonic");
const passphraseInput = el<HTMLInputElement>("passphrase");
const rememberCheckbox = el<HTMLInputElement>("remember");
const connectButton = el<HTMLButtonElement>("connect");
const disconnectButton = el<HTMLButtonElement>("disconnect");
const generateButton = el<HTMLButtonElement>("generate");
const statusEl = el<HTMLElement>("status");
const deviceCountEl = el<HTMLElement>("deviceCount");
const fingerprintEl = el<HTMLElement>("fingerprint");
const scrollbackEl = el<HTMLElement>("scrollback");
const sendForm = el<HTMLFormElement>("sendForm");
const messageInput = el<HTMLInputElement>("message");
const sendButton = el<HTMLButtonElement>("send");
const peopleList = el<HTMLElement>("peopleList");
const jumpLatest = el<HTMLButtonElement>("jumpLatest");
const composerHint = el<HTMLElement>("composerHint");
const settingsButton = el<HTMLButtonElement>("settingsButton");
const chatView = el<HTMLElement>("chatView");
const settingsView = el<HTMLElement>("settingsView");
const toasts = el<HTMLElement>("toasts");
const statusBar = el<HTMLElement>("statusBar");
const connDot = el<HTMLElement>("connDot");
const statusText = el<HTMLElement>("statusText");
const statusRoom = el<HTMLElement>("statusRoom");
const statusPeers = el<HTMLElement>("statusPeers");
const statusSessions = el<HTMLElement>("statusSessions");
const statusClock = el<HTMLElement>("statusClock");
const connectToggle = el<HTMLButtonElement>("connectToggle");
const connectToggleLabel = el<HTMLElement>("connectToggleLabel");
const connectDot = el<HTMLElement>("connectDot");
const connectHeading = el<HTMLElement>("connect-heading");
const connectDrawer = el<HTMLElement>("connectDrawer");
const connectClose = el<HTMLButtonElement>("connectClose");
const connectScrim = el<HTMLElement>("connectScrim");
const commandMenu = el<HTMLElement>("commandMenu");
const commandList = el<HTMLUListElement>("commandList");
const commandCount = el<HTMLElement>("commandCount");
const commandToggle = el<HTMLButtonElement>("commandToggle");

// --- Client state -----------------------------------------------------------

const STORAGE_STATE = "encchat.state.v1";
const STORAGE_SERVER = "encchat.serverUrl";

let client: WasmClient | null = null;
let socket: WebSocket | null = null;
let peers = new Map<string, DeviceView>();
// Peers we have started a handshake with but do not yet have a session for.
let handshaking = new Set<string>();
let peerNicks = new Map<string, string>();
let ownNick = loadOwnNick();
let aliases = loadAliases();
let appearance: Appearance = DEFAULT_APPEARANCE;
let settingsPage: SettingsPage | null = null;
let unread = 0;
let clockTimer: number | null = null;
let drawerOpen = false;
let drawerReturnFocus: HTMLElement | null = null;
let connectionState: ConnectionState = "idle";
let commandMenuOpen = false;
let commandMatches: CommandSpec[] = [];
let commandIndex = 0;

// Paint the fallback look immediately; the saved appearance is applied after
// the WASM module initializes (it needs `appearance_defaults`).
applyAppearance(appearance);

// --- Small helpers ----------------------------------------------------------

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function fromBase64(text: string): Uint8Array {
  const binary = atob(text);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

function shortId(hex: string): string {
  return hex.slice(0, 8);
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function setStatus(text: string, tone: "info" | "ok" | "warn" | "error" = "info"): void {
  statusEl.textContent = text;
  statusEl.dataset.tone = tone;
}

function setDeviceCount(count: number): void {
  deviceCountEl.textContent = String(count);
}

const CONNECTION_LABELS: Record<ConnectionState, string> = {
  idle: "Idle",
  connecting: "Connecting…",
  online: "Online",
  offline: "Offline",
};

function setConnection(state: ConnectionState): void {
  connectionState = state;
  connDot.dataset.state = state;
  connectDot.dataset.state = state;
  statusText.textContent = CONNECTION_LABELS[state];
  updateStatusBar();
}

function toast(text: string): void {
  const item = document.createElement("div");
  item.className = "toast";
  item.textContent = text;
  toasts.append(item);
  window.setTimeout(() => item.remove(), 3200);
}

function ownLabel(): string {
  const nick = ownNick.trim();
  if (nick) return nick;
  const fallback = appearance.labels.own_fallback.trim();
  return fallback || "me";
}

function peerLabel(device: string): string {
  const alias = aliases[device]?.trim();
  if (alias) return alias;
  const nick = peerNicks.get(device);
  if (nick && appearance.labels.show_peer_nicks) return nick;
  if (appearance.labels.show_device_ids) return shortId(device);
  return "peer";
}

/** Label for the People list, respecting the same nickname/device switches. */
function peopleLabel(device: string): string {
  return peerLabel(device);
}

function peerColorCss(device: string): string {
  return colorToCss(
    peerColor(appearance.theme.peer_palette, device),
    DEFAULT_COLOR_FALLBACKS.incoming_nick,
  );
}

// --- Scrollback -------------------------------------------------------------

function clearEmptyState(): void {
  const empty = scrollbackEl.querySelector(".empty");
  if (empty) empty.remove();
}

function atBottom(): boolean {
  return scrollbackEl.scrollHeight - scrollbackEl.scrollTop - scrollbackEl.clientHeight < 40;
}

function scrollToBottom(): void {
  scrollbackEl.scrollTop = scrollbackEl.scrollHeight;
}

function showUnread(): void {
  if (!appearance.layout.show_unread) {
    jumpLatest.hidden = true;
    return;
  }
  jumpLatest.textContent =
    unread === 1 ? "1 new message · jump to latest" : `${unread} new messages · jump to latest`;
  jumpLatest.hidden = false;
}

function bumpUnread(): void {
  if (!appearance.layout.show_unread) return;
  unread += 1;
  showUnread();
}

function refreshMessageMeta(wrap: HTMLElement): void {
  const outgoing = wrap.classList.contains("outgoing");
  const device = wrap.dataset.device ?? "";
  const who = wrap.querySelector<HTMLElement>(".who");
  const when = wrap.querySelector<HTMLElement>(".when");
  const sentAt = Number(wrap.dataset.sentAt ?? Date.now());
  const date = new Date(sentAt);

  if (who) {
    who.textContent = outgoing ? ownLabel() : peerLabel(device);
    who.style.color = "";
    if (!outgoing && appearance.layout.peer_colors) {
      who.style.color = peerColorCss(device);
    }
  }
  if (when) {
    when.textContent = appearance.timestamps.enabled
      ? formatTimestamp(appearance.timestamps.format, date)
      : "";
    when.title = date.toLocaleString();
  }
}

function refreshSystemMeta(line: HTMLElement): void {
  const when = line.querySelector<HTMLElement>(".when");
  if (!when) return;
  const sentAt = Number(line.dataset.sentAt ?? Date.now());
  const date = new Date(sentAt);
  const show = appearance.timestamps.enabled && appearance.timestamps.system;
  when.textContent = show ? formatTimestamp(appearance.timestamps.format, date) : "";
  when.title = date.toLocaleString();
}

function refreshMessageLabels(): void {
  scrollbackEl.querySelectorAll<HTMLElement>(".msg").forEach(refreshMessageMeta);
  scrollbackEl.querySelectorAll<HTMLElement>(".system").forEach(refreshSystemMeta);
}

function appendSystem(text: string): void {
  clearEmptyState();
  const stick = appearance.layout.auto_scroll || atBottom();
  const line = document.createElement("p");
  line.className = "system";
  line.dataset.sentAt = String(Date.now());
  const when = document.createElement("time");
  when.className = "when";
  const body = document.createElement("span");
  body.className = "system-text";
  body.textContent = text;
  line.append(when, body);
  refreshSystemMeta(line);
  scrollbackEl.append(line);
  if (stick) scrollToBottom();
}

function appendMessage(options: {
  outgoing: boolean;
  from: string;
  sentAt: number;
  body: string;
}): void {
  clearEmptyState();
  const stick = appearance.layout.auto_scroll || atBottom();

  const wrap = document.createElement("div");
  wrap.className = options.outgoing ? "msg outgoing" : "msg incoming";
  wrap.dataset.device = options.from;
  wrap.dataset.sentAt = String(options.sentAt);

  const meta = document.createElement("div");
  meta.className = "meta";
  const who = document.createElement("span");
  who.className = "who";
  const when = document.createElement("time");
  when.className = "when";
  meta.append(who, when);

  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.textContent = options.body;

  wrap.append(meta, bubble);
  refreshMessageMeta(wrap);
  scrollbackEl.append(wrap);

  if (stick) {
    scrollToBottom();
  } else if (!options.outgoing) {
    bumpUnread();
  }
}

function updateClock(): void {
  const now = new Date();
  statusClock.textContent = `${pad2(now.getHours())}:${pad2(now.getMinutes())}`;
}

function updateStatusBar(): void {
  statusBar.hidden = !appearance.layout.show_status_bar;
  statusPeers.textContent = String(peers.size);
  statusSessions.textContent = String(client ? client.session_count() : 0);
  const room = fingerprintEl.textContent?.trim();
  statusRoom.textContent = room && room !== "—" ? room : "—";
  updateClock();
}

function renderPeople(): void {
  peopleList.replaceChildren();
  if (!client || peers.size === 0) {
    const empty = document.createElement("li");
    empty.className = "people-empty";
    empty.textContent = connectionIsOpen()
      ? "No peers in the room yet."
      : "Connect to see peers.";
    peopleList.append(empty);
    return;
  }

  for (const id of [...peers.keys()].sort()) {
    const item = document.createElement("li");
    item.className = "person";

    const dot = document.createElement("span");
    dot.className = "person-dot";
    dot.style.background = peerColorCss(id);

    const copy = document.createElement("span");
    copy.className = "person-copy";
    const label = document.createElement("span");
    label.className = "person-label";
    label.textContent = peopleLabel(id);
    const meta = document.createElement("span");
    meta.className = "person-meta";
    const hasSession = client.has_session(id);
    meta.textContent = `${shortId(id)} · ${hasSession ? "session" : "awaiting session"}`;
    copy.append(label, meta);

    item.append(dot, copy);
    peopleList.append(item);
  }
}

/** Re-read appearance-dependent chrome without touching message history. */
function syncChrome(): void {
  statusBar.hidden = !appearance.layout.show_status_bar;
  composerHint.hidden = !appearance.layout.show_hints;
  if (!appearance.layout.show_unread) {
    unread = 0;
    jumpLatest.hidden = true;
  } else if (unread > 0) {
    showUnread();
  }
  refreshMessageLabels();
  renderPeople();
  updateStatusBar();
}

function setAppearance(next: Appearance): void {
  appearance = next;
  applyAppearance(appearance);
  syncChrome();
}

scrollbackEl.addEventListener("scroll", () => {
  if (!atBottom()) return;
  unread = 0;
  jumpLatest.hidden = true;
});

jumpLatest.addEventListener("click", () => {
  unread = 0;
  jumpLatest.hidden = true;
  scrollToBottom();
});

// --- Slash commands ---------------------------------------------------------

function commandOptionId(index: number): string {
  return `command-option-${index}`;
}

/** Text after the leading slash; used as the palette filter. */
function commandQuery(value: string): string {
  return value.slice(1);
}

/** `/name` (or `/`) with no whitespace yet is palette territory. */
function isCommandFilter(value: string): boolean {
  return value.startsWith("/") && !/\s/.test(value.slice(1));
}

function updateActiveDescendant(): void {
  const selected = commandMenuOpen ? commandMatches[commandIndex] : undefined;
  if (selected) {
    messageInput.setAttribute("aria-activedescendant", commandOptionId(commandIndex));
  } else {
    messageInput.removeAttribute("aria-activedescendant");
  }
}

function buildCommandRow(command: CommandSpec, index: number): HTMLLIElement {
  const row = document.createElement("li");
  row.id = commandOptionId(index);
  row.className = "command-option";
  row.setAttribute("role", "option");
  row.setAttribute("aria-selected", String(index === commandIndex));

  const line = document.createElement("span");
  line.className = "command-line";
  const name = document.createElement("span");
  name.className = "command-name";
  name.textContent = `/${command.name}`;
  line.append(name);
  if (command.args) {
    const args = document.createElement("span");
    args.className = "command-args";
    args.textContent = command.args;
    line.append(args);
  }

  const description = document.createElement("span");
  description.className = "command-desc";
  description.textContent = command.description;

  row.append(line, description);

  // Keep focus in the composer while the pointer is over the palette.
  row.addEventListener("pointerdown", (event) => event.preventDefault());
  row.addEventListener("mousedown", (event) => event.preventDefault());
  row.addEventListener("pointerenter", () => selectCommand(index));
  row.addEventListener("click", () => activateCommand(index));
  return row;
}

function renderCommandMenu(): void {
  commandMatches = filterCommands(commandQuery(messageInput.value));
  if (commandIndex >= commandMatches.length) commandIndex = 0;

  const fragment = document.createDocumentFragment();
  commandMatches.forEach((command, index) => {
    fragment.append(buildCommandRow(command, index));
  });
  if (commandMatches.length === 0) {
    const empty = document.createElement("li");
    empty.className = "command-empty";
    empty.setAttribute("role", "presentation");
    empty.textContent = "No matching commands";
    fragment.append(empty);
  }

  commandList.replaceChildren(fragment);
  commandCount.textContent = commandMatches.length === 0 ? "" : String(commandMatches.length);
  commandMenu.hidden = false;
  commandMenuOpen = true;
  messageInput.setAttribute("aria-expanded", "true");
  commandToggle.setAttribute("aria-expanded", "true");
  updateActiveDescendant();
}

function closeCommandMenu(): void {
  commandMenuOpen = false;
  commandMatches = [];
  commandIndex = 0;
  commandMenu.hidden = true;
  messageInput.setAttribute("aria-expanded", "false");
  commandToggle.setAttribute("aria-expanded", "false");
  messageInput.removeAttribute("aria-activedescendant");
}

function selectCommand(index: number, scroll = false): void {
  if (commandMatches.length === 0) return;
  commandIndex =
    ((index % commandMatches.length) + commandMatches.length) % commandMatches.length;
  commandList.querySelectorAll<HTMLElement>(".command-option").forEach((row, rowIndex) => {
    row.setAttribute("aria-selected", String(rowIndex === commandIndex));
  });
  updateActiveDescendant();
  if (scroll) {
    commandList
      .querySelector<HTMLElement>(`#${commandOptionId(commandIndex)}`)
      ?.scrollIntoView({ block: "nearest" });
  }
}

/** Tab / click completion: `/nick ` for argument-taking commands. */
function completeSelectedCommand(command: CommandSpec): void {
  messageInput.value = completeCommand(command.name, command.takesArgs);
  const end = messageInput.value.length;
  messageInput.setSelectionRange(end, end);
  messageInput.focus({ preventScroll: true });
  if (command.takesArgs) {
    closeCommandMenu();
  } else {
    commandIndex = 0;
    renderCommandMenu();
  }
}

/** Enter / click activation: run argument-free commands, complete the rest. */
function activateCommand(index: number): void {
  const command = commandMatches[index];
  if (!command) return;
  if (command.takesArgs) {
    completeSelectedCommand(command);
    return;
  }
  executeCommandText(`/${command.name}`);
}

function clearScrollback(): void {
  scrollbackEl.replaceChildren();
  const empty = document.createElement("p");
  empty.className = "empty";
  empty.textContent = "No messages yet.";
  scrollbackEl.append(empty);
  unread = 0;
  jumpLatest.hidden = true;
}

function commandUsage(command: CommandSpec): string {
  return `/${command.name}${command.args ? ` ${command.args}` : ""}`;
}

function helpText(): string {
  const width = Math.max(...COMMANDS.map((command) => commandUsage(command).length));
  const lines = COMMANDS.map(
    (command) => `  ${commandUsage(command).padEnd(width)}  ${command.description}`,
  );
  return [
    "Commands — type / to open the menu, then ↑↓ and Enter.",
    ...lines,
    "Arguments go after a space, e.g. /nick alice.",
  ].join("\n");
}

function shortcutText(): string {
  return [
    "Keyboard shortcuts",
    "  Enter          Send a message, or run the selected command",
    "  /              Open the command menu",
    "  ↑ / ↓          Move through the command menu",
    "  Tab            Complete the selected command",
    "  Esc            Close the command menu",
    "  Esc / backdrop Close the connection drawer",
    "  Ctrl/Cmd + S   Save changes on the settings page",
  ].join("\n");
}

function runCommand(name: string, args: string): void {
  switch (name) {
    case "help":
      appendSystem(helpText());
      break;
    case "clear":
      clearScrollback();
      toast("Conversation cleared.");
      break;
    case "nick": {
      const requested = sanitizeLabel(args);
      if (!requested) {
        const current = ownNick ? `Current display name: ${ownNick}` : "No display name set.";
        appendSystem(
          `Usage: /nick <name>\n${current}\nSet a lasting label in Settings → Labels.`,
        );
        break;
      }
      applySavedNick(requested);
      appendSystem(`Display name set to ${requested}.`);
      break;
    }
    case "who": {
      if (!client) {
        appendSystem("Not connected. Use /connect to join a room.");
        break;
      }
      const lines = [
        `Verified peers: ${peers.size}`,
        `You: ${ownLabel()} · ${shortId(client.device_id_hex())}`,
      ];
      if (peers.size === 0) {
        lines.push("  No peers in the room yet.");
      } else {
        for (const id of [...peers.keys()].sort()) {
          const session = client.has_session(id) ? "session" : "awaiting session";
          lines.push(`  ${shortId(id)} · ${peopleLabel(id)} · ${session}`);
        }
      }
      appendSystem(lines.join("\n"));
      break;
    }
    case "status": {
      const room = fingerprintEl.textContent?.trim() ?? "";
      appendSystem(
        [
          `Connection: ${CONNECTION_LABELS[connectionState]}`,
          `Relay: ${serverUrlInput.value.trim() || "—"}`,
          `Room fingerprint: ${room && room !== "—" ? room : "—"}`,
          `This device: ${client ? client.device_id_hex() : "— (not connected)"}`,
          `Peers: ${peers.size} · Sessions: ${client ? client.session_count() : 0}`,
        ].join("\n"),
      );
      break;
    }
    case "fingerprint": {
      const room = fingerprintEl.textContent?.trim() ?? "";
      appendSystem(
        room && room !== "—"
          ? `Room fingerprint: ${room}`
          : "No room fingerprint yet — enter the shared mnemonic in the connection panel.",
      );
      break;
    }
    case "settings":
      window.location.hash = "#/settings";
      break;
    case "theme": {
      const presets = appearancePresets();
      const names = presets.map((preset) => preset.name);
      const requested = args.trim();
      if (!requested) {
        const list = presets
          .map((preset) => `${preset.name}${preset.name === appearance.theme.name ? " (current)" : ""}`)
          .join(", ");
        appendSystem(`Themes: ${list}\nUsage: /theme <name>`);
        break;
      }
      const preset = presets.find(
        (candidate) => candidate.name.toLowerCase() === requested.toLowerCase(),
      );
      if (!preset) {
        appendSystem(`Unknown theme: ${requested}\nValid themes: ${names.join(", ")}`);
        break;
      }
      setAppearance({ ...appearance, theme: preset });
      saveAppearance(appearance);
      appendSystem(`Theme set to ${preset.name}.`);
      break;
    }
    case "timestamps": {
      const next: Appearance = {
        ...appearance,
        timestamps: { ...appearance.timestamps, enabled: !appearance.timestamps.enabled },
      };
      setAppearance(next);
      saveAppearance(next);
      appendSystem(`Timestamps ${next.timestamps.enabled ? "on" : "off"}.`);
      break;
    }
    case "keys":
      appendSystem(shortcutText());
      break;
    case "connect":
      openDrawer();
      break;
    case "disconnect":
      if (!socket && !client) {
        appendSystem("Not connected.");
        break;
      }
      disconnect();
      appendSystem("Disconnected from the relay.");
      break;
  }
}

function executeCommandText(raw: string): void {
  const parsed = parseCommand(raw);
  closeCommandMenu();
  messageInput.value = "";
  if (!messageInput.disabled) messageInput.focus({ preventScroll: true });
  if (!parsed) {
    appendSystem("Type a command after / (for example /help).");
    return;
  }
  if (!findCommand(parsed.name)) {
    appendSystem(`Unknown command: /${parsed.name} — type / for the list.`);
    return;
  }
  runCommand(parsed.name, parsed.args);
}

/** Enter: run the selected command, complete it, or send normal text. */
function submitComposer(): void {
  const selected = commandMenuOpen ? commandMatches[commandIndex] : undefined;
  if (selected) {
    if (selected.takesArgs) {
      completeSelectedCommand(selected);
      return;
    }
    executeCommandText(`/${selected.name}`);
    return;
  }
  const body = messageInput.value.trim();
  if (!body) return;
  if (body.startsWith("/")) executeCommandText(body);
  else sendMessage();
}

// --- Encrypted local state --------------------------------------------------

function loadSavedState(): SavedState | null {
  const raw = localStorage.getItem(STORAGE_STATE);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<SavedState>;
    if (typeof parsed.salt === "string" && typeof parsed.blob === "string") {
      return { salt: parsed.salt, blob: parsed.blob };
    }
  } catch {
    // Corrupt entry: drop it.
    localStorage.removeItem(STORAGE_STATE);
  }
  return null;
}

function persistState(): void {
  if (!client || !rememberCheckbox.checked) return;
  const passphrase = passphraseInput.value;
  if (!passphrase) return;
  try {
    const salt = new_salt();
    const blob = client.encode_state(passphrase, salt);
    const saved: SavedState = { salt: toBase64(salt), blob: toBase64(blob) };
    localStorage.setItem(STORAGE_STATE, JSON.stringify(saved));
  } catch (error) {
    appendSystem(`Could not save encrypted state: ${messageOf(error)}`);
  }
}

// --- Fingerprint ------------------------------------------------------------

function updateFingerprint(): void {
  const mnemonic = mnemonicInput.value.trim();
  if (!mnemonic) {
    fingerprintEl.textContent = "—";
    updateStatusBar();
    return;
  }
  try {
    fingerprintEl.textContent = room_fingerprint(mnemonic);
  } catch {
    fingerprintEl.textContent = "(invalid mnemonic)";
  }
  updateStatusBar();
}

// --- Transport --------------------------------------------------------------

function sendRelay(to: string, payload: Uint8Array): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) return;
  socket.send(encode_relay(to, payload));
}

function loadPeerNicks(): void {
  peerNicks = new Map();
  if (!client) return;
  try {
    for (const entry of client.peer_nicks() as PeerNick[]) {
      peerNicks.set(entry.device, entry.nick);
    }
  } catch {
    // Older state without nicknames: start with an empty map.
  }
}

function announceNicks(): void {
  if (!client || peers.size === 0) return;
  try {
    const frames = client.announce_nick([...peers.keys()].join(",")) as Outgoing[];
    for (const frame of frames) sendRelay(frame.to, frame.data);
  } catch (error) {
    appendSystem(`Could not announce your nickname: ${messageOf(error)}`);
  }
}

function applySavedNick(nick: string): void {
  ownNick = sanitizeLabel(nick);
  saveOwnNick(ownNick);
  if (client) {
    try {
      client.set_nick(ownNick);
    } catch (error) {
      appendSystem(`Could not set your nickname: ${messageOf(error)}`);
    }
    announceNicks();
  }
  refreshMessageLabels();
  renderPeople();
  settingsPage?.refresh();
}

function onJoined(devices: DeviceView[]): void {
  syncDevices(devices);
  // Restored sessions may already be able to carry our nickname.
  announceNicks();
}

function syncDevices(devices: DeviceView[]): void {
  if (!client) return;
  const self = client.device_id_hex();

  const verified = new Map<string, DeviceView>();
  for (const device of devices) {
    if (device.id === self) continue;
    if (!client.verify_device(device.id, device.mac)) {
      appendSystem(
        `Ignored device ${shortId(device.id)}…: its MAC is not valid for this room.`,
      );
      continue;
    }
    verified.set(device.id, device);
  }
  peers = verified;
  setDeviceCount(devices.length);

  for (const id of verified.keys()) {
    if (client.has_session(id) || handshaking.has(id)) continue;
    // Deterministic initiator election: only the smaller device id starts the
    // handshake, so two devices that see each other at the same time cannot
    // both initiate and collide.
    if (!client.should_initiate(id)) continue;
    try {
      const frame = client.begin_handshake(id);
      handshaking.add(id);
      sendRelay(id, frame);
      appendSystem(`Handshake started with ${shortId(id)}…`);
    } catch {
      // A handshake with this peer is already in flight; ignore.
    }
  }

  renderPeople();
  updateStatusBar();
  settingsPage?.refresh();
}

function handleAction(action: Action): void {
  switch (action.kind) {
    case "send":
      sendRelay(action.to, action.data);
      break;
    case "received":
      appendMessage({
        outgoing: false,
        from: action.from,
        sentAt: action.sentAt,
        body: action.body,
      });
      persistState();
      break;
    case "handshake":
      handshaking.delete(action.with);
      appendSystem(`Secure session established with ${peerLabel(action.with)}…`);
      persistState();
      announceNicks();
      renderPeople();
      updateStatusBar();
      settingsPage?.refresh();
      break;
    case "nick":
      peerNicks.set(action.from, action.nick);
      refreshMessageLabels();
      renderPeople();
      settingsPage?.refresh();
      break;
    case "notice":
      appendSystem(action.message);
      break;
  }
}

function onDeliver(from: string, payload: Uint8Array): void {
  if (!client) return;
  let actions: Action[];
  try {
    actions = client.handle_frame(from, payload) as Action[];
  } catch (error) {
    appendSystem(`Rejected a frame from ${shortId(from)}…: ${messageOf(error)}`);
    return;
  }
  for (const action of actions) handleAction(action);
}

function onServerFrame(frame: ServerFrame): void {
  switch (frame.kind) {
    case "joined":
      setStatus(`Joined room ${shortId(frame.room)}…`, "ok");
      onJoined(frame.devices);
      setConnection("online");
      // Joining is the point where the conversation becomes the whole surface.
      if (drawerOpen) {
        closeDrawer({ restoreFocus: false });
        if (!messageInput.disabled) messageInput.focus({ preventScroll: true });
      }
      break;
    case "devices":
      syncDevices(frame.devices);
      break;
    case "deliver":
      onDeliver(frame.from, frame.payload);
      break;
    case "error":
      appendSystem(`Server error ${frame.code}: ${frame.message}`);
      break;
    case "pong":
      break;
  }
}

// --- Connect drawer ---------------------------------------------------------

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "textarea:not([disabled])",
  "select:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

function connectionIsOpen(): boolean {
  return socket !== null && socket.readyState === WebSocket.OPEN;
}

function drawerFocusables(): HTMLElement[] {
  return Array.from(connectDrawer.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
}

/** The mnemonic is the step that actually unblocks onboarding. */
function focusDrawerStart(): void {
  const target: HTMLElement = connectionIsOpen() ? connectClose : mnemonicInput;
  target.focus({ preventScroll: true });
}

function openDrawer(options: { focus?: boolean } = {}): void {
  if (drawerOpen) {
    if (options.focus !== false) focusDrawerStart();
    return;
  }
  drawerOpen = true;
  const active = document.activeElement;
  drawerReturnFocus =
    active instanceof HTMLElement && active !== document.body ? active : connectToggle;
  connectDrawer.hidden = false;
  connectScrim.hidden = false;
  connectToggle.setAttribute("aria-expanded", "true");
  window.requestAnimationFrame(() => {
    if (!drawerOpen) return;
    connectDrawer.dataset.open = "true";
    connectScrim.dataset.open = "true";
    // Resolve the visible style before moving focus into the panel.
    void connectDrawer.offsetWidth;
    if (options.focus !== false) focusDrawerStart();
  });
}

function closeDrawer(options: { restoreFocus?: boolean } = {}): void {
  if (!drawerOpen) return;
  drawerOpen = false;
  connectDrawer.dataset.open = "false";
  connectScrim.dataset.open = "false";
  connectToggle.setAttribute("aria-expanded", "false");
  const target = drawerReturnFocus;
  drawerReturnFocus = null;
  if (options.restoreFocus === false) return;
  const fallback = target && target.isConnected ? target : connectToggle;
  fallback.focus({ preventScroll: true });
}

function toggleDrawer(): void {
  if (drawerOpen) closeDrawer();
  else openDrawer();
}

function handleDrawerKeydown(event: KeyboardEvent): void {
  if (!drawerOpen) return;
  if (event.key === "Escape") {
    event.preventDefault();
    closeDrawer();
    return;
  }
  if (event.key !== "Tab") return;
  const focusable = drawerFocusables();
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  const inside = active instanceof Node && connectDrawer.contains(active);
  if (event.shiftKey) {
    if (!inside || active === first) {
      event.preventDefault();
      last.focus();
    }
  } else if (!inside || active === last) {
    event.preventDefault();
    first.focus();
  }
}

// --- Connect / disconnect ---------------------------------------------------

function setConnected(connected: boolean): void {
  document.body.dataset.connected = connected ? "true" : "false";
  connectToggleLabel.textContent = connected ? "Connection" : "Connect";
  connectToggle.setAttribute(
    "aria-label",
    connected ? "Open connection details" : "Open connection setup",
  );
  connectHeading.textContent = connected ? "Connection" : "Connect";
  connectButton.disabled = connected;
  disconnectButton.disabled = !connected;
  messageInput.disabled = !connected;
  sendButton.disabled = !connected;
}

function disconnect(reason = "Disconnected."): void {
  if (socket) {
    socket.onopen = null;
    socket.onmessage = null;
    socket.onclose = null;
    socket.onerror = null;
    if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
      socket.close();
    }
  }
  socket = null;
  client = null;
  peers = new Map();
  peerNicks = new Map();
  handshaking = new Set();
  unread = 0;
  closeCommandMenu();
  jumpLatest.hidden = true;
  setDeviceCount(0);
  setConnected(false);
  setStatus(reason, "warn");
  setConnection("offline");
  renderPeople();
  updateStatusBar();
  settingsPage?.refresh();
}

function connect(): void {
  const serverUrl = serverUrlInput.value.trim();
  const mnemonic = mnemonicInput.value.trim();
  const passphrase = passphraseInput.value;

  if (!mnemonic) {
    setStatus("Enter the shared 24-word mnemonic first.", "error");
    return;
  }
  if (!/^wss?:\/\//i.test(serverUrl)) {
    setStatus("Relay URL must start with ws:// or wss://", "error");
    return;
  }

  let created: WasmClient;
  try {
    const saved = loadSavedState();
    if (saved && passphrase) {
      try {
        created = open_state(mnemonic, passphrase, fromBase64(saved.salt), fromBase64(saved.blob));
        appendSystem("Restored saved encrypted device state.");
      } catch (error) {
        appendSystem(`Could not restore saved state (${messageOf(error)}); using a new device.`);
        created = new WasmClient(mnemonic);
      }
    } else {
      created = new WasmClient(mnemonic);
    }
  } catch (error) {
    // A bad mnemonic (wrong word, wrong count, bad checksum) lands here.
    setStatus(`Could not use that mnemonic: ${messageOf(error)}`, "error");
    return;
  }
  client = created;
  peers = new Map();
  peerNicks = new Map();
  handshaking = new Set();
  unread = 0;
  jumpLatest.hidden = true;

  // Restore the advertised nickname: local preference wins, otherwise adopt
  // whatever the encrypted state remembered.
  loadPeerNicks();
  if (ownNick) {
    try {
      created.set_nick(ownNick);
    } catch {
      // An invalid stored nick simply falls back to `labels.own_fallback`.
    }
  } else {
    const restored = created.self_nick();
    if (restored) {
      ownNick = sanitizeLabel(restored);
      saveOwnNick(ownNick);
    }
  }

  setDeviceCount(0);
  renderPeople();
  localStorage.setItem(STORAGE_SERVER, serverUrl);

  appendSystem(`This device: ${created.device_id_hex()}`);
  setStatus(`Connecting to ${serverUrl}…`, "info");
  setConnection("connecting");

  let ws: WebSocket;
  try {
    ws = new WebSocket(serverUrl);
  } catch (error) {
    setStatus(`Invalid relay URL: ${messageOf(error)}`, "error");
    client = null;
    setConnection("offline");
    return;
  }
  ws.binaryType = "arraybuffer";
  socket = ws;
  setConnected(true);

  ws.onopen = () => {
    if (socket !== ws || !client) return;
    setStatus("Connected. Joining room…", "ok");
    ws.send(client.join_frame());
  };

  ws.onmessage = (event: MessageEvent) => {
    if (socket !== ws) return;
    if (!(event.data instanceof ArrayBuffer)) return;
    let frame: ServerFrame;
    try {
      frame = decode_server_frame(new Uint8Array(event.data)) as ServerFrame;
    } catch (error) {
      appendSystem(`Could not decode a server frame: ${messageOf(error)}`);
      return;
    }
    onServerFrame(frame);
  };

  ws.onclose = () => {
    if (socket !== ws) return;
    disconnect("Connection closed.");
  };

  ws.onerror = () => {
    if (socket !== ws) return;
    setStatus("WebSocket error. Is the relay running?", "error");
  };
}

// --- Send -------------------------------------------------------------------

function sendMessage(): void {
  if (!client || !socket || socket.readyState !== WebSocket.OPEN) {
    setStatus("Not connected.", "warn");
    return;
  }
  const body = messageInput.value.trim();
  if (!body) return;

  const recipients = [...peers.keys()];
  if (recipients.length === 0) {
    appendSystem("No peers in the room yet — wait for them to join.");
    return;
  }

  let outgoing: Outgoing[];
  try {
    outgoing = client.send_text(recipients.join(","), body, Date.now()) as Outgoing[];
  } catch (error) {
    appendSystem(`Cannot send yet: ${messageOf(error)}`);
    return;
  }

  for (const item of outgoing) sendRelay(item.to, item.data);
  appendMessage({ outgoing: true, from: client.device_id_hex(), sentAt: Date.now(), body });
  messageInput.value = "";
  persistState();
}

// --- Routing ----------------------------------------------------------------

function updateRoute(): void {
  const showSettings = window.location.hash === "#/settings";
  document.body.dataset.view = showSettings ? "settings" : "chat";
  chatView.hidden = showSettings;
  settingsView.hidden = !showSettings;
  if (showSettings) closeCommandMenu();
  if (showSettings && drawerOpen) closeDrawer({ restoreFocus: false });
  settingsButton.setAttribute("aria-pressed", String(showSettings));
  settingsButton.setAttribute(
    "aria-label",
    showSettings ? "Close appearance settings" : "Appearance settings",
  );
  if (showSettings) settingsPage?.show();
  else settingsPage?.hide();
}

// --- Bootstrap --------------------------------------------------------------

function createSettingsPage(): void {
  settingsPage = new SettingsPage(settingsView, {
    context: (): SettingsContext => ({
      connected: socket?.readyState === WebSocket.OPEN,
      selfId: client ? client.device_id_hex() : null,
      selfNick: ownNick,
      peers: [...peers.keys()].map((id) => ({
        id,
        session: client ? client.has_session(id) : false,
        nick: peerNicks.get(id) ?? "",
      })),
      aliases: { ...aliases },
    }),
    onDraft: (draft) => setAppearance(draft),
    onSaved: (saved, nick) => {
      setAppearance(saved);
      applySavedNick(nick);
    },
    onAliasChange: (device, alias) => {
      const clean = sanitizeLabel(alias);
      if (clean) aliases[device] = clean;
      else delete aliases[device];
      saveAliases(aliases);
      refreshMessageLabels();
      renderPeople();
    },
    toast,
    navigateChat: () => {
      if (window.location.hash === "#/settings") window.location.hash = "#/";
      else updateRoute();
    },
  });
}

async function bootstrap(): Promise<void> {
  try {
    await init();
  } catch (error) {
    setStatus(`Failed to load the WASM module: ${messageOf(error)}`, "error");
    return;
  }

  const loaded = loadAppearance();
  setAppearance(loaded.appearance);
  if (loaded.notice) {
    appendSystem(loaded.notice);
    toast(loaded.notice);
  }

  const savedServer = localStorage.getItem(STORAGE_SERVER);
  if (savedServer) serverUrlInput.value = savedServer;
  if (loadSavedState()) {
    appendSystem("Saved encrypted device state found. Enter your mnemonic and passphrase to restore it.");
  }

  updateFingerprint();
  setConnection("idle");
  renderPeople();
  updateStatusBar();

  mnemonicInput.addEventListener("input", updateFingerprint);

  generateButton.addEventListener("click", () => {
    try {
      mnemonicInput.value = generate_mnemonic();
      updateFingerprint();
    } catch (error) {
      setStatus(`Could not generate a mnemonic: ${messageOf(error)}`, "error");
    }
  });

  connectButton.addEventListener("click", connect);
  disconnectButton.addEventListener("click", () => disconnect());

  connectToggle.addEventListener("click", toggleDrawer);
  connectClose.addEventListener("click", () => closeDrawer());
  connectScrim.addEventListener("click", () => closeDrawer());
  connectDrawer.addEventListener("keydown", handleDrawerKeydown);
  document.addEventListener("keydown", (event) => {
    if (!drawerOpen || event.key !== "Escape") return;
    event.preventDefault();
    closeDrawer();
  });

  sendForm.addEventListener("submit", (event) => {
    event.preventDefault();
    submitComposer();
  });

  messageInput.addEventListener("input", () => {
    if (isCommandFilter(messageInput.value)) {
      commandIndex = 0;
      renderCommandMenu();
    } else {
      closeCommandMenu();
    }
  });

  messageInput.addEventListener("keydown", (event) => {
    if (!commandMenuOpen) return;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      selectCommand(commandIndex + (event.key === "ArrowDown" ? 1 : -1), true);
      return;
    }
    if (event.key === "Tab") {
      if (commandMatches.length > 0) {
        event.preventDefault();
        completeSelectedCommand(commandMatches[commandIndex]);
      }
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeCommandMenu();
    }
  });

  commandToggle.addEventListener("click", () => {
    if (messageInput.disabled) {
      openDrawer();
      return;
    }
    messageInput.focus({ preventScroll: true });
    if (!messageInput.value.trim()) messageInput.value = "/";
    if (isCommandFilter(messageInput.value)) {
      const end = messageInput.value.length;
      messageInput.setSelectionRange(end, end);
      commandIndex = 0;
      renderCommandMenu();
    } else {
      messageInput.select();
    }
  });

  settingsButton.addEventListener("click", () => {
    window.location.hash = window.location.hash === "#/settings" ? "#/" : "#/settings";
  });

  createSettingsPage();
  window.addEventListener("hashchange", updateRoute);
  updateRoute();

  // Onboarding: show the connection panel on first load unless the user
  // landed directly on the settings route.
  if (window.location.hash !== "#/settings") openDrawer();

  clockTimer = window.setInterval(() => {
    if (!statusBar.hidden) updateClock();
  }, 10_000);

  window.addEventListener("beforeunload", () => {
    persistState();
    if (clockTimer !== null) window.clearInterval(clockTimer);
  });
}

void bootstrap();
