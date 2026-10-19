/** The Trakt record's round trip through the real module, the bridge and the
 * profile session mocked: `setTrakt` before the view opens, each offered
 * record stored then acknowledged, the open profile's record sent again after
 * a switch and after a lock, and deleted on sign-out. */

import { expect, mock, test } from "bun:test";
import { clientMock, fake } from "./fake-client";
import { createSignal } from "solid-js";

type Sent = { name: string; fields: Record<string, unknown> };
const sent: Sent[] = [];
type Handlers = { snapshot: (s: unknown) => void; updates: (u: unknown[]) => void };
const views: { view: string; handlers: Handlers; closed: boolean }[] = [];

mock.module("./client", clientMock);
fake.op = async (name, fields) => {
  sent.push({ name, fields });
  return name === "traktSync" ? { pulled: 0, applied: 0, pushed: 0, pending: 0, unmapped: 0 } : {};
};
fake.subscribe = async (view, _params, handlers) => {
  sent.push({ name: `subscribe ${view}`, fields: {} });
  const v = { view, handlers, closed: false };
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

const trakt = await import("./trakt");
const settled = () => new Promise((done) => setTimeout(done, 5));
const live = () => views.filter((v) => !v.closed);
const record = (token: string) => ({ accessToken: token, refreshToken: "r", createdAtMs: 0, expiresAtMs: 1, scope: "public", account: "u", user: "ana", revision: 0, generation: 1 });
const status = (extra: Record<string, unknown> = {}) => ({ state: "connected", user: "ana", lastOkMs: null, lastError: null, pendingWrites: 0, unmapped: [], importing: null, record: null, ...extra });
/** What the page told the core, in order, then forgotten. */
const told = () => sent.splice(0).map((s) => (s.name === "setTrakt" ? `setTrakt ${(s.fields.record as { accessToken?: string } | null)?.accessToken ?? "none"}` : s.name));
/** A switch or a lock, as src/core/profiles.ts runs it: every reset, then the new session. */
async function reset(next: { active: string; locked: boolean }) {
  await Promise.all(resets.map((fn) => fn()));
  setSession(next);
  await settled();
}

test("setTrakt goes before the view opens, and an offered record is stored, then acknowledged", async () => {
  setSession({ active: "p1", locked: false });
  await settled();
  expect(told()).toEqual(["setTrakt none", "subscribe trakt"]);
  live()[0]?.handlers.snapshot(status({ record: record("t1") }));
  await settled();
  expect(told()).toEqual(["setTrakt t1"]);
  // The state the screens read carries no record.
  expect(trakt.traktStatus()?.record).toBeNull();
  expect(trakt.traktStatus()?.user).toBe("ana");
});

test("a switch sends the other profile's record (none), and switching back sends this one again", async () => {
  await reset({ active: "p2", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt none", "subscribe trakt"]);
  await reset({ active: "p1", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt t1", "subscribe trakt"]);
});

test("after a lock the unlock sends the record again before the view reopens", async () => {
  await reset({ active: "p1", locked: true });
  expect(told()).toEqual(["close trakt"]);
  expect(trakt.traktStatus()).toBeNull();
  setSession({ active: "p1", locked: false });
  await settled();
  expect(told()).toEqual(["setTrakt t1", "subscribe trakt"]);
});

test("signing out deletes the record: the next open sends none", async () => {
  await trakt.traktSignOut();
  expect(told()).toEqual(["traktSignOut"]);
  await reset({ active: "p2", locked: false });
  await reset({ active: "p1", locked: false });
  expect(told()).toEqual(["close trakt", "setTrakt none", "subscribe trakt", "close trakt", "setTrakt none", "subscribe trakt"]);
});
