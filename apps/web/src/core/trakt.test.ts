/** The Trakt record's round trip through the real module and the real vault,
 * with the bridge and the profile session mocked and IndexedDB in memory:
 * nothing sent for a page that holds no record, each offered record sealed
 * then acknowledged, the open profile's sent again after a switch and a lock,
 * no acknowledgement after its session ended (the reviewer's ordering: a sealed
 * write that lands after a switch), storage failures said and never
 * acknowledged, and sign-out deleting the sealed copy. */

import { afterAll, expect, mock, test } from "bun:test";
import { createSignal } from "solid-js";
import { clientMock, fake, gone } from "./fake-client";

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

const [session, setSession] = createSignal<{ active: string; locked: boolean; incognitoHost?: string } | null>(null);
const resets: (() => void | Promise<void>)[] = [];
mock.module("./profiles", () => ({
  profiles: session,
  gateOpen: () => session() !== null && !session()?.locked,
  incognito: () => session()?.incognitoHost != null,
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
          getKey: (key: string) => ((reading = key), { result: disk.has(key) ? key : undefined }),
          getAllKeys: () => ({ get result() { return [...disk.keys()]; } }),
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
/** Waits for everything the module started (seals, writes, openings) to finish: no fixed sleep. */
const settled = () => trakt.traktSettled();
/** Waits until `ready` holds, yielding to the event loop between looks (for work held open on purpose). */
async function until(ready: () => boolean) {
  for (let i = 0; i < 10_000 && !ready(); i++) await new Promise((done) => setTimeout(done, 0));
  expect(ready()).toBe(true);
}
const live = () => views.filter((v) => !v.closed);
let revision = 0;
/** A record as the core offers one: each a new rotation. */
const record = (token: string) => ({ accessToken: token, refreshToken: `${token}-r`, createdAtMs: 0, expiresAtMs: 1, scope: "public", account: "u", user: "ana", revision: ++revision, generation: 1 });
const status = (extra: Record<string, unknown> = {}) => ({ state: "connected", user: "ana", lastOkMs: null, lastError: null, pendingWrites: 0, unmapped: [], importing: null, record: null, ...extra });
/** What the page told the core, in order, then forgotten. */
const told = () => sent.splice(0).map((s) => (s.name === "setTrakt" ? `setTrakt ${(s.fields.record as { accessToken: string }).accessToken}` : s.name));
/** A switch or a lock, as src/core/profiles.ts runs it: every reset, then the new session. */
async function reset(next: { active: string; locked: boolean; incognitoHost?: string }) {
  await Promise.all(resets.map((fn) => fn()));
  setSession(next);
  await settled();
}
const offer = async (token: string) => {
  live().at(-1)?.handlers.snapshot(status({ record: record(token) }));
  await settled();
};
/** A profile's sealed Trakt entry, as bytes on disk. */
const sealed = (profile = "p1") => disk.get(`trakt:${profile}`) as { iv: Uint8Array; data: ArrayBuffer } | undefined;

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

test("Incognito never opens Trakt; leaving it opens the host's again (plans/batch-10-incognito.md 5.6)", async () => {
  await reset({ active: "incognito", locked: false, incognitoHost: "p1" });
  expect(told()).toEqual(["close trakt"]);
  await reset({ active: "p1", locked: false });
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
  live().at(-1)?.handlers.snapshot(status({ record: record("t2") }));
  // The write for p1 is open (held) when the session switches to p2.
  await until(() => idb.hold === false);
  await Promise.all(resets.map((fn) => fn()));
  setSession({ active: "p2", locked: false });
  // p1's session is over (its view closed); p2's opening waits behind the held write.
  await until(() => sent.some((x) => x.name === "close trakt"));
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
  idb.failRead = "trakt:p1";
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

test("a record pushed again is stored and acknowledged once", async () => {
  const again = record("w1");
  live().at(-1)?.handlers.snapshot(status({ record: again }));
  live().at(-1)?.handlers.updates([status({ record: again }), status({ record: again })]);
  await settled();
  expect(told().filter((x) => x === "setTrakt w1")).toHaveLength(1);
});

test("two switches in a row open only the newest profile's view", async () => {
  // Back to back, as the core can report them, while the open view is still closing.
  expect(live()).toHaveLength(1);
  setSession({ active: "p2", locked: false });
  setSession({ active: "p3", locked: false });
  await settled();
  expect(live()).toHaveLength(1);
  // What that view offers is the newest profile's.
  await offer("v1");
  await reset({ active: "p1", locked: false });
  told();
  await reset({ active: "p3", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt v1", "subscribe trakt"]);
});

test("signing out while this browser's lock is closed deletes the sealed copy, and only then says signed out", async () => {
  // p1 and p2 each have a sealed sign-in.
  await reset({ active: "p1", locked: false });
  await offer("t5");
  await reset({ active: "p2", locked: false });
  await offer("u1");
  expect(sealed("p1") && sealed("p2")).toBeTruthy();
  // As after a reload: this browser remembers, its lock not opened yet.
  vault.drop();
  await reset({ active: "p1", locked: false });
  expect(trakt.traktLockedHere()).toBe(true);
  told();
  expect(await trakt.traktSignOut()).toBe(true);
  expect(sealed("p1")).toBeUndefined();
  expect(sealed("p2")).toBeDefined();
  expect(trakt.traktLockedHere()).toBe(false);
  // Unlocked again, the sign-out stays done: p1's sign-in does not come back, the others do.
  await vault.unlock();
  await settled();
  expect(Object.keys(await vault.openTrakt()).sort()).toEqual(["p2", "p3"]);
});

test("a profile the core says is gone loses its sealed sign-in and its record here, no key needed", async () => {
  // p3 holds a sealed sign-in (from the switches above); this browser's lock is closed.
  expect(sealed("p3")).toBeDefined();
  told();
  vault.drop();
  for (const fn of gone) fn("p3");
  await settled();
  expect(sealed("p3")).toBeUndefined();
  expect(sealed("p2")).toBeDefined();
  // Nothing of it is sent again: switching to it finds no record.
  await reset({ active: "p3", locked: false });
  expect(told()).toEqual(["close trakt", "subscribe trakt"]);
});

test("a store that failed never brings back a record signed out since", async () => {
  await vault.unlock();
  await reset({ active: "p1", locked: false });
  idb.failWrites = true;
  await offer("t7");
  const saveAgain = trakt.traktStorage()?.retry;
  expect(saveAgain).toBeDefined();
  idb.failWrites = false;
  told();
  expect(await trakt.traktSignOut()).toBe(true);
  expect(trakt.traktStorage()).toBeNull();
  // The Save again from before the sign-out seals and sends nothing.
  await saveAgain?.();
  await settled();
  expect(sealed("p1")).toBeUndefined();
  expect(told()).toEqual(["traktSignOut"]);
});

/** An entry in the earlier layout: every profile's record in one, sealed as the sync record is. */
async function earlierEntry(records: Record<string, unknown>) {
  const info = new TextEncoder().encode("marquee/web-record/v1");
  const base = await crypto.subtle.importKey("raw", new Uint8Array(32).fill(3), "HKDF", false, ["deriveKey"]);
  const key = await crypto.subtle.deriveKey({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info }, base, { name: "AES-GCM", length: 256 }, false, ["encrypt"]);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const data = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: info }, key, new TextEncoder().encode(JSON.stringify(records)));
  return { credential: new Uint8Array([9, 9, 9]).buffer, salt: new Uint8Array(32), iv, data };
}

test("the earlier single Trakt entry moves to one entry per profile at the next unlock", async () => {
  disk.set("trakt", await earlierEntry({ p6: record("old6") }));
  vault.drop();
  await reset({ active: "p6", locked: false });
  // Before the unlock it already counts as a sign-in kept here.
  expect(trakt.traktLockedHere()).toBe(true);
  await vault.unlock();
  await settled();
  expect(disk.has("trakt")).toBe(false);
  expect(sealed("p6")).toBeDefined();
  told();
  await reset({ active: "p1", locked: false });
  await reset({ active: "p6", locked: false });
  expect(told()).toEqual(["close trakt", "subscribe trakt", "close trakt", "setTrakt old6", "subscribe trakt"]);
});

test("an earlier entry that does not open goes at the unlock, and a sign-out deletes one too", async () => {
  const unreadable = { credential: new Uint8Array([9, 9, 9]).buffer, salt: new Uint8Array(32), iv: new Uint8Array(12), data: new Uint8Array(48).buffer };
  disk.set("trakt", unreadable);
  vault.drop();
  await vault.unlock();
  await settled();
  expect(disk.has("trakt")).toBe(false);
  // Locked again, an earlier entry left: signing out deletes it with the profile's own.
  disk.set("trakt", unreadable);
  vault.drop();
  expect(await trakt.traktSignOut()).toBe(true);
  expect(disk.has("trakt")).toBe(false);
  expect(sealed("p6")).toBeUndefined();
});
