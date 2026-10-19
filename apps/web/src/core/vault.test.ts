/** The vault against a storage that answers late: what a call reported as
 * failed must never land afterwards. The browser parts are small fakes: an
 * IndexedDB whose open answers when the test says, a passkey with PRF, and
 * the 10 s timer fired by hand. */

import { expect, test } from "bun:test";
import type { SyncRecord } from "../lib/sync";
import * as vault from "./vault";

const g = globalThis as unknown as Record<string, unknown>;
const writes: string[] = [];
const signalled: string[] = [];
let openLate: (() => void) | null = null;
let timeUp: (() => void) | null = null;

function fakeDb() {
  return {
    close: () => undefined,
    transaction: () => {
      const t = { oncomplete: null as (() => void) | null, onabort: null, onerror: null, error: null, abort: () => undefined, objectStore: () => store };
      const done = <T>(result: T) => {
        queueMicrotask(() => t.oncomplete?.());
        return { result };
      };
      const store = {
        put: (_: unknown, key: string) => (writes.push(`put ${key}`), done(key)),
        delete: (key: string) => (writes.push(`delete ${key}`), done(undefined)),
        get: () => done(undefined),
      };
      return t;
    },
  };
}

g.indexedDB = {
  open: () => {
    const request: Record<string, unknown> = {};
    openLate = () => {
      request.result = fakeDb();
      (request.onsuccess as () => void)();
    };
    return request;
  },
};
g.window = { setTimeout: (fire: () => void) => ((timeUp = fire), 1) };
g.location = { hostname: "localhost" };
g.PublicKeyCredential = { signalUnknownCredential: async (o: { credentialId: string }) => void signalled.push(o.credentialId) };
Object.defineProperty(navigator, "credentials", {
  configurable: true,
  value: {
    create: async () => ({
      rawId: new Uint8Array([1, 2, 3]).buffer,
      getClientExtensionResults: () => ({ prf: { enabled: true, results: { first: new Uint8Array(32).fill(7).buffer } } }),
    }),
  },
});

async function until(ready: () => boolean) {
  for (let i = 0; i < 200 && !ready(); i++) await Bun.sleep(1);
  expect(ready()).toBe(true);
}

test("a Remember whose storage opens after the timeout writes nothing there, and its passkey is dropped", async () => {
  const outcome = vault.enable({} as SyncRecord).then(
    () => "sealed",
    (e: Error) => e.name,
  );
  // The sealed write waits on a storage that has not opened; then the 10 s are up.
  await until(() => openLate !== null && timeUp !== null);
  timeUp?.();
  expect(await outcome).toBe("TimeoutError");
  expect(signalled).toEqual(["AQID"]);
  // The storage opens late: the write it was asked for never starts.
  openLate?.();
  await Bun.sleep(5);
  expect(writes).toEqual([]);
  expect(vault.active()).toBe(false);
});
