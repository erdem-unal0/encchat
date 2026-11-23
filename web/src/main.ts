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
  | { kind: "notice"; message: string };

interface Outgoing {
  to: string;
  data: Uint8Array;
}

interface SavedState {
  salt: string;
  blob: string;
}

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

// --- Client state -----------------------------------------------------------

const STORAGE_STATE = "encchat.state.v1";
const STORAGE_SERVER = "encchat.serverUrl";

let client: WasmClient | null = null;
let socket: WebSocket | null = null;
let peers = new Map<string, DeviceView>();
// Peers we have started a handshake with but do not yet have a session for.
let handshaking = new Set<string>();

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

function setStatus(text: string, tone: "info" | "ok" | "warn" | "error" = "info"): void {
  statusEl.textContent = text;
  statusEl.dataset.tone = tone;
}

function setDeviceCount(count: number): void {
  deviceCountEl.textContent = String(count);
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

function appendSystem(text: string): void {
  clearEmptyState();
  const stick = atBottom();
  const line = document.createElement("p");
  line.className = "system";
  line.textContent = text;
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
  const stick = atBottom();

  const wrap = document.createElement("div");
  wrap.className = options.outgoing ? "msg outgoing" : "msg incoming";

  const meta = document.createElement("div");
  meta.className = "meta";
  const who = options.outgoing ? "you" : shortId(options.from);
  const when = new Date(options.sentAt).toLocaleTimeString();
  meta.textContent = `${who} · ${when}`;

  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.textContent = options.body;

  wrap.append(meta, bubble);
  scrollbackEl.append(wrap);
  if (stick) scrollToBottom();
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
    return;
  }
  try {
    fingerprintEl.textContent = room_fingerprint(mnemonic);
  } catch {
    fingerprintEl.textContent = "(invalid mnemonic)";
  }
}

// --- Transport --------------------------------------------------------------

function sendRelay(to: string, payload: Uint8Array): void {
  if (!socket || socket.readyState !== WebSocket.OPEN) return;
  socket.send(encode_relay(to, payload));
}

function onJoined(devices: DeviceView[]): void {
  syncDevices(devices);
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
      appendSystem(`Secure session established with ${shortId(action.with)}…`);
      persistState();
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

// --- Connect / disconnect ---------------------------------------------------

function setConnected(connected: boolean): void {
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
  handshaking = new Set();
  setDeviceCount(0);
  setConnected(false);
  setStatus(reason, "warn");
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
  handshaking = new Set();
  setDeviceCount(0);
  localStorage.setItem(STORAGE_SERVER, serverUrl);

  appendSystem(`This device: ${created.device_id_hex()}`);
  setStatus(`Connecting to ${serverUrl}…`, "info");

  let ws: WebSocket;
  try {
    ws = new WebSocket(serverUrl);
  } catch (error) {
    setStatus(`Invalid relay URL: ${messageOf(error)}`, "error");
    client = null;
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

// --- Bootstrap --------------------------------------------------------------

async function bootstrap(): Promise<void> {
  try {
    await init();
  } catch (error) {
    setStatus(`Failed to load the WASM module: ${messageOf(error)}`, "error");
    return;
  }

  const savedServer = localStorage.getItem(STORAGE_SERVER);
  if (savedServer) serverUrlInput.value = savedServer;
  if (loadSavedState()) {
    appendSystem("Saved encrypted device state found. Enter your mnemonic and passphrase to restore it.");
  }

  updateFingerprint();

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

  sendForm.addEventListener("submit", (event) => {
    event.preventDefault();
    sendMessage();
  });

  window.addEventListener("beforeunload", persistState);
}

void bootstrap();
