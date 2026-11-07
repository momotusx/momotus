/** On a TV the sync record is kept and opened without a passkey (src/core/webos-vault.ts): a start opens what the
 * service's keystore holds, and a first join keeps the record there at once. Run against the real module with the
 * bridge mocked and the service's socket faked. */

import { afterAll, expect, mock, test } from "bun:test";
import { clientMock, fake } from "./fake-client";
import { webosTransport } from "./webos-transport";

const g = globalThis as unknown as Record<string, unknown>;
const saved = ["navigator", "WebSocket"].map((k) => [k, Object.getOwnPropertyDescriptor(globalThis, k)] as const);
const UA = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/79.0.3945.79 Safari/537.36";
Object.defineProperty(globalThis, "navigator", { value: { userAgent: UA, onLine: true }, configurable: true });
g.document ??= { addEventListener: () => undefined, hidden: false };
g.window ??= { setTimeout };

const kept = { account: "acct", deviceToken: "tv" };
const store: Record<string, unknown> = { sync: kept };
const rounds: unknown[] = [];
let failNextWrite = false;
class ServiceSocket extends EventTarget {
  static OPEN = 1;
  readyState = 1;
  constructor() {
    super();
    setTimeout(() => this.dispatchEvent(new Event("open")));
  }
  send(text: string) {
    if (!text.startsWith('{"host":"keystore"')) return;
    const m = JSON.parse(text) as { id: number; op: string; key: string; value: unknown };
    const failed = m.op === "write" && failNextWrite;
    failNextWrite = false;
    if (m.op === "write" && !failed) m.value === null ? delete store[m.key] : (store[m.key] = m.value);
    const reply = { host: "keystore", id: m.id, ok: !failed, value: m.op === "read" ? { ...store } : undefined };
    setTimeout(() => this.dispatchEvent(Object.assign(new Event("message"), { data: JSON.stringify(reply) })));
  }
}
g.WebSocket = ServiceSocket;
webosTransport("ws://127.0.0.1:47801/ws", "tok");

mock.module("./client", clientMock);
mock.module("../form", () => ({ form: () => "tv" }));
fake.op = async (name, fields) => {
  if (name === "syncNow") rounds.push((fields.record as { deviceToken: string }).deviceToken);
  if (name === "syncNow") return { record: null, fingerprint: "ABCD", again: false, refused: 0, unreadable: 0, newer: 0 };
  return {};
};

// Its own instance (the query): the module chooses its vault as it loads, and sync.test.ts loads the browser's.
const instance = "./sync?webos";
const sync = (await import(instance)) as typeof import("./sync");

afterAll(async () => {
  // Leave here only (the minute's round stops), then give the page back its browser.
  if (sync.joined()) await sync.signOut(true, true);
  for (const [k, d] of saved) d ? Object.defineProperty(globalThis, k, d) : delete g[k];
});

test("a start opens the kept account with no prompt, and a new join is kept at once", async () => {
  await sync.vaultReady;
  expect(sync.joined()).toBe(true);
  expect(sync.locked()).toBe(false);
  expect(sync.remembered()).toBe(true);
  expect(rounds[0]).toBe("tv");
  expect(await sync.passkeyAvailable()).toBe(false);

  // Leave here only, then a pairing joins another account: the keystore holds it with no Remember press.
  await sync.signOut(true, true);
  expect(store.sync).toBeUndefined();
  sync.join({ account: "acct2", deviceToken: "tv2" } as never);
  for (let i = 0; i < 100 && !sync.remembered(); i++) await new Promise((done) => setTimeout(done, 0));
  expect(store.sync).toEqual({ account: "acct2", deviceToken: "tv2" });
  expect(sync.remembered()).toBe(true);
});

test("a first keep that failed is kept by Save again", async () => {
  await sync.signOut(true, true);
  failNextWrite = true;
  sync.join({ account: "acct3", deviceToken: "tv3" } as never);
  for (let i = 0; i < 100 && !sync.memoryProblem(); i++) await new Promise((done) => setTimeout(done, 0));
  expect(store.sync).toBeUndefined();
  await sync.saveAgain();
  expect(store.sync).toEqual({ account: "acct3", deviceToken: "tv3" });
  expect(sync.remembered()).toBe(true);
});
