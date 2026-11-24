// Portable appearance settings for the web client.
//
// The JSON shape is owned by `crates/appearance` (see docs/customization.md):
// the same document drives the CLI and the browser, so a theme imported here
// looks the same in a terminal. This module only does the browser half:
// storage, validation through the WASM binding, and CSS variables.

import {
  appearance_defaults,
  appearance_presets,
  validate_appearance,
} from "../pkg/encchat_wasm.js";

// --- Types mirroring crates/appearance --------------------------------------

/** Hex (`#89b4fa`), a terminal color name (`darkgray`), ANSI 0-255, or `reset`. */
export type Color = string;

export interface Theme {
  name: string;
  background: Color;
  text: Color;
  status_bg: Color;
  status_fg: Color;
  status_accent: Color;
  incoming_nick: Color;
  outgoing_nick: Color;
  timestamp: Color;
  system: Color;
  error: Color;
  input_prompt: Color;
  input_text: Color;
  input_bg: Color;
  border: Color;
  peer_palette: Color[];
}

export type FontFamily = "system" | "sans" | "serif" | "mono";
export type MessageStyle = "bubbles" | "irc";
export type Density = "compact" | "comfortable" | "spacious";

export interface Appearance {
  version: number;
  kind: string;
  theme: Theme;
  typography: { family: FontFamily; size: number; line_height: number };
  timestamps: { enabled: boolean; format: string; system: boolean };
  layout: {
    message_style: MessageStyle;
    density: Density;
    max_width: number;
    show_status_bar: boolean;
    show_hints: boolean;
    input_border: boolean;
    auto_scroll: boolean;
    peer_colors: boolean;
    show_unread: boolean;
  };
  labels: { own_fallback: string; show_peer_nicks: boolean; show_device_ids: boolean };
}

export type ThemeColorKey = Exclude<keyof Theme, "name" | "peer_palette">;

export const THEME_COLOR_KEYS: ReadonlyArray<{ key: ThemeColorKey; label: string }> = [
  { key: "background", label: "Background" },
  { key: "text", label: "Message text" },
  { key: "status_bg", label: "Status background" },
  { key: "status_fg", label: "Status text" },
  { key: "status_accent", label: "Accent" },
  { key: "incoming_nick", label: "Incoming nick" },
  { key: "outgoing_nick", label: "Outgoing nick" },
  { key: "timestamp", label: "Timestamp" },
  { key: "system", label: "System notices" },
  { key: "error", label: "Errors" },
  { key: "input_prompt", label: "Input prompt" },
  { key: "input_text", label: "Input text" },
  { key: "input_bg", label: "Input background" },
  { key: "border", label: "Borders" },
];

/**
 * The one built-in appearance copy, used only if the WASM binding is somehow
 * unavailable. `appearance_defaults()` is always preferred.
 */
export const DEFAULT_APPEARANCE: Appearance = {
  version: 1,
  kind: "encchat-appearance",
  theme: {
    name: "default",
    background: "reset",
    text: "reset",
    status_bg: "blue",
    status_fg: "white",
    status_accent: "lightcyan",
    incoming_nick: "green",
    outgoing_nick: "cyan",
    timestamp: "darkgray",
    system: "yellow",
    error: "lightred",
    input_prompt: "cyan",
    input_text: "reset",
    input_bg: "reset",
    border: "darkgray",
    peer_palette: ["green", "cyan", "magenta", "yellow", "lightblue", "lightgreen"],
  },
  typography: { family: "system", size: 15, line_height: 1.45 },
  timestamps: { enabled: true, format: "HH:mm:ss", system: true },
  layout: {
    message_style: "bubbles",
    density: "comfortable",
    max_width: 1100,
    show_status_bar: true,
    show_hints: true,
    input_border: true,
    auto_scroll: false,
    peer_colors: true,
    show_unread: true,
  },
  labels: { own_fallback: "me", show_peer_nicks: true, show_device_ids: true },
};

/**
 * Per-token fallbacks, used when a color is `reset`/`default` or unparseable.
 * These keep the connect flow looking like encchat even with a "reset" theme.
 */
export const DEFAULT_COLOR_FALLBACKS: Record<ThemeColorKey, string> = {
  background: "#0f1216",
  text: "#e6edf3",
  status_bg: "#1e242c",
  status_fg: "#e6edf3",
  status_accent: "#67e8f9",
  incoming_nick: "#4ade80",
  outgoing_nick: "#38bdf8",
  timestamp: "#8b98a9",
  system: "#fbbf24",
  error: "#f87171",
  input_prompt: "#38bdf8",
  input_text: "#e6edf3",
  input_bg: "#171c22",
  border: "#2f3945",
};

export const STORAGE_APPEARANCE = "encchat.settings.v1";
export const STORAGE_ALIASES = "encchat.labels.v1";
export const STORAGE_OWN_NICK = "encchat.nick.v1";

// --- Cloning ----------------------------------------------------------------

export function cloneAppearance<T>(value: T): T {
  if (typeof structuredClone === "function") {
    try {
      return structuredClone(value);
    } catch {
      // Fall through to JSON for values structuredClone refuses.
    }
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

/**
 * WASM carries `f32` values as JS doubles, so a line height of 1.45 comes back
 * as `1.4500000476837158`. Round the stored/exported form to the precision the
 * UI and the Rust sanitizer actually use.
 */
export function normalizeAppearance(value: Appearance): Appearance {
  const appearance = cloneAppearance(value);
  const height = appearance.typography.line_height;
  if (Number.isFinite(height)) {
    appearance.typography.line_height = Math.round(height * 100) / 100;
  }
  return appearance;
}

// --- WASM-backed lookups ----------------------------------------------------

export function appearanceDefaults(): Appearance {
  try {
    return normalizeAppearance(appearance_defaults() as Appearance);
  } catch {
    return cloneAppearance(DEFAULT_APPEARANCE);
  }
}

export function appearancePresets(): Theme[] {
  try {
    const presets = appearance_presets() as Theme[];
    if (Array.isArray(presets) && presets.length > 0) return presets;
  } catch {
    // Fall through to the single built-in theme.
  }
  return [cloneAppearance(DEFAULT_APPEARANCE.theme)];
}

export interface ImportResult {
  appearance: Appearance;
  warnings: string[];
}

/** Parse + sanitize arbitrary JSON through the shared Rust implementation. */
export function importAppearance(text: string): ImportResult {
  const result = validate_appearance(text) as ImportResult;
  return {
    appearance: normalizeAppearance(result.appearance),
    warnings: result.warnings ?? [],
  };
}

// --- Persistence ------------------------------------------------------------

export interface LoadedAppearance {
  appearance: Appearance;
  warnings: string[];
  /** Human-readable problem to surface when the saved copy could not be used. */
  notice: string | null;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function loadAppearance(): LoadedAppearance {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(STORAGE_APPEARANCE);
  } catch {
    return {
      appearance: appearanceDefaults(),
      warnings: [],
      notice: "Local storage is unavailable; using the default appearance.",
    };
  }
  if (!raw) {
    return { appearance: appearanceDefaults(), warnings: [], notice: null };
  }
  try {
    const result = importAppearance(raw);
    return {
      appearance: result.appearance,
      warnings: result.warnings,
      notice:
        result.warnings.length > 0
          ? `Saved appearance needed repairs: ${result.warnings.join("; ")}`
          : null,
    };
  } catch (error) {
    try {
      localStorage.removeItem(STORAGE_APPEARANCE);
    } catch {
      // Ignore: storage is unavailable, so there is nothing to clean up.
    }
    return {
      appearance: appearanceDefaults(),
      warnings: [],
      notice: `Saved appearance could not be read (${errorText(error)}); using defaults.`,
    };
  }
}

export function saveAppearance(appearance: Appearance): void {
  localStorage.setItem(STORAGE_APPEARANCE, formatAppearanceJson(appearance));
}

export function clearSavedAppearance(): void {
  localStorage.removeItem(STORAGE_APPEARANCE);
}

export function formatAppearanceJson(appearance: Appearance): string {
  return `${JSON.stringify(normalizeAppearance(appearance), null, 2)}\n`;
}

export function downloadAppearance(
  appearance: Appearance,
  filename = "encchat-appearance.json",
): void {
  const blob = new Blob([formatAppearanceJson(appearance)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.rel = "noopener";
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

// --- Colors -----------------------------------------------------------------

/** ANSI 0-15 as rendered by the usual xterm palette (matches the CLI crate). */
const ANSI_16: readonly string[] = [
  "#000000",
  "#cd0000",
  "#00cd00",
  "#cdcd00",
  "#0000ee",
  "#cd00cd",
  "#00cdcd",
  "#e5e5e5",
  "#7f7f7f",
  "#ff0000",
  "#00ff00",
  "#ffff00",
  "#5c5cff",
  "#ff00ff",
  "#00ffff",
  "#ffffff",
];

const CUBE_LEVELS: readonly number[] = [0, 95, 135, 175, 215, 255];

/**
 * Terminal color names mapped to CSS. ANSI indexes use the exact xterm table;
 * names are tuned slightly for a dark, modern browser UI while staying close to
 * the terminal hue.
 */
const NAMED_COLORS: Readonly<Record<string, string>> = {
  black: "#05070a",
  red: "#e5534b",
  green: "#57ab5a",
  yellow: "#d29922",
  blue: "#316dca",
  magenta: "#a371f7",
  cyan: "#39c5cf",
  gray: "#9aa4b2",
  grey: "#9aa4b2",
  lightgray: "#c9d1d9",
  lightgrey: "#c9d1d9",
  darkgray: "#8b98a9",
  darkgrey: "#8b98a9",
  lightred: "#ff7b72",
  lightgreen: "#7ee787",
  lightyellow: "#e3b341",
  lightblue: "#79c0ff",
  lightmagenta: "#d2a8ff",
  lightcyan: "#56d4dd",
  white: "#ffffff",
};

function namedColor(value: string): string | undefined {
  return NAMED_COLORS[value.replace(/[-_]/g, "")];
}

function normalizeHex(hex: string): string | null {
  if (!/^[0-9a-f]+$/.test(hex)) return null;
  if (hex.length === 3 || hex.length === 4) {
    return `#${hex[0]}${hex[0]}${hex[1]}${hex[1]}${hex[2]}${hex[2]}`;
  }
  if (hex.length === 6 || hex.length === 8) return `#${hex.slice(0, 6)}`;
  return null;
}

function ansiCss(index: number): string {
  const direct = ANSI_16[index];
  if (direct) return direct;
  if (index < 232) {
    const cube = index - 16;
    const r = CUBE_LEVELS[Math.floor(cube / 36)];
    const g = CUBE_LEVELS[Math.floor((cube % 36) / 6)];
    const b = CUBE_LEVELS[cube % 6];
    return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b
      .toString(16)
      .padStart(2, "0")}`;
  }
  const level = 8 + (index - 232) * 10;
  const hex = level.toString(16).padStart(2, "0");
  return `#${hex}${hex}${hex}`;
}

/** Whether `value` is a color this client (and the Rust crate) understands. */
export function isValidColorText(value: string): boolean {
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) return false;
  if (trimmed === "reset" || trimmed === "default") return true;
  if (namedColor(trimmed)) return true;
  if (trimmed.startsWith("#")) return normalizeHex(trimmed.slice(1)) !== null;
  return /^\d+$/.test(trimmed) && Number(trimmed) <= 255;
}

/**
 * Map a stored color to a CSS value. Hex passes through, `reset` returns
 * `fallback`, ANSI indexes use the xterm palette, and names use the table above.
 */
export function colorToCss(value: Color, fallback: string): string {
  const trimmed = value.trim().toLowerCase();
  if (!trimmed || trimmed === "reset" || trimmed === "default") return fallback;
  const named = namedColor(trimmed);
  if (named) return named;
  if (trimmed.startsWith("#")) {
    const hex = normalizeHex(trimmed.slice(1));
    if (hex) return hex;
    return fallback;
  }
  if (/^\d+$/.test(trimmed)) {
    const index = Number(trimmed);
    if (index <= 255) return ansiCss(index);
  }
  return fallback;
}

function parseHex(css: string): [number, number, number] | null {
  const match = /^#([0-9a-f]{6})$/.exec(css.trim().toLowerCase());
  if (!match) return null;
  const value = match[1];
  return [
    parseInt(value.slice(0, 2), 16),
    parseInt(value.slice(2, 4), 16),
    parseInt(value.slice(4, 6), 16),
  ];
}

function linearChannel(channel: number): number {
  const c = channel / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(css: string): number {
  const rgb = parseHex(css);
  if (!rgb) return 0;
  return (
    0.2126 * linearChannel(rgb[0]) +
    0.7152 * linearChannel(rgb[1]) +
    0.0722 * linearChannel(rgb[2])
  );
}

/** Black or white, whichever reads better on `background`. */
function readableTextOn(background: string): string {
  return luminance(background) > 0.42 ? "#0b0e12" : "#ffffff";
}

function isLight(background: string): boolean {
  return luminance(background) > 0.4;
}

// --- Applying CSS -----------------------------------------------------------

/** Push the whole appearance onto `:root` as CSS custom properties + data attrs. */
export function applyAppearance(appearance: Appearance): void {
  const root = document.documentElement;
  const theme = appearance.theme;
  const set = (name: string, value: string): void => {
    root.style.setProperty(name, value);
  };

  const background = colorToCss(theme.background, DEFAULT_COLOR_FALLBACKS.background);
  const accent = colorToCss(theme.status_accent, DEFAULT_COLOR_FALLBACKS.status_accent);
  const outgoing = colorToCss(theme.outgoing_nick, DEFAULT_COLOR_FALLBACKS.outgoing_nick);

  set("--enc-bg", background);
  set("--enc-text", colorToCss(theme.text, DEFAULT_COLOR_FALLBACKS.text));
  set("--enc-panel", colorToCss(theme.input_bg, DEFAULT_COLOR_FALLBACKS.input_bg));
  set("--enc-border", colorToCss(theme.border, DEFAULT_COLOR_FALLBACKS.border));
  set("--enc-accent", accent);
  set("--enc-accent-fg", readableTextOn(accent));
  set("--enc-status-bg", colorToCss(theme.status_bg, DEFAULT_COLOR_FALLBACKS.status_bg));
  set("--enc-status-fg", colorToCss(theme.status_fg, DEFAULT_COLOR_FALLBACKS.status_fg));
  set("--enc-status-accent", accent);
  set(
    "--enc-incoming-nick",
    colorToCss(theme.incoming_nick, DEFAULT_COLOR_FALLBACKS.incoming_nick),
  );
  set("--enc-outgoing-nick", outgoing);
  set("--enc-outgoing", outgoing);
  set("--enc-outgoing-fg", readableTextOn(outgoing));
  set("--enc-timestamp", colorToCss(theme.timestamp, DEFAULT_COLOR_FALLBACKS.timestamp));
  set("--enc-system", colorToCss(theme.system, DEFAULT_COLOR_FALLBACKS.system));
  set("--enc-error", colorToCss(theme.error, DEFAULT_COLOR_FALLBACKS.error));
  set(
    "--enc-input-prompt",
    colorToCss(theme.input_prompt, DEFAULT_COLOR_FALLBACKS.input_prompt),
  );
  set("--enc-input-text", colorToCss(theme.input_text, DEFAULT_COLOR_FALLBACKS.input_text));
  set("--enc-input-bg", colorToCss(theme.input_bg, DEFAULT_COLOR_FALLBACKS.input_bg));

  set("--enc-font-size", `${appearance.typography.size}px`);
  set("--enc-line-height", String(appearance.typography.line_height));
  set("--enc-max-width", `${appearance.layout.max_width}px`);

  root.dataset.messageStyle = appearance.layout.message_style;
  root.dataset.density = appearance.layout.density;
  root.dataset.font = appearance.typography.family;
  root.dataset.inputBorder = appearance.layout.input_border ? "on" : "off";
  root.style.colorScheme = isLight(background) ? "light" : "dark";
}

// --- Timestamps -------------------------------------------------------------

const TIMESTAMP_TOKENS = ["YYYY", "MM", "DD", "HH", "mm", "ss"] as const;
const MAX_TIMESTAMP_LEN = 64;

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/**
 * Render `format` for `date` using the shared token language
 * (`YYYY`, `MM`, `DD`, `HH`, `mm`, `ss`; unknown letter runs stay literal).
 */
export function formatTimestamp(format: string, date: Date): string {
  let out = "";
  let index = 0;
  while (index < format.length) {
    const ch = format[index];
    if (/[A-Za-z]/.test(ch)) {
      let end = index;
      while (end < format.length && /[A-Za-z]/.test(format[end])) end += 1;
      const run = format.slice(index, end);
      switch (run) {
        case "YYYY":
          out += String(date.getFullYear());
          break;
        case "MM":
          out += pad2(date.getMonth() + 1);
          break;
        case "DD":
          out += pad2(date.getDate());
          break;
        case "HH":
          out += pad2(date.getHours());
          break;
        case "mm":
          out += pad2(date.getMinutes());
          break;
        case "ss":
          out += pad2(date.getSeconds());
          break;
        default:
          out += run;
      }
      index = end;
    } else {
      out += ch;
      index += 1;
    }
  }
  return out;
}

/** Whether `value` is a valid timestamp format (mirrors the Rust validator). */
export function isValidTimestampFormat(value: string): boolean {
  if (Array.from(value).length > MAX_TIMESTAMP_LEN) return false;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return false;
  let found = false;
  let index = 0;
  while (index < value.length) {
    const ch = value[index];
    if (/[A-Za-z]/.test(ch)) {
      let end = index;
      while (end < value.length && /[A-Za-z]/.test(value[end])) end += 1;
      const run = value.slice(index, end);
      if ((TIMESTAMP_TOKENS as readonly string[]).includes(run)) found = true;
      index = end;
    } else {
      index += 1;
    }
  }
  return found;
}

// --- Peer colors ------------------------------------------------------------

/** Stable FNV-1a hash so a device keeps the same palette color across sessions. */
export function peerColor(palette: readonly Color[], deviceId: string): Color {
  if (palette.length === 0) return "reset";
  let hash = 0x811c9dc5;
  for (let i = 0; i < deviceId.length; i += 1) {
    hash ^= deviceId.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return palette[hash % palette.length];
}

// --- Local labels -----------------------------------------------------------

/** Trim, collapse internal whitespace, and cap at 32 characters. */
export function sanitizeLabel(value: string): string {
  let out = "";
  let pending = false;
  for (const ch of value) {
    if (/\s/.test(ch) || /[\u0000-\u001f\u007f-\u009f]/.test(ch)) {
      pending = out.length > 0;
      continue;
    }
    if (pending) {
      out += " ";
      pending = false;
    }
    out += ch;
  }
  return Array.from(out).slice(0, 32).join("");
}

export function loadAliases(): Record<string, string> {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(STORAGE_ALIASES);
  } catch {
    return {};
  }
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const aliases: Record<string, string> = {};
    for (const [device, alias] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof alias !== "string") continue;
      if (device.length === 0 || device.length > 64) continue;
      const clean = sanitizeLabel(alias);
      if (clean) aliases[device] = clean;
    }
    return aliases;
  } catch {
    return {};
  }
}

export function saveAliases(aliases: Record<string, string>): void {
  try {
    localStorage.setItem(STORAGE_ALIASES, JSON.stringify(aliases));
  } catch {
    // Aliases are a convenience; a full quota should not break chat.
  }
}

export function loadOwnNick(): string {
  try {
    return sanitizeLabel(localStorage.getItem(STORAGE_OWN_NICK) ?? "");
  } catch {
    return "";
  }
}

export function saveOwnNick(nick: string): void {
  try {
    const clean = sanitizeLabel(nick);
    if (clean) localStorage.setItem(STORAGE_OWN_NICK, clean);
    else localStorage.removeItem(STORAGE_OWN_NICK);
  } catch {
    // Ignore storage failures.
  }
}
