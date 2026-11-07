import { expect, test } from "bun:test";
import type { SyncRecord } from "../lib/sync";
import * as vault from "./webos-vault";
import { webosTransport } from "./webos-transport";

/** The service end of the socket: the token, then keystore messages answered from one object; the rest is the core's. */
class ServiceSocket extends EventTarget {
  static OPEN = 1;
  readyState = 1;
  store: Record<string, unknown> = {};
  toCore: string[] = [];
  authed = false;
  constructor() {
    super();
    setTimeout(() => this.dispatchEvent(new Event("open")));
  }
  send(text: string) {
    if (!this.authed) return void (this.authed = text === "tok");
    if (!text.startsWith('{"host":"keystore"')) return void this.toCore.push(text);
    const m = JSON.parse(text) as { id: number; op: string; key: string; value: unknown };
    if (m.op === "write") {
      const { [m.key]: _, ...rest } = this.store;
      this.store = m.value === null ? rest : { ...rest, [m.key]: m.value };
    }
    const reply = { host: "keystore", id: m.id, ok: true, ...(m.op === "read" ? { value: this.store } : {}) };
    setTimeout(() => this.dispatchEvent(Object.assign(new Event("message"), { data: JSON.stringify(reply) })));
  }
}

test("on a TV the record and the Trakt records live in the service's keystore, without a passkey", async () => {
  let socket!: ServiceSocket;
  Object.assign(globalThis, { WebSocket: Object.assign(function () { return (socket = new ServiceSocket()); }, { OPEN: 1 }) });
  const transport = webosTransport("ws://127.0.0.1:47801/ws", "tok");
  const toBridge: string[] = [];
  transport.onMessage((t) => toBridge.push(t));
  const record = { account: "a", device: "d" } as unknown as SyncRecord;
  let opened = 0;
  vault.onOpened(() => opened++);

  expect(await vault.available()).toBe(false);
  expect(await vault.stored()).toBe(false);
  // Trakt stands on its own: kept with no sync account.
  await vault.saveTrakt({ p1: { t: 1 } });
  expect(socket.store).toEqual({ "trakt:p1": { t: 1 } });
  await vault.enable(record);
  expect(vault.active()).toBe(true);
  expect(opened).toBe(1);
  await vault.saveTrakt({ p1: { t: 1 }, p2: { t: 2 } });
  expect(await vault.holdsTrakt("p2")).toBe(true);
  expect(await vault.openTrakt()).toEqual({ p1: { t: 1 }, p2: { t: 2 } });
  await vault.forgetTrakt("p2");
  expect(await vault.unlock()).toEqual(record);
  expect(opened).toBe(2);
  // Leaving the account forgets its record; the Trakt sign-ins stay.
  await vault.clear();
  expect(socket.store).toEqual({ "trakt:p1": { t: 1 } });
  expect(vault.active()).toBe(false);
  // Keystore answers never reach the Bridge, and keystore messages never reach the core.
  expect(toBridge).toEqual([]);
  expect(socket.toCore).toEqual([]);
});

test("a keystore call after the socket closed fails at once instead of waiting forever", async () => {
  let socket!: ServiceSocket;
  Object.assign(globalThis, {
    WebSocket: Object.assign(function () { return (socket = new ServiceSocket()); }, { OPEN: 1 }),
    location: { reload: () => undefined, replace: () => undefined, hash: "" },
    sessionStorage: { getItem: () => null, setItem: () => undefined },
    window: globalThis,
  });
  webosTransport("ws://127.0.0.1:47801/ws", "tok");
  await vault.forgetTrakt("p9");
  socket.dispatchEvent(Object.assign(new Event("close"), { code: 1006 }));
  await expect(vault.stored()).rejects.toThrow("no webOS service");
});
