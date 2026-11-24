// Slash-command metadata and the pure parser behind the composer palette.
//
// Nothing here touches the DOM, the transport, or the WASM module: `main.ts`
// owns execution. Keeping the list and parsing pure makes both easy to reason
// about and keeps the palette filter deterministic.

export interface CommandSpec {
  /** Command name without the leading slash, e.g. `status`. */
  name: string;
  /** Argument hint shown next to the name, e.g. `[name]`. */
  args?: string;
  /** One short line shown muted in the palette and `/help`. */
  description: string;
  /** Commands with arguments complete to `"/name "` instead of `"/name"`. */
  takesArgs: boolean;
}

export interface ParsedCommand {
  name: string;
  args: string;
}

/** Ordered the way the palette and `/help` list them. */
export const COMMANDS: readonly CommandSpec[] = [
  {
    name: "help",
    description: "Show this command list",
    takesArgs: false,
  },
  {
    name: "clear",
    description: "Clear the conversation scrollback",
    takesArgs: false,
  },
  {
    name: "nick",
    args: "[name]",
    description: "Set or show your display name",
    takesArgs: true,
  },
  {
    name: "who",
    description: "List verified peers and session state",
    takesArgs: false,
  },
  {
    name: "status",
    description: "Connection, relay, room and device summary",
    takesArgs: false,
  },
  {
    name: "fingerprint",
    description: "Show the room fingerprint",
    takesArgs: false,
  },
  {
    name: "settings",
    description: "Open appearance settings",
    takesArgs: false,
  },
  {
    name: "theme",
    args: "[name]",
    description: "List or apply a theme preset",
    takesArgs: true,
  },
  {
    name: "timestamps",
    description: "Toggle message timestamps",
    takesArgs: false,
  },
  {
    name: "keys",
    description: "Show keyboard shortcuts",
    takesArgs: false,
  },
  {
    name: "connect",
    description: "Open the connection panel",
    takesArgs: false,
  },
  {
    name: "disconnect",
    description: "Disconnect from the relay",
    takesArgs: false,
  },
];

/** Match a typed filter (text after `/`) against command names, case-insensitively. */
export function filterCommands(query: string): CommandSpec[] {
  const needle = query.trim().toLowerCase().replace(/^\//, "");
  if (!needle) return [...COMMANDS];
  return COMMANDS.filter((command) => command.name.toLowerCase().startsWith(needle));
}

/**
 * Parse `/name args…` into its parts. Returns null when the input is not a
 * command or is just a bare slash. Arguments are trimmed.
 */
export function parseCommand(input: string): ParsedCommand | null {
  const trimmed = input.trimStart();
  if (!trimmed.startsWith("/")) return null;
  const body = trimmed.slice(1);
  const match = /^(\S+)(?:\s+([\s\S]*))?$/.exec(body);
  if (!match) return null;
  return {
    name: match[1].toLowerCase(),
    args: (match[2] ?? "").trim(),
  };
}

/** What Tab should insert for a command: `/nick ` vs `/status`. */
export function completeCommand(name: string, takesArgs: boolean): string {
  return takesArgs ? `/${name} ` : `/${name}`;
}

/** Look up a command by its bare name (case-insensitive). */
export function findCommand(name: string): CommandSpec | undefined {
  const needle = name.toLowerCase();
  return COMMANDS.find((command) => command.name === needle);
}
