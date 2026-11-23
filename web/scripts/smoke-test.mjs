// Temporary runtime smoke test for the generated WASM bindings.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
  initSync,
  generate_mnemonic,
  room_fingerprint,
  WasmClient,
  new_salt,
  open_state,
  encode_relay,
} from "../pkg/encchat_wasm.js";

const here = dirname(fileURLToPath(import.meta.url));
initSync({ module: readFileSync(join(here, "..", "pkg", "encchat_wasm_bg.wasm")) });

let failures = 0;
function check(label, cond) {
  console.log(`${cond ? "ok  " : "FAIL"} ${label}`);
  if (!cond) failures += 1;
}

const mnemonic = generate_mnemonic();
check("mnemonic has 24 words", mnemonic.split(/\s+/).length === 24);
check("fingerprint looks right", /^[0-9A-F]{4}(-[0-9A-F]{4})+$/.test(room_fingerprint(mnemonic)));

const a = new WasmClient(mnemonic);
const b = new WasmClient(mnemonic);
const aId = a.device_id_hex();
const bId = b.device_id_hex();
check("device ids differ", aId !== bId);
check("A verifies B's MAC", a.verify_device(bId, b.device_mac_hex()) === true);
check("A rejects a forged MAC", a.verify_device(bId, "00".repeat(32)) === false);

// Noise handshake: A (initiator) -> B -> A -> B
let actions = b.handle_frame(aId, a.begin_handshake(bId));
const step1 = actions.find((x) => x.kind === "send")?.data;
check("B replies to handshake", step1 instanceof Uint8Array);

actions = a.handle_frame(bId, step1);
const step2 = actions.find((x) => x.kind === "send")?.data;
check("A finishes as initiator", actions.some((x) => x.kind === "handshake"));
check("A emits step 2", step2 instanceof Uint8Array);

actions = b.handle_frame(aId, step2);
check("B completes handshake", actions.some((x) => x.kind === "handshake"));
check("A has a session", a.has_session(bId) === true);
check("B has a session", b.has_session(aId) === true);

// Message A -> B
const sentAt = Date.now();
const outs = a.send_text(bId, "hello world", sentAt);
check("send_text returns one frame", outs.length === 1);
check("send_text target is B", outs[0].to === bId);
check("send_text data is Uint8Array", outs[0].data instanceof Uint8Array);

const received = b.handle_frame(aId, outs[0].data);
const msg = received.find((x) => x.kind === "received");
check("B decrypts the message", msg && msg.body === "hello world" && msg.sentAt === sentAt);

// Relay framing
const relay = encode_relay(bId, step1);
check("encode_relay returns bytes", relay instanceof Uint8Array && relay.length > step1.length);

// Encrypted state round-trip
const salt = new_salt();
const blob = a.encode_state("correct horse", salt);
const restored = open_state(mnemonic, "correct horse", salt, blob);
check("restored device id matches", restored.device_id_hex() === aId);
check("restored session survives", restored.has_session(bId) === true);

let threw = false;
try {
  open_state(mnemonic, "wrong", salt, blob);
} catch {
  threw = true;
}
check("wrong passphrase is rejected", threw);

console.log(failures === 0 ? "\nALL PASSED" : `\n${failures} FAILED`);
process.exit(failures === 0 ? 0 : 1);
