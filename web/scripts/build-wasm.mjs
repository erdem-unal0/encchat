// Cross-platform wrapper around `wasm-pack build`.
//
// wasm-pack shells out to `cargo`, which on many machines (including a plain
// `rustup` install on Windows) lives in ~/.cargo/bin but is not on PATH. We
// prepend that directory ourselves so `npm run build:wasm` works the same on
// Linux, macOS and Windows.

import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";

const cargoBin = join(homedir(), ".cargo", "bin");
const separator = process.platform === "win32" ? ";" : ":";
const env = {
  ...process.env,
  PATH: `${cargoBin}${separator}${process.env.PATH ?? ""}`,
};

const args = [
  "build",
  "../crates/wasm",
  "--target",
  "web",
  "--out-dir",
  "../../web/pkg",
];

const result = spawnSync("wasm-pack", args, {
  stdio: "inherit",
  env,
  // On Windows `wasm-pack` is a `.cmd` shim, so it needs a shell to run.
  shell: process.platform === "win32",
});

if (result.error) {
  console.error(`Failed to run wasm-pack: ${result.error.message}`);
  console.error("Install it with `cargo install wasm-pack` or `npm install -D wasm-pack`.");
  process.exit(1);
}

process.exit(result.status ?? 1);
