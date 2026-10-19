/** The vault against a storage that answers late: what a call reported as
 * failed must never land afterwards, and a write the browser can no longer
 * stop is judged by how it ends. The browser parts are small fakes: an
 * IndexedDB whose open and commit answer when the test says, a passkey with
 * PRF, and the 10 s timer fired by hand. */

import { beforeEach, expect, test } from "bun:test";
import type { SyncRecord } from "../lib/sync";
import * as vault from "./vault";

const g = globalThis as unknown as Record<string, unknown>;
/** How the fake storage behaves in the next call. */
const storage = { openLate: false, committing: false };
const writes: string[] = [];
const signalled: string[] = [];
let openNow: (() => void) | null = null;
let settle: ((committed: boolean) => void) | null = null;
let timeUp: (() => void) | null = null;

beforeEach(() => {
  Object.assign(storage, { openLate: false, committing: false });
  writes.length = 0;
  signalled.length = 0;
  [openNow, settle, timeUp] = [null, null, null];
});

function fakeDb() {
  return {
    close: () => undefined,
    transaction: () => {
      const t = {
        oncomplete: null as (() => void) | null,
        onabort: null,
        onerror: null as (() => void) | null,
        error: null as DOMException | null,
        objectStore: () => store,
        abort: () => {
          // A committing transaction can no longer be stopped.
          if (storage.committing) throw new DOMException("committing", "InvalidStateError");
        },
      };
      const done = <T>(result: T) => {
        if (storage.committing)
          settle = (committed) => {
            if (committed) t.oncomplete?.();
            else {
              t.error = new DOMException("the disk failed", "UnknownError");
              t.onerror?.();
            }
          };
        else queueMicrotask(() => t.oncomplete?.());
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
    const answer = () => {
      request.result = fakeDb();
      (request.onsuccess as () => void)();
    };
    if (storage.openLate) openNow = answer;
    else queueMicrotask(answer);
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

/** Remember, as the page calls it: "sealed", or the error's name. */
const remember = () =>
  vault.enable({} as SyncRecord).then(
    () => "sealed",
    (e: Error) => e.name,
  );

test("a Remember whose storage opens after the timeout writes nothing there, and its passkey is dropped", async () => {
  storage.openLate = true;
  const outcome = remember();
  // The sealed write waits on a storage that has not opened; then the 10 s are up.
  await until(() => openNow !== null && timeUp !== null);
  timeUp?.();
  expect(await outcome).toBe("TimeoutError");
  expect(signalled).toEqual(["AQID"]);
  // The storage opens late: the write it was asked for never starts.
  openNow?.();
  await Bun.sleep(5);
  expect(writes).toEqual([]);
  expect(vault.active()).toBe(false);
});

test("a write still committing when the 10 s run out is judged by its own end: failed", async () => {
  storage.committing = true;
  let ended = "";
  void remember().then((o) => (ended = o));
  await until(() => settle !== null && timeUp !== null);
  timeUp?.();
  // Its outcome is unknown: nothing is reported and no passkey is dropped yet.
  await Bun.sleep(5);
  expect(ended).toBe("");
  expect(signalled).toEqual([]);
  settle?.(false);
  await until(() => ended !== "");
  expect(ended).toBe("UnknownError");
  expect(signalled).toEqual(["AQID"]);
  expect(vault.active()).toBe(false);
});

test("a write still committing when the 10 s run out is judged by its own end: remembered", async () => {
  storage.committing = true;
  let ended = "";
  void remember().then((o) => (ended = o));
  await until(() => settle !== null && timeUp !== null);
  timeUp?.();
  await Bun.sleep(5);
  expect(ended).toBe("");
  settle?.(true);
  await until(() => ended !== "");
  expect(ended).toBe("sealed");
  expect(writes).toEqual(["put sync"]);
  expect(signalled).toEqual([]);
  expect(vault.active()).toBe(true);
});
