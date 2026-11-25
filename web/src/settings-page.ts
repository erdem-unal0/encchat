// Appearance settings page.
//
// A real route (`#/settings`), not a modal: main.ts hides the chat view and
// shows this one, so an active WebSocket session is never touched. Every edit
// is applied to the draft immediately for a live preview; only "Save changes"
// writes to localStorage and reports the new nickname.

import {
  DEFAULT_COLOR_FALLBACKS,
  THEME_COLOR_KEYS,
  applyAppearance,
  appearanceDefaults,
  appearancePresets,
  cloneAppearance,
  colorToCss,
  downloadAppearance,
  formatAppearanceJson,
  formatTimestamp,
  importAppearance,
  isValidColorText,
  isValidTimestampFormat,
  loadAppearance,
  peerColor,
  sanitizeLabel,
  saveAppearance,
  type Appearance,
  type Color,
  type Density,
  type FontFamily,
  type MessageStyle,
  type Theme,
  type ThemeColorKey,
} from "./appearance.js";

// --- Public API -------------------------------------------------------------

export interface SettingsPeer {
  id: string;
  session: boolean;
  nick: string;
}

export interface SettingsContext {
  connected: boolean;
  selfId: string | null;
  selfNick: string;
  peers: SettingsPeer[];
  aliases: Record<string, string>;
}

export interface SettingsHooks {
  /** Current chat state, read fresh whenever the page needs it. */
  context(): SettingsContext;
  /** A draft change was applied; main.ts mirrors it on the chat view. */
  onDraft(appearance: Appearance): void;
  /** The user saved; main.ts persists the nickname and announces it. */
  onSaved(appearance: Appearance, nick: string): void;
  /** A local alias changed for `device` (local only, never exported). */
  onAliasChange(device: string, alias: string): void;
  toast(message: string): void;
  navigateChat(): void;
}

// --- Constants and helpers --------------------------------------------------

type SectionId = "theme" | "typography" | "layout" | "timestamps" | "labels" | "profile";

const SECTIONS: ReadonlyArray<{ id: SectionId; label: string }> = [
  { id: "theme", label: "Theme" },
  { id: "typography", label: "Typography" },
  { id: "layout", label: "Layout" },
  { id: "timestamps", label: "Timestamps" },
  { id: "labels", label: "Labels" },
  { id: "profile", label: "Profile" },
];

const TIMESTAMP_PRESETS = ["HH:mm", "HH:mm:ss", "YYYY-MM-DD HH:mm", "DD/MM HH:mm"];

const FONT_OPTIONS: ReadonlyArray<{ value: FontFamily; label: string }> = [
  { value: "system", label: "System" },
  { value: "sans", label: "Sans" },
  { value: "serif", label: "Serif" },
  { value: "mono", label: "Mono" },
];

const STYLE_OPTIONS: ReadonlyArray<{ value: MessageStyle; label: string }> = [
  { value: "bubbles", label: "Bubbles" },
  { value: "irc", label: "IRC" },
];

const DENSITY_OPTIONS: ReadonlyArray<{ value: Density; label: string }> = [
  { value: "compact", label: "Compact" },
  { value: "comfortable", label: "Comfortable" },
  { value: "spacious", label: "Spacious" },
];

type LayoutToggleKey =
  | "show_status_bar"
  | "input_border"
  | "auto_scroll"
  | "peer_colors"
  | "show_unread";

const LAYOUT_TOGGLES: ReadonlyArray<{ key: LayoutToggleKey; label: string; hint: string }> = [
  { key: "show_status_bar", label: "Status bar", hint: "Connection, room and peer counts." },
  { key: "input_border", label: "Input border", hint: "Outline around the composer." },
  { key: "auto_scroll", label: "Auto-scroll", hint: "Follow new messages while reading back." },
  { key: "peer_colors", label: "Peer colors", hint: "Stable nickname colors per device." },
  { key: "show_unread", label: "Unread counter", hint: "Offer a jump button while scrolled up." },
];

const PREVIEW_PEER = "3f9a2c41d8b7e6a5";

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

function button(className: string, text: string): HTMLButtonElement {
  const element = node("button", className, text);
  element.type = "button";
  return element;
}

function shortId(hex: string): string {
  return hex.slice(0, 8);
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function setPressed<K extends string>(buttons: Map<K, HTMLButtonElement>, value: K): void {
  for (const [key, element] of buttons) {
    element.setAttribute("aria-pressed", String(key === value));
  }
}

function segmented<K extends string>(
  label: string,
  options: ReadonlyArray<{ value: K; label: string }>,
  current: K,
  onPick: (value: K) => void,
): { host: HTMLElement; buttons: Map<K, HTMLButtonElement> } {
  const host = node("div", "segmented");
  host.setAttribute("role", "group");
  host.setAttribute("aria-label", label);
  const buttons = new Map<K, HTMLButtonElement>();
  for (const option of options) {
    const element = button("segmented-button", option.label);
    element.setAttribute("aria-pressed", String(option.value === current));
    element.addEventListener("click", () => onPick(option.value));
    buttons.set(option.value, element);
    host.append(element);
  }
  return { host, buttons };
}

function switchRow(
  label: string,
  hint: string | null,
  checked: boolean,
  onChange: (checked: boolean) => void,
): { row: HTMLElement; input: HTMLInputElement } {
  const row = node("div", "switch-row");
  const copy = node("div", "switch-copy");
  copy.append(node("span", "switch-label", label));
  if (hint) copy.append(node("span", "switch-hint", hint));
  const wrapper = node("label", "switch");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  input.setAttribute("role", "switch");
  input.setAttribute("aria-label", label);
  const track = node("span", "switch-track");
  track.setAttribute("aria-hidden", "true");
  wrapper.append(input, track);
  input.addEventListener("change", () => onChange(input.checked));
  row.append(copy, wrapper);
  return { row, input };
}

// --- Page -------------------------------------------------------------------

export class SettingsPage {
  private readonly root: HTMLElement;
  private readonly hooks: SettingsHooks;
  private readonly presets: Theme[];
  private readonly defaults: Appearance;
  private saved: Appearance;
  private draft: Appearance;
  private savedNick = "";
  private displayName = "";
  private warnings: string[] = [];
  private noticeTone: "warn" | "error" = "warn";
  private aliases: Record<string, string> = {};
  private visible = false;

  // Theme
  private presetButtons = new Map<string, HTMLButtonElement>();
  private colorControls = new Map<
    ThemeColorKey,
    { picker: HTMLInputElement; text: HTMLInputElement }
  >();
  private paletteRows!: HTMLElement;
  private paletteAdd!: HTMLButtonElement;

  // Typography
  private fontButtons = new Map<FontFamily, HTMLButtonElement>();
  private sizeRange!: HTMLInputElement;
  private sizeValue!: HTMLElement;
  private lineRange!: HTMLInputElement;
  private lineValue!: HTMLElement;

  // Layout
  private styleButtons = new Map<MessageStyle, HTMLButtonElement>();
  private densityButtons = new Map<Density, HTMLButtonElement>();
  private widthRange!: HTMLInputElement;
  private widthValue!: HTMLElement;
  private layoutToggles = new Map<LayoutToggleKey, HTMLInputElement>();

  // Timestamps
  private timestampEnabled!: HTMLInputElement;
  private timestampSystem!: HTMLInputElement;
  private timestampSelect!: HTMLSelectElement;
  private timestampInput!: HTMLInputElement;
  private timestampError!: HTMLElement;
  private timestampExample!: HTMLElement;

  // Labels
  private displayNameInput!: HTMLInputElement;
  private ownFallbackInput!: HTMLInputElement;
  private peerNicksToggle!: HTMLInputElement;
  private deviceIdsToggle!: HTMLInputElement;
  private aliasesHost!: HTMLElement;

  // Profile / save bar / preview
  private bannerHost!: HTMLElement;
  private dirtyBadge!: HTMLElement;
  private dirtyText!: HTMLElement;
  private saveButton!: HTMLButtonElement;
  private revertButton!: HTMLButtonElement;
  private previewHost!: HTMLElement;
  private navButtons = new Map<SectionId, HTMLButtonElement>();
  private sectionsHost!: HTMLElement;
  private scrollFrame: number | null = null;

  constructor(root: HTMLElement, hooks: SettingsHooks) {
    this.root = root;
    this.hooks = hooks;
    this.presets = appearancePresets();
    this.defaults = appearanceDefaults();
    this.saved = cloneAppearance(this.defaults);
    this.draft = cloneAppearance(this.defaults);

    this.root.addEventListener("keydown", (event) => {
      if (!this.visible) return;
      if (event.key === "Escape") {
        event.preventDefault();
        this.hooks.navigateChat();
        return;
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        this.save();
      }
    });

    // Scroll spy: keep the section nav in sync with what is actually visible.
    window.addEventListener("scroll", this.handleWindowScroll, { passive: true });
    window.addEventListener("resize", this.handleWindowScroll, { passive: true });
  }

  private readonly handleWindowScroll = (): void => {
    if (!this.visible || this.scrollFrame !== null) return;
    this.scrollFrame = window.requestAnimationFrame(() => {
      this.scrollFrame = null;
      this.syncActiveSection();
    });
  };

  show(): void {
    const loaded = loadAppearance();
    this.saved = loaded.appearance;
    this.draft = cloneAppearance(loaded.appearance);
    this.warnings = loaded.notice ? [loaded.notice] : [];
    this.noticeTone = "warn";
    const context = this.hooks.context();
    this.aliases = { ...context.aliases };
    this.savedNick = context.selfNick;
    this.displayName = context.selfNick;
    this.visible = true;
    this.build();
    applyAppearance(this.draft);
  }

  hide(): void {
    if (!this.visible) return;
    this.visible = false;
    if (this.isDirty()) {
      this.hooks.onDraft(this.saved);
      this.hooks.toast("Unsaved appearance changes were discarded.");
    }
  }

  /** Called by main.ts when peers, nicks or sessions change while open. */
  refresh(): void {
    if (!this.visible) return;
    this.aliases = { ...this.hooks.context().aliases };
    this.renderAliases();
    this.renderPreview();
  }

  // --- Construction ---------------------------------------------------------

  private build(): void {
    const shell = node("div", "settings-shell");
    shell.append(this.buildHeader());

    const layout = node("div", "settings-layout");
    this.sectionsHost = node("div", "settings-sections");
    this.bannerHost = node("div", "banner-host");
    this.sectionsHost.append(this.bannerHost);
    this.buildThemeSection();
    this.buildTypographySection();
    this.buildLayoutSection();
    this.buildTimestampsSection();
    this.buildLabelsSection();
    this.buildProfileSection();

    const preview = node("aside", "preview-col");
    this.previewHost = node("div", "preview-host");
    preview.append(this.previewHost);

    layout.append(this.buildNav(), this.sectionsHost, preview);
    shell.append(layout, this.buildSaveBar());
    this.root.replaceChildren(shell);

    this.syncAll();
    this.renderAliases();
    this.applyDraft();
    window.requestAnimationFrame(() => this.syncActiveSection());
  }

  private buildHeader(): HTMLElement {
    const header = node("header", "settings-header");
    const copy = node("div", "settings-header-copy");
    copy.append(
      node("h2", "settings-title", "Appearance"),
      node(
        "p",
        "settings-subtitle",
        "Changes preview instantly. Settings stay on this device; nothing is sent to the relay.",
      ),
    );
    const back = button("btn ghost", "Back to chat");
    back.addEventListener("click", () => this.hooks.navigateChat());
    header.append(copy, back);
    return header;
  }

  private buildNav(): HTMLElement {
    const nav = node("nav", "settings-nav");
    nav.setAttribute("aria-label", "Appearance sections");
    for (const section of SECTIONS) {
      const element = button("settings-nav-link", section.label);
      element.addEventListener("click", () => {
        this.setActiveSection(section.id);
        const target = document.getElementById(`settings-section-${section.id}`);
        const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        target?.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
      });
      this.navButtons.set(section.id, element);
      nav.append(element);
    }
    this.setActiveSection("theme");
    return nav;
  }

  private setActiveSection(id: SectionId): void {
    for (const [key, element] of this.navButtons) {
      if (key === id) element.setAttribute("aria-current", "true");
      else element.removeAttribute("aria-current");
    }
  }

  /** Pick the section whose heading has most recently crossed the top band. */
  private syncActiveSection(): void {
    if (!this.visible) return;
    let active: SectionId = SECTIONS[0].id;
    for (const section of SECTIONS) {
      const element = document.getElementById(`settings-section-${section.id}`);
      if (element && element.getBoundingClientRect().top <= 140) active = section.id;
    }
    const doc = document.documentElement;
    if (
      doc.scrollHeight > window.innerHeight + 4 &&
      window.innerHeight + window.scrollY >= doc.scrollHeight - 4
    ) {
      active = SECTIONS[SECTIONS.length - 1].id;
    }
    this.setActiveSection(active);
  }

  private createCard(
    id: SectionId,
    title: string,
    subtitle: string,
    allowReset = true,
  ): { card: HTMLElement; body: HTMLElement } {
    const card = node("section", "card");
    card.id = `settings-section-${id}`;
    const head = node("div", "card-head");
    const copy = node("div", "card-head-copy");
    copy.append(node("h3", "card-title", title), node("p", "card-subtitle", subtitle));
    head.append(copy);
    if (allowReset) {
      const reset = button("card-reset", "Reset");
      reset.addEventListener("click", () => this.resetSection(id));
      head.append(reset);
    }
    const body = node("div", "card-body");
    card.append(head, body);
    this.sectionsHost.append(card);
    return { card, body };
  }

  // --- Theme ----------------------------------------------------------------

  private buildThemeSection(): void {
    const { body } = this.createCard(
      "theme",
      "Theme",
      "Portable presets shared with the CLI; every color can be overridden.",
    );

    const grid = node("div", "preset-grid");
    this.presetButtons.clear();
    for (const preset of this.presets) {
      const card = button("preset-card", "");
      card.title = `Use the ${preset.name} theme`;
      card.setAttribute("aria-pressed", String(preset.name === this.draft.theme.name));
      const swatch = node("span", "preset-swatch");
      const colors: Array<[Color, string]> = [
        [preset.background, DEFAULT_COLOR_FALLBACKS.background],
        [preset.status_bg, DEFAULT_COLOR_FALLBACKS.status_bg],
        [preset.status_accent, DEFAULT_COLOR_FALLBACKS.status_accent],
        [preset.outgoing_nick, DEFAULT_COLOR_FALLBACKS.outgoing_nick],
        [preset.text, DEFAULT_COLOR_FALLBACKS.text],
      ];
      for (const [color, fallback] of colors) {
        const slice = node("span");
        slice.style.background = colorToCss(color, fallback);
        swatch.append(slice);
      }
      card.append(swatch, node("span", "preset-name", preset.name));
      card.addEventListener("click", () => {
        this.draft.theme = cloneAppearance(preset);
        this.syncTheme();
        this.applyDraft();
      });
      this.presetButtons.set(preset.name, card);
      grid.append(card);
    }
    body.append(grid);

    const details = node("details", "disclose");
    const summary = node("summary", "disclose-summary", "Custom colors");
    details.append(summary);

    const colorGrid = node("div", "color-grid");
    this.colorControls.clear();
    for (const { key, label } of THEME_COLOR_KEYS) {
      const row = node("div", "color-row");
      row.append(node("span", "color-label", label));
      const controls = node("div", "color-controls");

      const picker = document.createElement("input");
      picker.type = "color";
      picker.value = colorToCss(this.draft.theme[key], DEFAULT_COLOR_FALLBACKS[key]);
      picker.setAttribute("aria-label", `${label} picker`);

      const text = document.createElement("input");
      text.type = "text";
      text.spellcheck = false;
      text.value = this.draft.theme[key];
      text.setAttribute("aria-label", `${label} color value`);
      text.setAttribute("autocomplete", "off");

      text.addEventListener("input", () => {
        const value = text.value.trim().toLowerCase();
        if (!isValidColorText(value)) {
          text.setAttribute("aria-invalid", "true");
          return;
        }
        text.removeAttribute("aria-invalid");
        picker.value = colorToCss(value, DEFAULT_COLOR_FALLBACKS[key]);
        this.setThemeColor(key, value);
      });
      picker.addEventListener("input", () => {
        text.removeAttribute("aria-invalid");
        text.value = picker.value;
        this.setThemeColor(key, picker.value);
      });

      controls.append(picker, text);
      row.append(controls);
      colorGrid.append(row);
      this.colorControls.set(key, { picker, text });
    }
    details.append(colorGrid);

    const palette = node("div", "palette-editor");
    palette.append(node("h4", "palette-title", "Peer colors"));
    palette.append(
      node("p", "palette-hint", "Used for nicknames when “Peer colors” is on."),
    );
    this.paletteRows = node("div", "palette-rows");
    this.paletteAdd = button("btn ghost small", "Add color");
    this.paletteAdd.addEventListener("click", () => {
      if (this.draft.theme.peer_palette.length >= 12) return;
      this.draft.theme.peer_palette.push("#79c0ff");
      this.draft.theme.name = "custom";
      this.syncPresetButtons();
      this.renderPalette();
      this.applyDraft();
    });
    palette.append(this.paletteRows, this.paletteAdd);
    details.append(palette);
    body.append(details);
  }

  private setThemeColor(key: ThemeColorKey, value: string): void {
    this.draft.theme[key] = value;
    this.draft.theme.name = "custom";
    this.syncPresetButtons();
    this.applyDraft();
  }

  private syncTheme(): void {
    this.syncPresetButtons();
    for (const [key, controls] of this.colorControls) {
      const value = this.draft.theme[key];
      controls.text.value = value;
      controls.text.removeAttribute("aria-invalid");
      controls.picker.value = colorToCss(value, DEFAULT_COLOR_FALLBACKS[key]);
    }
    this.renderPalette();
  }

  private syncPresetButtons(): void {
    for (const [name, element] of this.presetButtons) {
      element.setAttribute("aria-pressed", String(name === this.draft.theme.name));
    }
  }

  private renderPalette(): void {
    if (!this.paletteRows) return;
    this.paletteRows.replaceChildren();
    const palette = this.draft.theme.peer_palette;
    palette.forEach((color, index) => {
      const row = node("div", "palette-row");
      const picker = document.createElement("input");
      picker.type = "color";
      picker.value = colorToCss(color, "#79c0ff");
      picker.setAttribute("aria-label", `Peer color ${index + 1} picker`);
      const text = document.createElement("input");
      text.type = "text";
      text.spellcheck = false;
      text.value = color;
      text.setAttribute("aria-label", `Peer color ${index + 1} value`);
      text.setAttribute("autocomplete", "off");

      text.addEventListener("input", () => {
        const value = text.value.trim().toLowerCase();
        if (!isValidColorText(value)) {
          text.setAttribute("aria-invalid", "true");
          return;
        }
        text.removeAttribute("aria-invalid");
        picker.value = colorToCss(value, "#79c0ff");
        this.setPaletteColor(index, value);
      });
      picker.addEventListener("input", () => {
        text.removeAttribute("aria-invalid");
        text.value = picker.value;
        this.setPaletteColor(index, picker.value);
      });

      const remove = button("icon-button tiny", "×");
      remove.setAttribute("aria-label", `Remove peer color ${index + 1}`);
      remove.disabled = palette.length <= 1;
      remove.addEventListener("click", () => {
        this.draft.theme.peer_palette.splice(index, 1);
        this.draft.theme.name = "custom";
        this.syncPresetButtons();
        this.renderPalette();
        this.applyDraft();
      });

      row.append(picker, text, remove);
      this.paletteRows.append(row);
    });
    this.paletteAdd.disabled = palette.length >= 12;
  }

  private setPaletteColor(index: number, value: string): void {
    this.draft.theme.peer_palette[index] = value;
    this.draft.theme.name = "custom";
    this.syncPresetButtons();
    this.applyDraft();
  }

  // --- Typography -----------------------------------------------------------

  private buildTypographySection(): void {
    const { body } = this.createCard(
      "typography",
      "Typography",
      "Base size and line height apply to chat and settings.",
    );

    const family = segmented("Font family", FONT_OPTIONS, this.draft.typography.family, (value) => {
      this.draft.typography.family = value;
      setPressed(this.fontButtons, value);
      this.applyDraft();
    });
    this.fontButtons = family.buttons;
    body.append(field("Font family", family.host));

    this.sizeRange = document.createElement("input");
    this.sizeRange.type = "range";
    this.sizeRange.min = "12";
    this.sizeRange.max = "22";
    this.sizeRange.step = "1";
    this.sizeRange.value = String(this.draft.typography.size);
    this.sizeRange.setAttribute("aria-label", "Base font size in pixels");
    this.sizeRange.addEventListener("input", () => {
      const size = Number(this.sizeRange.value);
      this.draft.typography.size = size;
      this.sizeValue.textContent = `${size}px`;
      this.applyDraft();
    });
    this.sizeValue = node("output", "range-value", `${this.draft.typography.size}px`);
    body.append(rangeRow("Size", this.sizeRange, this.sizeValue));

    this.lineRange = document.createElement("input");
    this.lineRange.type = "range";
    this.lineRange.min = "1";
    this.lineRange.max = "2";
    this.lineRange.step = "0.01";
    this.lineRange.value = String(this.draft.typography.line_height);
    this.lineRange.setAttribute("aria-label", "Line height multiple");
    this.lineRange.addEventListener("input", () => {
      const value = Math.round(Number(this.lineRange.value) * 100) / 100;
      this.draft.typography.line_height = value;
      this.lineValue.textContent = value.toFixed(2);
      this.applyDraft();
    });
    this.lineValue = node("output", "range-value", this.draft.typography.line_height.toFixed(2));
    body.append(rangeRow("Line height", this.lineRange, this.lineValue));
  }

  private syncTypography(): void {
    setPressed(this.fontButtons, this.draft.typography.family);
    this.sizeRange.value = String(this.draft.typography.size);
    this.sizeValue.textContent = `${this.draft.typography.size}px`;
    this.lineRange.value = String(this.draft.typography.line_height);
    this.lineValue.textContent = this.draft.typography.line_height.toFixed(2);
  }

  // --- Layout ---------------------------------------------------------------

  private buildLayoutSection(): void {
    const { body } = this.createCard(
      "layout",
      "Layout",
      "Message style, vertical rhythm, and which chrome is visible.",
    );

    const styles = node("div", "style-cards");
    this.styleButtons.clear();
    for (const option of STYLE_OPTIONS) {
      const card = button("style-card", "");
      card.setAttribute("aria-pressed", String(this.draft.layout.message_style === option.value));
      card.setAttribute("aria-label", `${option.label} message style`);
      const diagram = node("span", `style-diagram ${option.value}`);
      if (option.value === "bubbles") {
        diagram.append(
          node("span", "diagram-bubble"),
          node("span", "diagram-bubble right"),
        );
      } else {
        diagram.append(
          node("span", "diagram-line"),
          node("span", "diagram-line short"),
        );
      }
      card.append(diagram, node("span", "style-card-label", option.label));
      card.addEventListener("click", () => {
        this.draft.layout.message_style = option.value;
        setPressed(this.styleButtons, option.value);
        this.applyDraft();
      });
      this.styleButtons.set(option.value, card);
      styles.append(card);
    }
    body.append(field("Message style", styles));

    const density = segmented(
      "Message density",
      DENSITY_OPTIONS,
      this.draft.layout.density,
      (value) => {
        this.draft.layout.density = value;
        setPressed(this.densityButtons, value);
        this.applyDraft();
      },
    );
    this.densityButtons = density.buttons;
    body.append(field("Density", density.host));

    this.widthRange = document.createElement("input");
    this.widthRange.type = "range";
    this.widthRange.min = "480";
    this.widthRange.max = "1600";
    this.widthRange.step = "1";
    this.widthRange.value = String(this.draft.layout.max_width);
    this.widthRange.setAttribute("aria-label", "Maximum content width in pixels");
    this.widthRange.addEventListener("input", () => {
      const width = Number(this.widthRange.value);
      this.draft.layout.max_width = width;
      this.widthValue.textContent = `${width}px`;
      this.applyDraft();
    });
    this.widthValue = node("output", "range-value", `${this.draft.layout.max_width}px`);
    body.append(rangeRow("Max width", this.widthRange, this.widthValue));

    const toggles = node("div", "switch-list");
    this.layoutToggles.clear();
    for (const option of LAYOUT_TOGGLES) {
      const { row, input } = switchRow(
        option.label,
        option.hint,
        this.draft.layout[option.key],
        (checked) => {
          this.draft.layout[option.key] = checked;
          this.applyDraft();
        },
      );
      this.layoutToggles.set(option.key, input);
      toggles.append(row);
    }
    body.append(toggles);
  }

  private syncLayout(): void {
    setPressed(this.styleButtons, this.draft.layout.message_style);
    setPressed(this.densityButtons, this.draft.layout.density);
    this.widthRange.value = String(this.draft.layout.max_width);
    this.widthValue.textContent = `${this.draft.layout.max_width}px`;
    for (const [key, input] of this.layoutToggles) {
      input.checked = this.draft.layout[key];
    }
  }

  // --- Timestamps -----------------------------------------------------------

  private buildTimestampsSection(): void {
    const { body } = this.createCard(
      "timestamps",
      "Timestamps",
      "A tiny token language shared with the CLI: YYYY, MM, DD, HH, mm, ss.",
    );

    const enabled = switchRow(
      "Show timestamps",
      "Applies to chat messages.",
      this.draft.timestamps.enabled,
      (checked) => {
        this.draft.timestamps.enabled = checked;
        this.applyDraft();
      },
    );
    this.timestampEnabled = enabled.input;
    body.append(enabled.row);

    this.timestampSelect = document.createElement("select");
    this.timestampSelect.setAttribute("aria-label", "Timestamp format preset");
    for (const format of TIMESTAMP_PRESETS) {
      const option = document.createElement("option");
      option.value = format;
      option.textContent = format;
      this.timestampSelect.append(option);
    }
    const custom = document.createElement("option");
    custom.value = "__custom";
    custom.textContent = "Custom…";
    this.timestampSelect.append(custom);
    this.timestampSelect.value = TIMESTAMP_PRESETS.includes(this.draft.timestamps.format)
      ? this.draft.timestamps.format
      : "__custom";
    this.timestampSelect.addEventListener("change", () => {
      const value = this.timestampSelect.value;
      if (value === "__custom") {
        this.timestampInput.focus();
        return;
      }
      this.timestampInput.value = value;
      this.commitTimestamp(value);
    });
    body.append(field("Format preset", this.timestampSelect));

    const customField = node("div", "field");
    const customLabel = node("label", "field-label", "Custom format");
    this.timestampInput = document.createElement("input");
    this.timestampInput.type = "text";
    this.timestampInput.spellcheck = false;
    this.timestampInput.value = this.draft.timestamps.format;
    this.timestampInput.setAttribute("autocomplete", "off");
    this.timestampInput.id = "settings-timestamp-format";
    customLabel.htmlFor = this.timestampInput.id;
    this.timestampInput.addEventListener("input", () => {
      const value = this.timestampInput.value;
      const valid = isValidTimestampFormat(value);
      if (valid) {
        this.timestampInput.removeAttribute("aria-invalid");
        this.timestampError.hidden = true;
        this.timestampSelect.value = TIMESTAMP_PRESETS.includes(value) ? value : "__custom";
        this.draft.timestamps.format = value;
        this.timestampExample.textContent = formatTimestamp(value, new Date());
        this.applyDraft();
      } else {
        this.timestampInput.setAttribute("aria-invalid", "true");
        this.timestampError.hidden = false;
        this.timestampError.textContent =
          Array.from(value).length > 64
            ? "Formats are limited to 64 characters."
            : "Use at least one token: YYYY, MM, DD, HH, mm, ss.";
        this.timestampExample.textContent = "—";
      }
    });
    this.timestampError = node("span", "field-error");
    this.timestampError.hidden = true;
    this.timestampError.setAttribute("role", "alert");
    customField.append(customLabel, this.timestampInput, this.timestampError);
    body.append(customField);

    const example = node("p", "example-line");
    example.append(node("span", "example-label", "Example"));
    this.timestampExample = node("code", "example-value", formatTimestamp(this.draft.timestamps.format, new Date()));
    example.append(this.timestampExample);
    body.append(example);

    const system = switchRow(
      "Timestamps on system lines",
      "Notices and session events get the same prefix.",
      this.draft.timestamps.system,
      (checked) => {
        this.draft.timestamps.system = checked;
        this.applyDraft();
      },
    );
    this.timestampSystem = system.input;
    body.append(system.row);
  }

  private commitTimestamp(value: string): void {
    this.timestampInput.removeAttribute("aria-invalid");
    this.timestampError.hidden = true;
    this.draft.timestamps.format = value;
    this.timestampExample.textContent = formatTimestamp(value, new Date());
    this.applyDraft();
  }

  private syncTimestamps(): void {
    const timestamps = this.draft.timestamps;
    this.timestampEnabled.checked = timestamps.enabled;
    this.timestampSystem.checked = timestamps.system;
    this.timestampInput.value = timestamps.format;
    this.timestampInput.removeAttribute("aria-invalid");
    this.timestampError.hidden = true;
    this.timestampSelect.value = TIMESTAMP_PRESETS.includes(timestamps.format)
      ? timestamps.format
      : "__custom";
    this.timestampExample.textContent = formatTimestamp(timestamps.format, new Date());
  }

  // --- Labels ---------------------------------------------------------------

  private buildLabelsSection(): void {
    const { body } = this.createCard(
      "labels",
      "Labels",
      "Nicknames are encrypted; aliases and fallbacks never leave this device.",
    );

    this.displayNameInput = document.createElement("input");
    this.displayNameInput.type = "text";
    this.displayNameInput.spellcheck = false;
    this.displayNameInput.maxLength = 32;
    this.displayNameInput.value = this.displayName;
    this.displayNameInput.placeholder = "Your display name";
    this.displayNameInput.setAttribute("autocomplete", "off");
    this.displayNameInput.addEventListener("input", () => {
      this.displayName = this.displayNameInput.value;
      this.renderPreview();
      this.updateDirty();
    });
    body.append(
      field("Display name", this.displayNameInput, "Sent to peers when you save; empty clears it."),
    );

    this.ownFallbackInput = document.createElement("input");
    this.ownFallbackInput.type = "text";
    this.ownFallbackInput.spellcheck = false;
    this.ownFallbackInput.value = this.draft.labels.own_fallback;
    this.ownFallbackInput.placeholder = "me";
    this.ownFallbackInput.addEventListener("input", () => {
      this.draft.labels.own_fallback = Array.from(this.ownFallbackInput.value)
        .slice(0, 32)
        .join("");
      this.applyDraft();
    });
    body.append(
      field(
        "Own label fallback",
        this.ownFallbackInput,
        "Shown before a display name is set.",
      ),
    );

    const toggles = node("div", "switch-list");
    const peerNicks = switchRow(
      "Show peer nicknames",
      "Use a peer's advertised nickname in the chat.",
      this.draft.labels.show_peer_nicks,
      (checked) => {
        this.draft.labels.show_peer_nicks = checked;
        this.applyDraft();
      },
    );
    this.peerNicksToggle = peerNicks.input;
    const deviceIds = switchRow(
      "Show device ids",
      "Fall back to an 8-character device id.",
      this.draft.labels.show_device_ids,
      (checked) => {
        this.draft.labels.show_device_ids = checked;
        this.applyDraft();
      },
    );
    this.deviceIdsToggle = deviceIds.input;
    toggles.append(peerNicks.row, deviceIds.row);
    body.append(toggles);

    const aliasBox = node("div", "alias-box");
    aliasBox.append(
      node("h4", "alias-title", "Local aliases"),
      node(
        "p",
        "alias-hint",
        "Saved immediately as you type. Only you see these, and they are not part of exported profiles.",
      ),
    );
    this.aliasesHost = node("div", "alias-list");
    aliasBox.append(this.aliasesHost);
    body.append(aliasBox);
  }

  private syncLabels(): void {
    this.displayNameInput.value = this.displayName;
    this.ownFallbackInput.value = this.draft.labels.own_fallback;
    this.peerNicksToggle.checked = this.draft.labels.show_peer_nicks;
    this.deviceIdsToggle.checked = this.draft.labels.show_device_ids;
  }

  private renderAliases(): void {
    if (!this.aliasesHost) return;
    this.aliasesHost.replaceChildren();
    const context = this.hooks.context();
    this.aliases = { ...context.aliases };
    const peers = [...context.peers].sort((left, right) => left.id.localeCompare(right.id));
    if (peers.length === 0) {
      this.aliasesHost.append(node("p", "empty-note", "Connect to see peers."));
      return;
    }
    for (const peer of peers) {
      const row = node("div", "alias-row");
      row.dataset.device = peer.id;
      const dot = node("span", "person-dot");
      dot.style.background = colorToCss(
        peerColor(this.draft.theme.peer_palette, peer.id),
        DEFAULT_COLOR_FALLBACKS.incoming_nick,
      );
      const device = node("code", "alias-device", shortId(peer.id));
      const input = document.createElement("input");
      input.type = "text";
      input.spellcheck = false;
      input.maxLength = 32;
      input.value = this.aliases[peer.id] ?? "";
      input.placeholder = peer.nick || "Local alias";
      input.setAttribute("aria-label", `Local alias for device ${shortId(peer.id)}`);
      input.addEventListener("input", () => {
        this.aliases[peer.id] = input.value;
        this.hooks.onAliasChange(peer.id, input.value);
      });
      row.append(dot, device, input);
      this.aliasesHost.append(row);
    }
  }

  // --- Profile --------------------------------------------------------------

  private buildProfileSection(): void {
    const { body } = this.createCard(
      "profile",
      "Profile",
      "Portable JSON: the same file the CLI reads with `encchat config import-appearance`.",
      false,
    );

    const exportRow = node("div", "button-row");
    const download = button("btn", "Download JSON");
    download.addEventListener("click", () => {
      downloadAppearance(this.draft);
      this.hooks.toast("Appearance exported.");
    });
    const copy = button("btn ghost", "Copy JSON");
    copy.addEventListener("click", () => {
      void this.copyJson();
    });
    exportRow.append(download, copy);
    body.append(field("Export", exportRow));

    const fileField = node("div", "field");
    const fileLabel = node("label", "field-label", "Import from a file");
    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = "application/json,.json";
    fileInput.className = "file-input";
    fileLabel.htmlFor = "settings-import-file";
    fileInput.id = "settings-import-file";
    fileInput.addEventListener("change", () => {
      const file = fileInput.files?.[0];
      if (!file) return;
      file
        .text()
        .then((text) => this.applyImport(text))
        .catch(() => this.hooks.toast("Could not read that file."));
      fileInput.value = "";
    });
    fileField.append(fileLabel, fileInput);
    body.append(fileField);

    const pasteField = node("div", "field");
    const pasteLabel = node("label", "field-label", "Paste JSON");
    const textarea = document.createElement("textarea");
    textarea.rows = 7;
    textarea.spellcheck = false;
    textarea.placeholder = '{ "theme": { "name": "nord" } }';
    textarea.id = "settings-import-text";
    pasteLabel.htmlFor = textarea.id;
    const pasteRow = node("div", "button-row");
    const importButton = button("btn", "Import");
    importButton.addEventListener("click", () => this.applyImport(textarea.value));
    const resetAll = button("btn danger", "Reset all");
    resetAll.addEventListener("click", () => {
      this.draft = cloneAppearance(this.defaults);
      this.displayName = this.savedNick;
      this.syncAll();
      this.applyDraft();
      this.hooks.toast("Draft reset to defaults. Save to keep it.");
    });
    pasteRow.append(importButton, resetAll);
    pasteField.append(pasteLabel, textarea, pasteRow);
    body.append(pasteField);
  }

  private async copyJson(): Promise<void> {
    try {
      await navigator.clipboard.writeText(formatAppearanceJson(this.draft));
      this.hooks.toast("Appearance JSON copied.");
    } catch {
      this.hooks.toast("Clipboard access was denied; download the file instead.");
    }
  }

  private applyImport(text: string): void {
    if (!text.trim()) {
      this.noticeTone = "error";
      this.warnings = ["Paste an appearance document or choose a file first."];
      this.renderBanner();
      return;
    }
    try {
      const result = importAppearance(text);
      this.draft = result.appearance;
      this.warnings = result.warnings;
      this.noticeTone = "warn";
      this.syncAll();
      this.applyDraft();
      this.hooks.toast(
        result.warnings.length > 0
          ? "Appearance imported with warnings."
          : "Appearance imported.",
      );
    } catch (error) {
      this.noticeTone = "error";
      this.warnings = [`Import failed: ${errorText(error)}`];
      this.renderBanner();
      this.hooks.toast("Import failed: invalid appearance JSON.");
    }
  }

  // --- Banner, save bar, preview -------------------------------------------

  private renderBanner(): void {
    if (!this.bannerHost) return;
    this.bannerHost.replaceChildren();
    if (this.warnings.length === 0) return;
    const banner = node("div", `banner ${this.noticeTone}`);
    banner.setAttribute("role", this.noticeTone === "error" ? "alert" : "status");
    const body = node("div", "banner-body");
    body.append(
      node(
        "strong",
        "banner-title",
        this.noticeTone === "error" ? "Import failed" : "Heads up",
      ),
    );
    const list = node("ul", "banner-list");
    for (const warning of this.warnings) list.append(node("li", undefined, warning));
    body.append(list);
    const dismiss = button("banner-dismiss", "Dismiss");
    dismiss.addEventListener("click", () => {
      this.warnings = [];
      this.renderBanner();
    });
    banner.append(body, dismiss);
    this.bannerHost.append(banner);
  }

  private buildSaveBar(): HTMLElement {
    const bar = node("div", "savebar");
    this.dirtyBadge = node("div", "dirty");
    const dot = node("span", "dirty-dot");
    dot.setAttribute("aria-hidden", "true");
    this.dirtyText = node("span", "dirty-text", "All changes saved");
    this.dirtyBadge.append(dot, this.dirtyText);

    const actions = node("div", "savebar-actions");
    this.revertButton = button("btn ghost", "Revert");
    this.revertButton.addEventListener("click", () => this.revert());
    this.saveButton = button("btn primary", "Save changes");
    this.saveButton.addEventListener("click", () => this.save());
    actions.append(this.revertButton, this.saveButton);
    bar.append(this.dirtyBadge, actions);
    return bar;
  }

  private renderPreview(): void {
    if (!this.previewHost) return;
    const appearance = this.draft;
    const frame = node("div", "preview-frame");

    const top = node("div", "preview-top");
    top.append(node("span", "preview-brand", "encchat"));
    const summary = `${styleLabel(appearance.layout.message_style)} · ${densityLabel(
      appearance.layout.density,
    )} · ${appearance.layout.max_width}px`;
    top.append(node("span", "preview-meta", summary));
    frame.append(top);

    if (appearance.layout.show_status_bar) {
      const status = node("div", "preview-status");
      const dot = node("span", "conn-dot");
      dot.dataset.state = "online";
      status.append(dot, node("span", "preview-status-text", "online · Room 4F2A-9C11 · 2 peers"));
      frame.append(status);
    }

    const log = node("div", "preview-log");
    if (appearance.timestamps.enabled && appearance.timestamps.system) {
      log.append(this.previewSystemLine());
    }
    const now = Date.now();
    log.append(
      this.previewMessage(false, PREVIEW_PEER, "Hey — this is a live preview.", new Date(now - 60000)),
      this.previewMessage(
        true,
        "",
        "Looks good. Nothing leaves this device unencrypted.",
        new Date(now),
      ),
    );
    frame.append(log);

    const composer = node("div", "composer preview-composer");
    const prompt = node("span", "preview-prompt", "›");
    const placeholder = node("span", "preview-placeholder", "Type a message…");
    const send = node("span", "preview-send", "Send");
    composer.append(prompt, placeholder, send);
    frame.append(composer);

    this.previewHost.replaceChildren(frame);
  }

  private previewSystemLine(): HTMLElement {
    const line = node("p", "system");
    const when = node(
      "time",
      "when",
      this.draft.timestamps.enabled
        ? formatTimestamp(this.draft.timestamps.format, new Date(Date.now() - 120000))
        : "",
    );
    const text = node("span", "system-text", "Secure session established with alice");
    line.append(when, text);
    return line;
  }

  private previewMessage(
    outgoing: boolean,
    device: string,
    body: string,
    at: Date,
  ): HTMLElement {
    const wrap = node("div", outgoing ? "msg outgoing" : "msg incoming");
    if (device) wrap.dataset.device = device;
    const meta = node("div", "meta");
    const who = node(
      "span",
      "who",
      outgoing ? this.previewOwnLabel() : this.previewPeerLabel(),
    );
    if (!outgoing && this.draft.layout.peer_colors) {
      who.style.color = colorToCss(
        peerColor(this.draft.theme.peer_palette, device),
        DEFAULT_COLOR_FALLBACKS.incoming_nick,
      );
    }
    const when = node(
      "time",
      "when",
      this.draft.timestamps.enabled
        ? formatTimestamp(this.draft.timestamps.format, at)
        : "",
    );
    when.title = at.toLocaleString();
    meta.append(who, when);
    const bubble = node("div", "bubble", body);
    wrap.append(meta, bubble);
    return wrap;
  }

  private previewPeerLabel(): string {
    if (this.draft.labels.show_peer_nicks) return "alice";
    if (this.draft.labels.show_device_ids) return shortId(PREVIEW_PEER);
    return "peer";
  }

  private previewOwnLabel(): string {
    return (
      this.displayName.trim() || this.draft.labels.own_fallback.trim() || "me"
    );
  }

  // --- State sync -----------------------------------------------------------

  private syncAll(): void {
    this.syncTheme();
    this.syncTypography();
    this.syncLayout();
    this.syncTimestamps();
    this.syncLabels();
    this.renderBanner();
    this.updateDirty();
    this.renderPreview();
  }

  private applyDraft(): void {
    applyAppearance(this.draft);
    this.hooks.onDraft(this.draft);
    this.updateAliasDots();
    this.renderPreview();
    this.updateDirty();
  }

  /** Recolor alias dots without rebuilding the inputs (which would drop focus). */
  private updateAliasDots(): void {
    if (!this.aliasesHost) return;
    for (const row of this.aliasesHost.querySelectorAll<HTMLElement>(".alias-row")) {
      const device = row.dataset.device;
      const dot = row.querySelector<HTMLElement>(".person-dot");
      if (!device || !dot) continue;
      dot.style.background = colorToCss(
        peerColor(this.draft.theme.peer_palette, device),
        DEFAULT_COLOR_FALLBACKS.incoming_nick,
      );
    }
  }

  private isDirty(): boolean {
    return (
      JSON.stringify(this.draft) !== JSON.stringify(this.saved) ||
      this.displayName.trim() !== this.savedNick
    );
  }

  private updateDirty(): void {
    const dirty = this.isDirty();
    this.dirtyText.textContent = dirty ? "Unsaved changes" : "All changes saved";
    this.dirtyBadge.dataset.dirty = String(dirty);
    this.saveButton.disabled = !dirty;
    this.revertButton.disabled = !dirty;
  }

  private revert(): void {
    this.draft = cloneAppearance(this.saved);
    this.displayName = this.savedNick;
    this.syncAll();
    this.applyDraft();
    this.hooks.toast("Reverted to the last saved appearance.");
  }

  private save(): void {
    let finalAppearance = this.draft;
    let warnings: string[] = [];
    try {
      const validated = importAppearance(formatAppearanceJson(this.draft));
      finalAppearance = validated.appearance;
      warnings = validated.warnings;
    } catch {
      // The draft is structurally valid (it came from a validated document),
      // so keep it as-is if re-validation unexpectedly fails.
    }
    try {
      saveAppearance(finalAppearance);
    } catch (error) {
      this.noticeTone = "error";
      this.warnings = [`Could not save to local storage: ${errorText(error)}`];
      this.renderBanner();
      this.hooks.toast("Could not save appearance settings.");
      return;
    }
    this.saved = cloneAppearance(finalAppearance);
    this.draft = cloneAppearance(finalAppearance);
    this.savedNick = sanitizeLabel(this.displayName);
    this.displayName = this.savedNick;
    this.warnings = warnings;
    this.noticeTone = "warn";
    this.syncAll();
    this.applyDraft();
    this.hooks.onSaved(this.saved, this.savedNick);
    this.hooks.toast(
      warnings.length > 0 ? "Appearance saved with warnings." : "Appearance saved.",
    );
  }

  private resetSection(id: SectionId): void {
    const defaults = this.defaults;
    switch (id) {
      case "theme":
        this.draft.theme = cloneAppearance(defaults.theme);
        break;
      case "typography":
        this.draft.typography = cloneAppearance(defaults.typography);
        break;
      case "layout":
        this.draft.layout = cloneAppearance(defaults.layout);
        break;
      case "timestamps":
        this.draft.timestamps = cloneAppearance(defaults.timestamps);
        break;
      case "labels":
        this.draft.labels = cloneAppearance(defaults.labels);
        break;
      case "profile":
        this.draft = cloneAppearance(defaults);
        break;
    }
    this.syncAll();
    this.applyDraft();
    this.hooks.toast("Section reset to defaults (not saved yet).");
  }
}

// --- Small builders ---------------------------------------------------------

function field(label: string, control: HTMLElement, hint?: string): HTMLElement {
  const wrapper = node("div", "field");
  const labelElement = node("label", "field-label", label);
  if (
    control instanceof HTMLInputElement ||
    control instanceof HTMLTextAreaElement ||
    control instanceof HTMLSelectElement
  ) {
    if (!control.id) control.id = `settings-field-${Math.random().toString(36).slice(2, 9)}`;
    labelElement.htmlFor = control.id;
  } else {
    labelElement.setAttribute("aria-hidden", "true");
  }
  const row = node("div", "field-control");
  row.append(labelElement, control);
  wrapper.append(row);
  if (hint) wrapper.append(node("span", "field-hint", hint));
  return wrapper;
}

function rangeRow(label: string, input: HTMLInputElement, output: HTMLElement): HTMLElement {
  const row = node("div", "range-row");
  const labelElement = node("label", "range-label", label);
  input.id = `settings-range-${Math.random().toString(36).slice(2, 9)}`;
  labelElement.htmlFor = input.id;
  row.append(labelElement, input, output);
  return row;
}

function styleLabel(style: MessageStyle): string {
  return style === "irc" ? "IRC" : "Bubbles";
}

function densityLabel(density: Density): string {
  return density.charAt(0).toUpperCase() + density.slice(1);
}
