/** The Trakt record's round trip through the real module and the real vault,
 * with the bridge and the profile session mocked and IndexedDB in memory:
 * nothing sent for a page that holds no record, each offered record sealed
 * then acknowledged, the open profile's sent again after a switch and a lock,
 * no acknowledgement after its session ended (the reviewer's ordering: a sealed
 * write that lands after a switch), storage failures said and never
 * acknowledged, and sign-out deleting the sealed copy. */

import { afterAll, expect, mock, test } from "bun:test";
import { createSignal } from "solid-js";
import { clientMock, fake } from "./fake-client";

const g = globalThis as unknown as Record<string, unknown>;
type Sent = { name: string; fields: Record<string, unknown> };
const sent: Sent[] = [];
type Handlers = { snapshot: (s: unknown) => void; updates: (u: unknown[]) => void };
const views: { handlers: Handlers; closed: boolean }[] = [];

mock.module("./client", clientMock);
fake.op = async (name, fields) => {
  sent.push({ name, fields });
  return {};
};
fake.subscribe = async (view, _params, handlers) => {
  sent.push({ name: `subscribe ${view}`, fields: {} });
  const v = { handlers, closed: false };
  views.push(v);
  return { sub: views.length, command: async () => undefined, close: async () => void ((v.closed = true), sent.push({ name: `close ${view}`, fields: {} })) };
};

const [session, setSession] = createSignal<{ active: string; locked: boolean } | null>(null);
const resets: (() => void | Promise<void>)[] = [];
mock.module("./profiles", () => ({
  profiles: session,
  gateOpen: () => session() !== null && !session()?.locked,
  onProfileReset: (fn: () => void | Promise<void>) => void resets.push(fn),
}));

/** IndexedDB in memory. `hold` keeps the next write's transaction open until `release()`;
 * `failWrites` ends writes in an error, `failRead` reads of that one entry. */
const disk = new Map<string, unknown>();
const idb = { hold: false, release: () => undefined as void, failWrites: false, failRead: null as string | null };
g.indexedDB = {
  open: () => {
    const request: Record<string, unknown> = {};
    request.result = {
      close: () => undefined,
      transaction: (_store: string, mode: string) => {
        const t = { oncomplete: null as (() => void) | null, onerror: null as (() => void) | null, onabort: null, error: null as DOMException | null, abort: () => undefined };
        const changes: (() => void)[] = [];
        let reading = "";
        const end = () => {
          if ((mode === "readwrite" && idb.failWrites) || (mode === "readonly" && reading === idb.failRead)) {
            t.error = new DOMException("the disk failed", "UnknownError");
            return t.onerror?.();
          }
          for (const c of changes) c();
          t.oncomplete?.();
        };
        const store = {
          get: (key: string) => ((reading = key), { result: disk.get(key) }),
          put: (value: unknown, key: string) => (changes.push(() => disk.set(key, value)), { result: key }),
          delete: (key: string) => (changes.push(() => disk.delete(key)), { result: undefined }),
        };
        queueMicrotask(() => {
          if (mode === "readwrite" && idb.hold) {
            idb.hold = false;
            idb.release = end;
          } else end();
        });
        return Object.assign(t, { objectStore: () => store });
      },
    };
    queueMicrotask(() => (request.onsuccess as () => void)());
    return request;
  },
};
g.window = { setTimeout: (f: () => void, ms: number) => setTimeout(f, ms) };
g.location = { hostname: "localhost" };
g.PublicKeyCredential = { signalUnknownCredential: async () => undefined };
Object.defineProperty(navigator, "credentials", {
  configurable: true,
  value: {
    create: async () => ({
      rawId: new Uint8Array([9, 9, 9]).buffer,
      getClientExtensionResults: () => ({ prf: { enabled: true, results: { first: new Uint8Array(32).fill(3).buffer } } }),
    }),
    // The passkey again (an unlock): it verifies the user (UV flag) and answers the same PRF.
    get: async () => ({
      response: { authenticatorData: new Uint8Array(37).fill(0x04).buffer },
      getClientExtensionResults: () => ({ prf: { results: { first: new Uint8Array(32).fill(3).buffer } } }),
    }),
  },
});

const vault = await import("./vault");
const trakt = await import("./trakt");
const settled = () => new Promise((done) => setTimeout(done, 10));
const live = () => views.filter((v) => !v.closed);
const record = (token: string) => ({ accessToken: token, refreshToken: `${token}-r`, createdAtMs: 0, expiresAtMs: 1, scope: "public", account: "u", user: "ana", revision: 0, generation: 1 });
const status = (extra: Record<string, unknown> = {}) => ({ state: "connected", user: "ana", lastOkMs: null, lastError: null, pendingWrites: 0, unmapped: [], importing: null, record: null, ...extra });
/** What the page told the core, in order, then forgotten. */
const told = () => sent.splice(0).map((s) => (s.name === "setTrakt" ? `setTrakt ${(s.fields.record as { accessToken: string }).accessToken}` : s.name));
/** A switch or a lock, as src/core/profiles.ts runs it: every reset, then the new session. */
async function reset(next: { active: string; locked: boolean }) {
  await Promise.all(resets.map((fn) => fn()));
  setSession(next);
  await settled();
}
const offer = async (token: string) => {
  live().at(-1)?.handlers.snapshot(status({ record: record(token) }));
  await settled();
};
/** The sealed Trakt entry, as bytes on disk. */
const sealed = () => disk.get("trakt") as { iv: Uint8Array; data: ArrayBuffer } | undefined;

afterAll(async () => {
  // The vault module is shared with the other test files: nothing remembered after this one.
  idb.failWrites = false;
  await vault.clear();
});

test("a page that holds no record sends nothing; an offered record is stored, then acknowledged", async () => {
  setSession({ active: "p1", locked: false });
  await settled();
  expect(told()).toEqual(["subscribe trakt"]);
  await offer("t1");
  expect(told()).toEqual(["setTrakt t1"]);
  // The state the screens read carries no record.
  expect(trakt.traktStatus()?.record).toBeNull();
  expect(trakt.traktStatus()?.user).toBe("ana");
});

test("a switch sends the other profile's record only if it has one; switching back sends this one", async () => {
  await reset({ active: "p2", locked: false });
  expect(told()).toEqual(["close trakt", "subscribe trakt"]);
  await reset({ active: "p1", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt t1", "subscribe trakt"]);
  await reset({ active: "p1", locked: true });
  expect(told()).toEqual(["close trakt"]);
  setSession({ active: "p1", locked: false });
  await settled();
  expect(told()).toEqual(["setTrakt t1", "subscribe trakt"]);
});

test("remembering seals the records under the passkey key, never in the clear", async () => {
  await vault.enable({ deviceToken: "sync" } as never);
  await settled();
  const entry = sealed();
  expect(entry).toBeDefined();
  expect(JSON.stringify(entry)).not.toContain("t1");
  expect(new TextDecoder().decode(entry?.data)).not.toContain("t1");
  expect(((await vault.openTrakt()) as Record<string, { accessToken: string }>).p1?.accessToken).toBe("t1");
  told();
});

test("a sealed write that lands after a switch acknowledges nothing for the new profile (the reviewer's ordering)", async () => {
  idb.hold = true;
  await offer("t2");
  // The write for p1 is still open when the session switches to p2.
  await reset({ active: "p2", locked: false });
  idb.release();
  await settled();
  expect(told()).toEqual(["close trakt", "subscribe trakt"]);
  // It was stored for p1 all the same: back on p1, that record goes.
  await reset({ active: "p1", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt t2", "subscribe trakt"]);
});

test("a sealed write that fails is said and not acknowledged; Save again stores and acknowledges it", async () => {
  idb.failWrites = true;
  await offer("t3");
  expect(told()).toEqual([]);
  expect(trakt.traktStorage()?.words).toContain("could not store the newest Trakt sign-in");
  idb.failWrites = false;
  await trakt.traktStorage()?.retry();
  await settled();
  expect(told()).toEqual(["setTrakt t3"]);
  expect(trakt.traktStorage()).toBeNull();
});

test("a sealed copy that could not be read is never overwritten", async () => {
  const before = sealed();
  // The passkey opens remembering again (an unlock): reading the Trakt entry fails.
  idb.failRead = "trakt";
  await vault.unlock();
  await settled();
  expect(trakt.traktStorage()?.words).toContain("could not read the Trakt sign-ins");
  await offer("t4");
  expect(sealed()).toEqual(before);
  expect(told()).toEqual([]);
  // Read again: the records meet, and t4 is sealed and acknowledged.
  idb.failRead = null;
  await trakt.traktStorage()?.retry();
  await offer("t4");
  expect(told()).toContain("setTrakt t4");
});

test("sign-out deletes the sealed copy; one that stays is said, not reported as gone", async () => {
  idb.failWrites = true;
  expect(await trakt.traktSignOut()).toBe(false);
  expect(trakt.traktStorage()?.words).toContain("could not delete the sign-in it keeps");
  idb.failWrites = false;
  await trakt.traktStorage()?.retry();
  expect(trakt.traktStorage()).toBeNull();
  expect(((await vault.openTrakt()) as Record<string, unknown>).p1).toBeUndefined();
  told();
  // The next opening for p1 sends nothing: the page holds no record for it.
  await reset({ active: "p2", locked: false });
  await reset({ active: "p1", locked: false });
  expect(told()).toEqual(["close trakt", "subscribe trakt", "close trakt", "subscribe trakt"]);
});
