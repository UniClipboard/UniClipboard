// Browser-side peer for the interoperability check (0022 step 4).
//
// It runs the pinned tailcat wasm build and the website's own protocol modules
// under bun, so unic is tested against the same code the page at
// try.uniclipboard.app ships. Nothing here is a reimplementation of the protocol.
//
//   bun harness.ts send <link-file> <text> [file...]   offer content, write the link, wait for delivery
//   bun harness.ts recv <link>                          pull and print the items as JSON
//
// UC_WEBSITE must name the uc-website checkout.
import { gunzipSync } from "node:zlib";
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const root = process.env.UC_WEBSITE;
if (!root) throw new Error("UC_WEBSITE is not set");
const lib = `${root}/src/lib/web-transfer`;
const { preparePayload } = await import(`${lib}/payload.ts`);
const { SendSession, pull } = await import(`${lib}/protocol.ts`);
const { newToken, buildLink, parseFragment } = await import(`${lib}/link.ts`);

const DERP_MAP_URL = "https://tailcat.dev/derpmap.json";
const build = "83921d71";
const dir = `${root}/public/tailcat`;

// wasm_exec.js expects a browser-like global.
(globalThis as any).window = globalThis;
await import(`${dir}/wasm_exec.${build}.js`);
const go = new (globalThis as any).Go();
const wasm = gunzipSync(readFileSync(`${dir}/main.${build}.wasm.gz`));
const { instance } = await WebAssembly.instantiate(wasm, go.importObject);
const ready = new Promise<void>((r) => ((globalThis as any).onTailcatReady = r));
void go.run(instance);
await ready;

const g = globalThis as any;
const [mode, ...rest] = process.argv.slice(2);

if (mode === "send") {
  const [linkFile, text, ...files] = rest;
  const drafts: any[] = [];
  if (text) drafts.push({ kind: "text", text });
  for (const f of files) {
    const bytes = readFileSync(f);
    drafts.push({ kind: "file", file: new File([bytes], basename(f), { type: "application/octet-stream" }) });
  }
  const payload = await preparePayload(drafts);
  const token = newToken();
  let delivered: () => void;
  let failed: (e: unknown) => void;
  const done = new Promise<void>((res, rej) => ((delivered = res), (failed = rej)));
  const session = new SendSession(payload, token, {
    onSendStart() {},
    onSendProgress() {},
    onDelivered: () => delivered(),
    onReplyStart() {},
    onReplyProgress() {},
    onReply() {},
    onFailed: (e: unknown) => failed(e),
  });
  const ln = await g.tailcatListen({ derpMapURL: DERP_MAP_URL, privateKey: "", onConnection: (c: any) => session.handleConnection(c) });
  writeFileSync(linkFile, buildLink("https://try.uniclipboard.app", "", { addr: ln.addr, token }, ""));
  const timer = setTimeout(() => failed(new Error("no receiver within 120 s")), 120_000);
  await done;
  clearTimeout(timer);
  console.log("DELIVERED");
  setTimeout(() => process.exit(0), 1500);
} else if (mode === "recv") {
  const ticket = parseFragment(new URL(rest[0]).hash);
  if (!ticket) throw new Error("not a transfer link");
  const stream = await g.tailcatDial({ addr: ticket.addr, derpMapURL: DERP_MAP_URL });
  const items = await pull(stream, ticket.token, { onOffer() {}, onProgress() {} }).result;
  console.log(JSON.stringify(items.map((i: any) => ({ kind: i.kind, name: i.name, mime: i.mime, size: i.size, text: i.kind === "text" ? new TextDecoder().decode(i.bytes) : undefined }))));
  setTimeout(() => process.exit(0), 500);
} else {
  throw new Error("usage: harness.ts send|recv ...");
}
